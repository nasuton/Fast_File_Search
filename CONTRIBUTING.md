# 開発への参加

不具合はIssueテンプレートを利用し、アプリとWindowsのバージョン、処理方式、再現手順を記載してください。ファイルパス・画面画像の個人情報は伏せてください。

## 開発環境

- Windows x86_64、Rust 1.92以上（MSVC）
- Visual Studio Build Tools: C++によるデスクトップ開発、Windows SDK
- 配布スクリプトはPowerShell 7以上

```powershell
cargo run --locked --bin Fast_File_Search
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked
```

処理の変更では並列・逐次の双方を確認し、必要なら `tests/engine.rs` に回帰テストを追加してください。GUI変更は実際のウィンドウでも確認してください。

依存更新では `Cargo.lock` を一緒に更新し、最低対応Rustでのビルドと配布用ライセンス一覧の生成を確認してください。実行対象名 `Fast_File_Search` はIDE設定や配布スクリプトから参照されます。

## Pull Request

解決する問題、利用者から見た動作の変化、実施した確認を記載してください。仕様変更ではREADME・操作ガイド・CHANGELOGも更新します。

## ライセンス

投稿するコードは本プロジェクトのMITライセンスで公開できるものにしてください。第三者のコードや素材を追加する場合は出典と適用ライセンスを記載してください。
