use eframe::egui::{self, Color32, RichText};
use fast_file_search::{
    engine::{self, Mode, Progress, Report, Request, format_size},
    export,
};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::Ordering,
        mpsc::{self, Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};

const ACCENT: Color32 = Color32::from_rgb(82, 213, 182);
const MUTED: Color32 = Color32::from_rgb(150, 165, 184);
const WARNING: Color32 = Color32::from_rgb(255, 193, 107);

pub struct SearchApp {
    mode: Mode,
    folders: Vec<PathBuf>,
    size_folder: Vec<PathBuf>,
    files: Vec<PathBuf>,
    path_input: String,
    name_query: String,
    extension_query: String,
    case_sensitive: bool,
    parallel: bool,
    threads: usize,
    progress: Arc<Progress>,
    pending: Option<Receiver<Result<Report, String>>>,
    started: Option<Instant>,
    report: Option<Report>,
    selected: Option<usize>,
    message: String,
    font_warning: bool,
}

impl SearchApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(20, 27, 39);
        style.visuals.window_fill = Color32::from_rgb(26, 35, 48);
        style.visuals.selection.bg_fill = Color32::from_rgb(32, 94, 86);
        style.visuals.selection.stroke.color = Color32::WHITE;
        style.spacing.item_spacing = egui::vec2(10.0, 9.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        cc.egui_ctx.set_style(style);
        let font_warning = !install_japanese_font(&cc.egui_ctx);
        Self {
            mode: Mode::Name,
            folders: vec![],
            size_folder: vec![],
            files: vec![],
            path_input: String::new(),
            name_query: String::new(),
            extension_query: "pdf, txt".into(),
            case_sensitive: false,
            parallel: true,
            threads: std::thread::available_parallelism()
                .map_or(4, usize::from)
                .min(8),
            progress: Arc::new(Progress::default()),
            pending: None,
            started: None,
            report: None,
            selected: None,
            message: String::new(),
            font_warning,
        }
    }

    fn paths(&mut self) -> &mut Vec<PathBuf> {
        match self.mode {
            Mode::Hash => &mut self.files,
            Mode::Size => &mut self.size_folder,
            _ => &mut self.folders,
        }
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>) {
        if self.mode == Mode::Size {
            self.size_folder.clear();
        }
        let target = self.paths();
        for path in paths {
            if !target.contains(&path) {
                target.push(path);
            }
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        let request = Request {
            mode: self.mode,
            paths: self.paths().clone(),
            query: if self.mode == Mode::Extension {
                self.extension_query.clone()
            } else {
                self.name_query.clone()
            },
            case_sensitive: self.case_sensitive,
            parallel: self.parallel,
            threads: self.threads,
        };
        self.progress = Arc::new(Progress::default());
        let progress = Arc::clone(&self.progress);
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("file-search".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(|| engine::run(&request, &progress))
                    .unwrap_or_else(|_| Err("処理中に予期しないエラーが発生しました。".into()));
                let _ = sender.send(result);
                ctx.request_repaint();
            }) {
            Ok(_) => {
                self.pending = Some(receiver);
                self.started = Some(Instant::now());
                self.report = None;
                self.selected = None;
                self.message.clear();
            }
            Err(e) => self.message = format!("処理を開始できません: {e}"),
        }
    }

    fn poll(&mut self) {
        let Some(receiver) = &self.pending else {
            return;
        };
        match receiver.try_recv() {
            Ok(result) => {
                self.pending = None;
                match result {
                    Ok(report) => self.report = Some(report),
                    Err(e) => self.message = e,
                }
            }
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                self.message = "処理スレッドとの接続が切れました。再実行してください。".into();
            }
            Err(TryRecvError::Empty) => (),
        }
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        let busy = self.pending.is_some();
        egui::SidePanel::left("navigation")
            .exact_width(218.0)
            .resizable(false)
            .show(ctx, |ui| {
                ui.add_space(18.0);
                ui.heading(RichText::new("FAST FILE").strong().color(ACCENT));
                ui.label(RichText::new("SEARCH & ANALYZE").size(11.0).color(MUTED));
                ui.add_space(24.0);
                ui.label(RichText::new("ツール").small().color(MUTED));
                ui.add_enabled_ui(!busy, |ui| {
                    for mode in Mode::ALL {
                        if ui
                            .add_sized(
                                [196.0, 42.0],
                                egui::Button::new(mode.label()).selected(self.mode == mode),
                            )
                            .clicked()
                        {
                            self.mode = mode;
                            self.path_input.clear();
                            self.message.clear();
                        }
                    }
                });
                ui.add_space(24.0);
                ui.separator();
                ui.label(RichText::new("処理方式").color(MUTED));
                ui.add_enabled_ui(!busy, |ui| {
                    ui.radio_value(&mut self.parallel, true, "並列処理");
                    ui.radio_value(&mut self.parallel, false, "シングルスレッド");
                    ui.add_enabled_ui(self.parallel, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("スレッド数");
                            ui.add(egui::DragValue::new(&mut self.threads).range(1..=64));
                        });
                    });
                });
                ui.add_space(12.0);
                ui.label(
                    RichText::new(
                        "フォルダ探索はフォルダ単位、SHA-256はファイル単位で並列化します。",
                    )
                    .small()
                    .color(MUTED),
                );
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new(concat!("v", env!("CARGO_PKG_VERSION"), "  /  ローカル処理"))
                            .small()
                            .color(MUTED),
                    );
                    ui.label(
                        RichText::new("ファイル内容の変更は行いません")
                            .small()
                            .color(MUTED),
                    );
                });
            });
    }

    fn inputs(&mut self, ui: &mut egui::Ui) {
        let hash = self.mode == Mode::Hash;
        ui.horizontal(|ui| {
            ui.strong(if hash {
                "比較するファイル"
            } else {
                "対象フォルダ"
            });
            if ui
                .button(if hash {
                    "+ ファイルを選択"
                } else {
                    "+ フォルダを選択"
                })
                .clicked()
            {
                let dialog = rfd::FileDialog::new().set_title(if hash {
                    "比較するファイルを選択（複数可）"
                } else {
                    "対象フォルダを選択"
                });
                let paths = if hash {
                    dialog.pick_files()
                } else if self.mode == Mode::Size {
                    dialog.pick_folder().map(|p| vec![p])
                } else {
                    dialog.pick_folders()
                };
                if let Some(paths) = paths {
                    self.add_paths(paths);
                }
            }
            if ui.button("クリア").clicked() {
                self.paths().clear();
            }
        });
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.path_input)
                    .hint_text("パスを直接入力して追加することもできます")
                    .desired_width((ui.available_width() - 80.0).max(100.0)),
            );
            if ui.button("追加").clicked()
                || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                let path = self.path_input.trim().trim_matches('"');
                if !path.is_empty() {
                    self.add_paths(vec![PathBuf::from(path)]);
                    self.path_input.clear();
                }
            }
        });
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt("targets")
                .max_height(90.0)
                .show(ui, |ui| {
                    let paths = self.paths();
                    if paths.is_empty() {
                        ui.label(RichText::new("対象を追加してください").color(MUTED));
                    }
                    let mut remove = None;
                    for (index, path) in paths.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.small_button("削除").clicked() {
                                remove = Some(index);
                            }
                            ui.add(egui::Label::new(display_path(path)).truncate())
                                .on_hover_text(display_path(path));
                        });
                    }
                    if let Some(index) = remove {
                        paths.remove(index);
                    }
                });
        });
        match self.mode {
            Mode::Name => {
                ui.horizontal(|ui| {
                    ui.label("ファイル名");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.name_query)
                            .hint_text("部分一致 / 空欄で全ファイル")
                            .desired_width(320.0),
                    );
                    ui.checkbox(&mut self.case_sensitive, "大文字・小文字を区別");
                });
            }
            Mode::Extension => {
                ui.horizontal(|ui| {
                    ui.label("拡張子");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.extension_query)
                            .hint_text("例: pdf, .txt, *.jpg")
                            .desired_width(320.0),
                    );
                });
                ui.label(
                    RichText::new(
                        "カンマ・空白・セミコロンで複数指定。大文字・小文字は区別しません。",
                    )
                    .small()
                    .color(MUTED),
                );
            }
            Mode::Duplicate => {
                ui.checkbox(&mut self.case_sensitive, "大文字・小文字を区別");
            }
            Mode::Size => {
                ui.label(
                    RichText::new("サイズの大きい順に表示。隠しファイルも含みます。")
                        .small()
                        .color(MUTED),
                );
            }
            Mode::Hash => {
                ui.label(RichText::new("同じSHA-256のファイルは内容が一致します。ファイル名が異なっていても比較できます。").small().color(MUTED));
            }
        }
    }

    fn status(&mut self, ui: &mut egui::Ui) {
        let busy = self.pending.is_some();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(RichText::new("実行").color(Color32::from_rgb(12, 35, 31)))
                        .fill(ACCENT)
                        .min_size(egui::vec2(100.0, 36.0)),
                )
                .clicked()
            {
                self.start(ui.ctx());
            }
            if busy {
                if ui
                    .add_enabled(!self.progress.cancelled(), egui::Button::new("中止"))
                    .clicked()
                {
                    self.progress.cancel();
                }
                ui.spinner();
                let files = self.progress.files.load(Ordering::Relaxed);
                let elapsed = self.started.map_or(0.0, |s| s.elapsed().as_secs_f64());
                let status = if self.progress.cancelled() {
                    "中止しています"
                } else {
                    "処理中"
                };
                if self.mode == Mode::Hash {
                    ui.label(format!(
                        "{status}  /  {files} ファイル完了  /  {} 読込  /  {elapsed:.1} 秒",
                        format_size(self.progress.bytes.load(Ordering::Relaxed))
                    ));
                } else {
                    ui.label(format!(
                        "{status}  /  {files} ファイル確認  /  {} フォルダ  /  {elapsed:.1} 秒",
                        self.progress.directories.load(Ordering::Relaxed)
                    ));
                }
            }
        });
        if !self.message.is_empty() {
            ui.colored_label(WARNING, &self.message);
        }
    }

    fn save_csv(&mut self) {
        let Some(report) = &self.report else { return };
        if let Some(path) = rfd::FileDialog::new()
            .set_title("結果をCSVとして保存")
            .add_filter("CSV", &["csv"])
            .set_file_name("search-results.csv")
            .save_file()
        {
            // Never overwrite a file that was itself returned by the search.
            let existing = std::fs::canonicalize(&path).ok();
            if existing
                .as_ref()
                .is_some_and(|p| report.rows.iter().any(|row| &row.path == p))
            {
                self.message =
                    "検索対象のファイルには上書きできません。別の保存先を選択してください。".into();
                return;
            }
            match File::create(&path).and_then(|f| export::write_csv(f, report)) {
                Ok(()) => self.message = format!("CSVを保存しました: {}", path.display()),
                Err(e) => self.message = format!("CSVの保存に失敗しました: {e}"),
            }
        }
    }

    fn results(&mut self, ui: &mut egui::Ui) {
        let Some(report) = &mut self.report else {
            ui.add_space(35.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(if self.pending.is_some() {
                        "処理が終わると結果を表示します"
                    } else {
                        "対象を選択して「実行」を押してください"
                    })
                    .size(18.0)
                    .color(MUTED),
                );
                ui.label(
                    RichText::new("サブフォルダも対象 / リンク・ジャンクションは探索対象外")
                        .small()
                        .color(MUTED),
                );
            });
            return;
        };
        let mut export_requested = false;
        ui.horizontal(|ui| {
            ui.heading("結果");
            ui.label(report.mode.label());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                export_requested = ui
                    .add_enabled(!report.rows.is_empty(), egui::Button::new("CSVに保存"))
                    .clicked();
            });
        });
        let state = if report.cancelled {
            "中止（部分結果）"
        } else if report.error_count > 0 {
            "一部エラー"
        } else {
            "完了"
        };
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                if report.cancelled || report.error_count > 0 {
                    WARNING
                } else {
                    ACCENT
                },
                state,
            );
            ui.strong(format!("{} ファイル", report.rows.len()));
            ui.label(format!(
                "合計 {}（{} bytes）",
                format_size(report.total_size),
                report.total_size
            ));
            ui.label(format!(
                "{:.2} 秒 / {}",
                report.elapsed.as_secs_f64(),
                if report.parallel {
                    "並列"
                } else {
                    "シングル"
                }
            ));
            if report.mode == Mode::Duplicate {
                ui.label(format!("{} グループ", report.groups));
            }
        });
        if report.mode == Mode::Hash {
            match report.hashes_equal() {
                Some(true) => {
                    ui.colored_label(ACCENT, "すべてのファイルのSHA-256が一致しています。");
                }
                Some(false) => {
                    ui.colored_label(
                        WARNING,
                        "SHA-256が異なるファイルがあります。各行の値を確認してください。",
                    );
                }
                None => {
                    ui.colored_label(
                        WARNING,
                        "比較未完了：中止またはエラーのため、全ファイルの一致を判定できません。",
                    );
                }
            }
        }
        if report.skipped_links > 0 {
            ui.label(format!(
                "リンク・再解析ポイントを {} 件スキップしました。",
                report.skipped_links
            ));
        }
        if report.error_count > 0 {
            egui::CollapsingHeader::new(format!(
                "エラー {} 件（最大200件を表示）",
                report.error_count
            ))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("errors")
                    .max_height(100.0)
                    .show(ui, |ui| {
                        for error in &report.errors {
                            ui.colored_label(WARNING, error);
                        }
                    });
            });
        }
        ui.horizontal(|ui| {
            ui.label("並び順:");
            let sort = if ui.small_button("名前").clicked() {
                Some(0)
            } else if ui.small_button("サイズ（大きい順）").clicked() {
                Some(1)
            } else if ui.small_button("パス").clicked() {
                Some(2)
            } else if report.mode == Mode::Duplicate && ui.small_button("グループ").clicked() {
                Some(3)
            } else {
                None
            };
            if let Some(sort) = sort {
                match sort {
                    0 => report
                        .rows
                        .sort_by_cached_key(|r| (r.name().to_lowercase(), r.path.clone())),
                    1 => report
                        .rows
                        .sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path))),
                    2 => report.rows.sort_by(|a, b| a.path.cmp(&b.path)),
                    _ => report
                        .rows
                        .sort_by(|a, b| a.group.cmp(&b.group).then_with(|| a.path.cmp(&b.path))),
                }
                self.selected = None;
            }
        });
        ui.separator();
        // Virtualize rows so large result sets don't produce one widget per file per frame.
        let table_height = (ui.available_height() - 100.0).max(80.0);
        egui::ScrollArea::both()
            .id_salt("results")
            .max_height(table_height)
            .auto_shrink([false, false])
            .show_rows(ui, 27.0, report.rows.len() + 1, |ui, range| {
                ui.set_min_width(if report.mode == Mode::Hash {
                    1490.0
                } else {
                    1060.0
                });
                for index in range {
                    ui.horizontal(|ui| {
                        if index == 0 {
                            cell(ui, 64.0, "組", false);
                            cell(ui, 230.0, "ファイル名（クリックで詳細）", false);
                            cell(ui, 110.0, "サイズ", false);
                            if report.mode == Mode::Hash {
                                cell(ui, 530.0, "SHA-256", false);
                            }
                            cell(ui, 620.0, "パス", false);
                        } else {
                            let row = &report.rows[index - 1];
                            cell(
                                ui,
                                64.0,
                                &row.group
                                    .map(|g| g.to_string())
                                    .unwrap_or_else(|| "-".into()),
                                false,
                            );
                            let response = ui.add_sized(
                                [230.0, 27.0],
                                egui::Button::new(row.name())
                                    .selected(self.selected == Some(index - 1))
                                    .truncate(),
                            );
                            if response.clicked() {
                                self.selected = Some(index - 1);
                            }
                            cell(ui, 110.0, &format_size(row.size), false)
                                .on_hover_text(format!("{} bytes", row.size));
                            if let Some(hash) = &row.hash {
                                cell(ui, 530.0, hash, true);
                            }
                            cell(ui, 620.0, &display_path(&row.path), false)
                                .on_hover_text(display_path(&row.path));
                        }
                    });
                }
            });
        ui.separator();
        if let Some(row) = self.selected.and_then(|i| report.rows.get(i)) {
            ui.horizontal(|ui| {
                if ui.button("パスをコピー").clicked() {
                    ui.ctx().copy_text(display_path(&row.path));
                }
                if let Some(hash) = &row.hash
                    && ui.button("SHA-256をコピー").clicked()
                {
                    ui.ctx().copy_text(hash.clone());
                }
                ui.label(format!("{} bytes", row.size));
            });
            ui.add(egui::Label::new(display_path(&row.path)).truncate())
                .on_hover_text(display_path(&row.path));
        } else {
            ui.label(
                RichText::new("ファイル名をクリックすると、パスやSHA-256をコピーできます。")
                    .small()
                    .color(MUTED),
            );
        }
        if export_requested {
            self.save_csv();
        }
    }
}

