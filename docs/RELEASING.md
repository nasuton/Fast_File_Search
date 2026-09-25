# GitHub公開・リリース手順

## 初回公開

1. GitHubに公開リポジトリを作成します。既存ファイルをpushする場合はGitHub側でREADME等を自動生成しないと履歴の競合を避けられます。
2. `git status --short` で対象を確認します。`.idea/`・`target/`・`dist/`・環境変数ファイルは除外し、**Cargo.lockは含めます**。
3. コミットしてリモートにpushします。ブランチ名は利用しているものに合わせてください。
4. ActionsのCIが成功することを確認します。Windows / Rust 1.92.0とstableの2ジョブがあります。
5. GitHubのAbout欄に概要とTopicsを設定します。概要例: `Rust製の日本語GUIファイル検索・サイズ集計・SHA-256比較ツール。並列処理と逐次処理を切り替えて実行。` Topics例: `rust`, `egui`, `rayon`, `windows`, `file-search`, `sha256`。

公開URLは作成するリポジトリに依存するため、READMEに架空のURL・CI成功バッジは入れていません。作成後に必要なら `Cargo.toml` の `repository` とActionsの実URLによるバッジを追加してください。

## ローカルで配布物を作る

PowerShell 7、Rust 1.92以上、WindowsのMSVCビルド環境が必要です。

```powershell
cargo install cargo-about --version 0.9.1 --features cli --locked --root target/tools
pwsh -File scripts/package.ps1
```

初回はツール・依存ライブラリの取得にインターネット接続が必要です。ライセンス生成でも、crateに含まれないライセンスを元リポジトリから取得する場合があります。スクリプトはリリースモードのテスト、静的CRT付きWindowsビルド、依存ライセンス一覧、ZIPとSHA-256を生成します。検証やライセンス生成に失敗すると新しいZIPを作りません。

出力:

```text
dist/
  Fast_File_Search-v0.1.0-windows-x86_64.zip
  Fast_File_Search-v0.1.0-windows-x86_64.zip.sha256
```

`dist/` はGitに含めず、ReleaseのAssetsとして配布します。IDEのビルドとは別の `target/distribution/` を使用するため、起動中の開発用EXEと競合しません。同じバージョンで再実行すると、そのZIPとチェックサムを上書きします。

チェックサムを検証する例（ZIPと `.sha256` が同じフォルダにある場合）:

```powershell
$archive = 'dist/Fast_File_Search-v0.1.0-windows-x86_64.zip'
$expected = ((Get-Content "$archive.sha256" -Raw).Trim() -split '\s+')[0]
$actual = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
if ($actual -ine $expected) { throw 'Checksum mismatch' }
```

## GitHub ActionsでReleaseを作る

1. `Cargo.toml` のversion、`CHANGELOG.md`、`docs/releases/vX.Y.Z.md` を更新してコミット・pushします。GUIのバージョン表示はCargo.tomlから自動取得します。
2. 初回なら、公開したいコミットにタグを付けてpushします。

```powershell
git tag -a v0.1.0 -m "Release v0.1.0"
git push origin v0.1.0
```

3. ReleaseワークフローがタグとCargo.tomlのバージョン一致、fmt、Clippy、配布用テストを確認します。
4. ZIP・チェックサムを添付した **下書きRelease** が作られます。
5. AssetsのZIPを展開して手動確認し、ノートを確認してGitHub上のPublish releaseで公開します。

GitHub側の組織ポリシーでActionsの書込権限が制限されている場合は、Releaseジョブに `contents: write` を許可する必要があります。外部PATは使わず、そのジョブの `GITHUB_TOKEN` を使います。

Actionsの「Run workflow」をブランチから実行すると、公開せずZIP・チェックサムをActionsのArtifactとして取得できます。タグを選択して実行すると下書きReleaseも作成します。同じタグのReleaseが既に存在すると作成を停止します。公開済みタグの付け替えは避け、修正は新しいバージョンで行ってください。

## 手動確認

- ZIPを新しいフォルダに展開し、EXEを起動できる。
- 日本語表示、フォルダ／ファイル選択、複数対象の追加が動作する。
- 5機能を逐次・並列の両方で実行し、期待する結果が得られる。
- 大きなファイルのSHA-256計算を中止し、未完了の比較が一致と表示されない。
- CSVを保存でき、日本語・カンマを含む名前とバイト数を確認できる。
- リリース候補と同じバージョンがGUIに表示される。

ポートフォリオに画面画像を追加する場合は、実データや個人名が写らないテスト用フォルダで撮影し、`docs/images/` に保存してREADMEから参照してください。未計測の速度倍率や、未実行のCI結果は記載しないでください。

参考: [GitHub CLIのRelease作成](https://cli.github.com/manual/gh_release_create)、[cargo-about](https://embarkstudios.github.io/cargo-about/)。
