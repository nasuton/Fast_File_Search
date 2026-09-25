use fast_file_search::{
    engine::{self, Mode, Progress, Request},
    export,
};
use std::{fs, path::Path, sync::atomic::Ordering};
use tempfile::TempDir;

fn fixture() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    for (name, data) in [
        ("a/report.txt", "abc"),
        ("b/report.txt", "different"),
        ("b/REPORT.TXT", "case"),
        ("a/deep/画像.JPG", "jpeg"),
        ("b/manual.pdf", "12345"),
        ("empty", ""),
        (".hidden", "x"),
    ] {
        // Use a different directory for the uppercase name on case-insensitive systems.
        let name = if name == "b/REPORT.TXT" {
            "c/REPORT.TXT"
        } else {
            name
        };
        let path = temp.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    }
    temp
}

fn request(mode: Mode, path: &Path) -> Request {
    Request {
        mode,
        paths: vec![path.to_owned()],
        query: String::new(),
        case_sensitive: false,
        parallel: false,
        threads: 3,
    }
}

fn names(report: &engine::Report) -> Vec<String> {
    let mut names: Vec<_> = report.rows.iter().map(|r| r.name()).collect();
    names.sort();
    names
}

#[test]
fn every_folder_mode_has_identical_serial_and_parallel_results() {
    let temp = fixture();
    for mode in [Mode::Name, Mode::Size, Mode::Extension, Mode::Duplicate] {
        let mut request = request(mode, temp.path());
        request.query = if mode == Mode::Extension {
            "txt, .JPG"
        } else {
            "report"
        }
        .into();
        let serial = engine::run(&request, &Progress::default()).unwrap();
        request.parallel = true;
        let parallel = engine::run(&request, &Progress::default()).unwrap();
        assert_eq!(serial.rows, parallel.rows);
        assert_eq!(serial.total_size, parallel.total_size);
        assert_eq!(serial.groups, parallel.groups);
        assert_eq!(serial.error_count, 0);
        assert_eq!(parallel.error_count, 0);
    }
}

#[test]
fn name_search_recurses_and_honors_case_option() {
    let temp = fixture();
    let mut request = request(Mode::Name, temp.path());
    request.query = "report".into();
    assert_eq!(
        engine::run(&request, &Progress::default())
            .unwrap()
            .rows
            .len(),
        3
    );
    request.case_sensitive = true;
    assert_eq!(
        engine::run(&request, &Progress::default())
            .unwrap()
            .rows
            .len(),
        2
    );
    request.query = "画像".into();
    assert_eq!(
        names(&engine::run(&request, &Progress::default()).unwrap()),
        ["画像.JPG"]
    );
    request.query.clear();
    assert_eq!(
        engine::run(&request, &Progress::default())
            .unwrap()
            .rows
            .len(),
        7
    );
}

#[test]
fn size_counts_all_files_including_empty_and_hidden() {
    let temp = fixture();
    let progress = Progress::default();
    let report = engine::run(&request(Mode::Size, temp.path()), &progress).unwrap();
    assert_eq!(report.rows.len(), 7);
    assert_eq!(report.total_size, 26);
    assert_eq!(report.rows.first().unwrap().size, 9);
    assert_eq!(report.rows.last().unwrap().size, 0);
    assert_eq!(progress.files.load(Ordering::Relaxed), 7);
}

#[test]
fn overlapping_duplicate_and_multiple_roots_are_not_counted_twice() {
    let temp = fixture();
    let other = tempfile::tempdir().unwrap();
    fs::write(other.path().join("other.txt"), "xyz").unwrap();
    let mut request = request(Mode::Name, temp.path());
    request.paths.extend([
        temp.path().join("a"),
        temp.path().to_owned(),
        other.path().to_owned(),
    ]);
    for parallel in [false, true] {
        request.parallel = parallel;
        let report = engine::run(&request, &Progress::default()).unwrap();
        assert_eq!(report.rows.len(), 8);
        assert_eq!(report.total_size, 29);
    }
}