impl eframe::App for SearchApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.sidebar(ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(12.0);
            ui.heading(RichText::new(self.mode.label()).size(26.0));
            ui.label(RichText::new(self.mode.description()).color(MUTED));
            if self.font_warning {
                ui.colored_label(
                    WARNING,
                    "Japanese font not found. Install Meiryo or Noto Sans CJK.",
                );
            }
            ui.add_space(8.0);
            let busy = self.pending.is_some();
            ui.add_enabled_ui(!busy, |ui| self.inputs(ui));
            self.status(ui);
            ui.separator();
            self.results(ui);
        });
        if self.pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

impl Drop for SearchApp {
    fn drop(&mut self) {
        self.progress.cancel();
    }
}

fn cell(ui: &mut egui::Ui, width: f32, text: &str, monospace: bool) -> egui::Response {
    let text = if monospace {
        RichText::new(text).monospace().size(12.0)
    } else {
        RichText::new(text)
    };
    ui.add_sized([width, 27.0], egui::Label::new(text).truncate())
}

fn display_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
    }
}

fn install_japanese_font(ctx: &egui::Context) -> bool {
    let windows = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/Windows"));
    let candidates = [
        windows.join("Fonts/meiryo.ttc"),
        windows.join("Fonts/YuGothM.ttc"),
        PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        PathBuf::from("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc"),
    ];
    for path in candidates {
        if let Ok(data) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("japanese".into(), egui::FontData::from_owned(data).into());
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .unwrap()
                .insert(0, "japanese".into());
            fonts
                .families
                .get_mut(&egui::FontFamily::Monospace)
                .unwrap()
                .push("japanese".into());
            ctx.set_fonts(fonts);
            return true;
        }
    }
    false
}
