use crate::engine::{Report, Row};
use std::io::{self, Write};

// Excel treats these prefixes as formulas even in quoted CSV fields.
fn safe_cell(value: &str) -> String {
    if value.starts_with(['=', '+', '-', '@', '\t', '\r', '\n']) {
        format!("'{value}")
    } else {
        value.to_owned()
    }
}

pub fn write_csv(writer: impl Write, report: &Report) -> io::Result<()> {
    let mut writer = writer;
    writer.write_all(&[0xef, 0xbb, 0xbf])?;
    let mut csv = csv::Writer::from_writer(writer);
    csv.write_record([
        "機能",
        "状態",
        "エラー件数",
        "グループ",
        "ファイル名",
        "パス",
        "サイズ (bytes)",
        "SHA-256",
    ])?;
    let status = if report.cancelled {
        "中止・部分結果"
    } else if report.error_count > 0 {
        "一部エラー"
    } else {
        "完了"
    };
    for Row {
        path,
        size,
        hash,
        group,
    } in &report.rows
    {
        csv.write_record([
            report.mode.label().to_owned(),
            status.to_owned(),
            report.error_count.to_string(),
            group.map(|g| g.to_string()).unwrap_or_default(),
            safe_cell(&path.file_name().unwrap_or_default().to_string_lossy()),
            safe_cell(&path.to_string_lossy()),
            size.to_string(),
            hash.clone().unwrap_or_default(),
        ])?;
    }
    csv.flush()
}
