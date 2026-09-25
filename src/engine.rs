use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File, Metadata},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Name,
    Size,
    Extension,
    Duplicate,
    Hash,
}

impl Mode {
    pub const ALL: [Self; 5] = [
        Self::Name,
        Self::Size,
        Self::Extension,
        Self::Duplicate,
        Self::Hash,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "ファイル名検索",
            Self::Size => "ファイルサイズ集計",
            Self::Extension => "拡張子検索",
            Self::Duplicate => "重複ファイル名検索",
            Self::Hash => "SHA-256比較",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Name => "名前にキーワードを含むファイルを、サブフォルダからも検索します。",
            Self::Size => "1つのフォルダ配下のファイルサイズを集計します（論理サイズ）。",
            Self::Extension => "指定した拡張子を持つファイルを、サブフォルダからも検索します。",
            Self::Duplicate => {
                "拡張子を含むファイル名が一致するものをグループで表示します。内容は比較しません。"
            }
            Self::Hash => "2つ以上のファイルのSHA-256を計算し、内容が一致するか比較します。",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Request {
    pub mode: Mode,
    pub paths: Vec<PathBuf>,
    pub query: String,
    pub case_sensitive: bool,
    pub parallel: bool,
    pub threads: usize,
}

#[derive(Default)]
pub struct Progress {
    pub cancel: AtomicBool,
    pub files: AtomicU64,
    pub directories: AtomicU64,
    pub bytes: AtomicU64,
}

impl Progress {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub path: PathBuf,
    pub size: u64,
    pub hash: Option<String>,
    pub group: Option<usize>,
}

impl Row {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }
}

#[derive(Debug)]
pub struct Report {
    pub mode: Mode,
    pub rows: Vec<Row>,
    pub total_size: u64,
    pub groups: usize,
    pub errors: Vec<String>,
    pub error_count: u64,
    pub skipped_links: u64,
    pub cancelled: bool,
    pub elapsed: Duration,
    pub parallel: bool,
}

impl Report {
    pub fn hashes_equal(&self) -> Option<bool> {
        if self.mode != Mode::Hash || self.cancelled || self.error_count != 0 || self.rows.len() < 2
        {
            return None;
        }
        Some(self.rows.iter().all(|r| r.hash == self.rows[0].hash))
    }
}

const ERROR_LIMIT: usize = 200;

#[derive(Default)]
struct Batch {
    directories: Vec<PathBuf>,
    rows: Vec<Row>,
    errors: Vec<String>,
    error_count: u64,
    skipped_links: u64,
}

impl Batch {
    fn error(&mut self, path: &Path, error: impl std::fmt::Display) {
        self.error_count += 1;
        if self.errors.len() < ERROR_LIMIT {
            self.errors.push(format!("{}: {error}", path.display()));
        }
    }

    fn merge(&mut self, mut other: Self) {
        self.directories.append(&mut other.directories);
        self.rows.append(&mut other.rows);
        self.error_count += other.error_count;
        self.skipped_links += other.skipped_links;
        self.errors.extend(
            other
                .errors
                .into_iter()
                .take(ERROR_LIMIT - self.errors.len()),
        );
    }
}

pub fn parse_extensions(query: &str) -> HashSet<String> {
    query
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '、'))
        .map(|s| {
            s.trim_start_matches("*.")
                .trim_start_matches('.')
                .to_lowercase()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

fn normalize_paths(request: &Request) -> Result<Vec<PathBuf>, String> {
    if request.paths.is_empty() {
        return Err("対象を追加してください。".into());
    }
    if request.mode == Mode::Size && request.paths.len() != 1 {
        return Err("サイズ集計ではフォルダを1つだけ指定してください。".into());
    }
    if request.mode == Mode::Extension && parse_extensions(&request.query).is_empty() {
        return Err("検索する拡張子を入力してください（例: pdf, txt）。".into());
    }
    if request.parallel && !(1..=64).contains(&request.threads) {
        return Err("並列スレッド数は1〜64にしてください。".into());
    }
    let mut paths = Vec::new();
    for path in &request.paths {
        let absolute = fs::canonicalize(path)
            .map_err(|e| format!("対象を開けません: {}: {e}", path.display()))?;
        let meta = fs::metadata(&absolute).map_err(|e| e.to_string())?;
        if request.mode == Mode::Hash {
            if !meta.is_file() {
                return Err(format!("ファイルを指定してください: {}", path.display()));
            }
        } else if !meta.is_dir() {
            return Err(format!("フォルダを指定してください: {}", path.display()));
        }
        paths.push(absolute);
    }
    paths.sort();
    paths.dedup();
    if request.mode == Mode::Hash {
        if paths.len() < 2 {
            return Err("比較する別々のファイルを2つ以上追加してください。".into());
        }
        return Ok(paths);
    }
    // Parents sort before their descendants. Avoid counting overlapping roots twice.
    let mut roots: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !roots.iter().any(|root| path.starts_with(root)) {
            roots.push(path);
        }
    }
    Ok(roots)
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Includes junctions and other reparse points; never recurse through these.
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn scan_directory(
    path: &Path,
    request: &Request,
    extensions: &HashSet<String>,
    needle: &str,
    progress: &Progress,
) -> Batch {
    let mut batch = Batch::default();
    if progress.cancelled() {
        return batch;
    }
    progress.directories.fetch_add(1, Ordering::Relaxed);
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => {
            batch.error(path, e);
            return batch;
        }
    };
    for entry in entries {
        if progress.cancelled() {
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                batch.error(path, e);
                continue;
            }
        };
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(e) => {
                batch.error(&path, e);
                continue;
            }
        };
        if is_link(&metadata) {
            batch.skipped_links += 1;
            continue;
        }
        if metadata.is_dir() {
            batch.directories.push(path);
        } else if metadata.is_file() {
            progress.files.fetch_add(1, Ordering::Relaxed);
            let matches = match request.mode {
                Mode::Name => {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if request.case_sensitive {
                        name.contains(needle)
                    } else {
                        name.to_lowercase().contains(needle)
                    }
                }
                Mode::Extension => path
                    .extension()
                    .is_some_and(|ext| extensions.contains(&ext.to_string_lossy().to_lowercase())),
                _ => true,
            };
            if matches {
                batch.rows.push(Row {
                    path,
                    size: metadata.len(),
                    hash: None,
                    group: None,
                });
            }
        }
    }
    batch
}

