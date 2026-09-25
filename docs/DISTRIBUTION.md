# Fast File Search

Windows x86_64向けの日本語ファイル検索・サイズ集計・SHA-256比較ツールです。

ZIPを展開して `Fast_File_Search.exe` を起動してください。Rustのインストールは不要です。日本語表示にはWindowsのメイリオ／游ゴシックを使います。

## 同梱ファイル

- `Fast_File_Search.exe`: アプリ本体
- `USAGE.md`: 操作方法と仕様（ソースからのビルド説明は開発者向け）
- `CHANGELOG.md`: 変更履歴
- `LICENSE`: 本プロジェクトのMITライセンス
- `THIRD_PARTY_LICENSES.html`: 依存ライブラリ・内蔵フォントのライセンス一覧
- `BUILD-INFO.txt`: アプリ・コンパイラのバージョンとビルド対象

内容の一致確認にはSHA-256比較を使ってください。重複ファイル名検索は名前のみを比較します。

配布物はコード署名されていません。問題を報告する際はバージョン、Windowsのバージョン、再現手順を添えて、配布元リポジトリのIssuesへお知らせください。