#[test]
fn extension_search_accepts_common_separators_and_prefixes() {
    let temp = fixture();
    let mut request = request(Mode::Extension, temp.path());
    request.query = " .txt; *.JPG, pdf 、 txt ".into();
    assert_eq!(
        engine::run(&request, &Progress::default())
            .unwrap()
            .rows
            .len(),
        5
    );
    request.query = "jp".into();
    assert!(
        engine::run(&request, &Progress::default())
            .unwrap()
            .rows
            .is_empty()
    );
}

#[test]
fn duplicates_are_names_not_contents_and_require_distinct_paths() {
    let temp = fixture();
    let mut request = request(Mode::Duplicate, temp.path());
    request.paths.push(temp.path().join("a"));
    let report = engine::run(&request, &Progress::default()).unwrap();
    assert_eq!(report.groups, 1);
    assert_eq!(report.rows.len(), 3);
    assert!(report.rows.iter().all(|r| r.group == Some(1)));
    request.case_sensitive = true;
    assert_eq!(
        engine::run(&request, &Progress::default())
            .unwrap()
            .rows
            .len(),
        2
    );
}

#[test]
fn hash_uses_sha256_known_vectors_and_detects_differences() {
    let temp = fixture();
    let mut request = request(Mode::Hash, &temp.path().join("a/report.txt"));
    request.paths.push(temp.path().join("empty"));
    for parallel in [false, true] {
        request.parallel = parallel;
        let report = engine::run(&request, &Progress::default()).unwrap();
        assert_eq!(report.hashes_equal(), Some(false));
        assert_eq!(
            report.rows[0].hash.as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        assert_eq!(
            report.rows[1].hash.as_deref(),
            Some("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
        );
    }
}

#[test]
fn hashing_handles_multiple_read_buffers_and_matches_across_modes() {
    let temp = tempfile::tempdir().unwrap();
    let data = vec![0x61; 2 * 1024 * 1024 + 17];
    let paths: Vec<_> = ["one", "two", "three"]
        .map(|name| temp.path().join(name))
        .into();
    for path in &paths {
        fs::write(path, &data).unwrap();
    }
    let mut request = request(Mode::Hash, &paths[0]);
    request.paths = paths;
    let progress = Progress::default();
    let serial = engine::run(&request, &progress).unwrap();
    assert_eq!(serial.hashes_equal(), Some(true));
    assert_eq!(
        progress.bytes.load(Ordering::Relaxed),
        data.len() as u64 * 3
    );
    request.parallel = true;
    let parallel = engine::run(&request, &Progress::default()).unwrap();
    assert_eq!(serial.rows, parallel.rows);
}

#[test]
fn cancellation_never_claims_successful_hash_comparison() {
    let temp = fixture();
    for parallel in [false, true] {
        for mode in [Mode::Name, Mode::Hash] {
            let mut request = request(mode, temp.path());
            request.parallel = parallel;
            if mode == Mode::Hash {
                request.paths = vec![temp.path().join("empty"), temp.path().join("a/report.txt")];
            }
            let progress = Progress::default();
            progress.cancel();
            let report = engine::run(&request, &progress).unwrap();
            assert!(report.cancelled);
            assert!(report.rows.is_empty());
            assert_eq!(report.hashes_equal(), None);
        }
    }
}

#[test]
fn invalid_inputs_are_actionable_errors() {
    let temp = fixture();
    let mut request = request(Mode::Name, temp.path());
    request.paths.clear();
    assert!(engine::run(&request, &Progress::default()).is_err());
    request.paths = vec![temp.path().join("missing")];
    assert!(engine::run(&request, &Progress::default()).is_err());
    request.paths = vec![temp.path().join("empty")];
    assert!(engine::run(&request, &Progress::default()).is_err());
    request.mode = Mode::Hash;
    request.paths.push(temp.path().join("empty"));
    assert!(engine::run(&request, &Progress::default()).is_err());
    request.mode = Mode::Size;
    request.paths = vec![temp.path().to_owned(), temp.path().join("a")];
    assert!(engine::run(&request, &Progress::default()).is_err());
    request.mode = Mode::Extension;
    request.paths.truncate(1);
    assert!(engine::run(&request, &Progress::default()).is_err());
    request.mode = Mode::Name;
    request.parallel = true;
    request.threads = 0;
    assert!(engine::run(&request, &Progress::default()).is_err());
}

#[test]
fn empty_folder_is_a_valid_zero_result() {
    let temp = tempfile::tempdir().unwrap();
    let report = engine::run(&request(Mode::Size, temp.path()), &Progress::default()).unwrap();
    assert!(report.rows.is_empty());
    assert_eq!(report.total_size, 0);
    assert!(!report.cancelled);
}

#[test]
fn csv_preserves_unicode_commas_sizes_and_partial_status() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("=式,画像.txt"), "abc").unwrap();
    let mut report = engine::run(&request(Mode::Size, temp.path()), &Progress::default()).unwrap();
    report.cancelled = true;
    let mut output = Vec::new();
    export::write_csv(&mut output, &report).unwrap();
    assert!(output.starts_with(&[0xef, 0xbb, 0xbf]));
    let mut reader = csv::Reader::from_reader(&output[3..]);
    let record = reader.records().next().unwrap().unwrap();
    assert_eq!(&record[1], "中止・部分結果");
    assert_eq!(&record[4], "'=式,画像.txt");
    assert_eq!(&record[6], "3");
}