fn hash_file(path: &Path, progress: &Progress) -> Result<Option<Row>, io::Error> {
    if progress.cancelled() {
        return Ok(None);
    }
    let mut file = File::open(path)?;
    let before = file.metadata()?;
    if !before.is_file() {
        return Err(io::Error::other("通常のファイルではありません"));
    }
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut read_bytes = 0;
    loop {
        if progress.cancelled() {
            return Ok(None);
        }
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        read_bytes += count as u64;
        progress.bytes.fetch_add(count as u64, Ordering::Relaxed);
    }
    let after = file.metadata()?;
    if before.len() != after.len()
        || read_bytes != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(io::Error::other(
            "計算中にファイルが変更されました。再実行してください",
        ));
    }
    progress.files.fetch_add(1, Ordering::Relaxed);
    Ok(Some(Row {
        path: path.to_owned(),
        size: read_bytes,
        hash: Some(format!("{:x}", digest.finalize())),
        group: None,
    }))
}

pub fn run(request: &Request, progress: &Progress) -> Result<Report, String> {
    let started = Instant::now();
    let paths = normalize_paths(request)?;
    let pool = if request.parallel {
        Some(
            rayon::ThreadPoolBuilder::new()
                .num_threads(request.threads)
                .build()
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    let mut batch = Batch::default();
    if request.mode == Mode::Hash {
        let process = |path: &PathBuf| {
            let mut result = Batch::default();
            match hash_file(path, progress) {
                Ok(Some(row)) => result.rows.push(row),
                Ok(None) => (),
                Err(e) => result.error(path, e),
            }
            result
        };
        let results: Vec<_> = if let Some(pool) = &pool {
            pool.install(|| paths.par_iter().map(process).collect())
        } else {
            paths.iter().map(process).collect()
        };
        for result in results {
            batch.merge(result);
        }
    } else {
        let extensions = parse_extensions(&request.query);
        let needle = if request.case_sensitive {
            request.query.clone()
        } else {
            request.query.to_lowercase()
        };
        let mut directories = paths;
        while !directories.is_empty() && !progress.cancelled() {
            let process =
                |path: &PathBuf| scan_directory(path, request, &extensions, &needle, progress);
            let results: Vec<_> = if let Some(pool) = &pool {
                pool.install(|| directories.par_iter().map(process).collect())
            } else {
                directories.iter().map(process).collect()
            };
            for result in results {
                batch.merge(result);
            }
            directories = std::mem::take(&mut batch.directories);
        }
    }
    let mut groups = 0;
    if request.mode == Mode::Duplicate {
        let mut by_name = BTreeMap::<String, Vec<Row>>::new();
        for row in batch.rows {
            let key = if request.case_sensitive {
                row.name()
            } else {
                row.name().to_lowercase()
            };
            by_name.entry(key).or_default().push(row);
        }
        batch.rows = Vec::new();
        for (_, mut rows) in by_name {
            if rows.len() > 1 {
                groups += 1;
                rows.sort_by(|a, b| a.path.cmp(&b.path));
                for row in &mut rows {
                    row.group = Some(groups);
                }
                batch.rows.extend(rows);
            }
        }
    } else if request.mode == Mode::Size {
        batch
            .rows
            .sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    } else {
        batch.rows.sort_by(|a, b| a.path.cmp(&b.path));
    }
    Ok(Report {
        mode: request.mode,
        total_size: batch.rows.iter().map(|r| r.size).sum(),
        rows: batch.rows,
        groups,
        errors: batch.errors,
        error_count: batch.error_count,
        skipped_links: batch.skipped_links,
        cancelled: progress.cancelled(),
        elapsed: started.elapsed(),
        parallel: request.parallel,
    })
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}