#[test]
fn incomplete_hash_reports_never_claim_equality() {
    let temp = tempfile::tempdir().unwrap();
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    fs::write(&a, "same").unwrap();
    fs::write(&b, "same").unwrap();
    let mut request = request(Mode::Hash, &a);
    request.paths.push(b);
    let mut report = engine::run(&request, &Progress::default()).unwrap();
    assert_eq!(report.hashes_equal(), Some(true));
    report.error_count = 1;
    assert_eq!(report.hashes_equal(), None);
}

#[test]
fn running_hash_can_be_cancelled_between_read_blocks() {
    let temp = tempfile::tempdir().unwrap();
    let a = temp.path().join("large-a");
    let b = temp.path().join("large-b");
    for path in [&a, &b] {
        fs::File::create(path)
            .unwrap()
            .set_len(32 * 1024 * 1024)
            .unwrap();
    }
    for parallel in [false, true] {
        let mut request = request(Mode::Hash, &a);
        request.paths.push(b.clone());
        request.parallel = parallel;
        let progress = Progress::default();
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| engine::run(&request, &progress).unwrap());
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while progress.bytes.load(Ordering::Relaxed) == 0
                && !worker.is_finished()
                && std::time::Instant::now() < deadline
            {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(progress.bytes.load(Ordering::Relaxed) > 0);
            progress.cancel();
            let report = worker.join().unwrap();
            assert!(report.cancelled);
            assert_eq!(report.hashes_equal(), None);
            assert!(progress.bytes.load(Ordering::Relaxed) < 64 * 1024 * 1024);
        });
    }
}

#[test]
fn directory_links_are_skipped_and_do_not_loop() {
    let temp = fixture();
    let link = temp.path().join("a/loop");
    make_directory_link(temp.path(), &link);
    for parallel in [false, true] {
        let mut request = request(Mode::Name, temp.path());
        request.parallel = parallel;
        let report = engine::run(&request, &Progress::default()).unwrap();
        assert_eq!(report.rows.len(), 7);
        assert_eq!(report.skipped_links, 1);
    }
}

fn make_directory_link(target: &Path, link: &Path) {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("cmd.exe")
            .args(["/C", "mklink", "/J"])
            .arg(link.to_string_lossy().replace('/', "\\"))
            .arg(target.to_string_lossy().replace('/', "\\"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
}
