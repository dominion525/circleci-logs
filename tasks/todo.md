# TODO: コード品質改善

## 深刻度：高

- [x] エラーメッセージの言語統一
  - config.rs, api.rs のエラーメッセージが日本語になっている
  - プロジェクト指針ではCLIテキストは英語
  - 対象: config.rs (28, 30, 43, 76行目), api.rs (38-43行目)

- [x] main.rs:105 — ログ取得失敗の silent failure
  - `unwrap_or_default()` でエラーが完全に無視される
  - ユーザーにはログが空なのかAPI失敗なのか判別不能
  - eprintln! で警告を出す

## 深刻度：中

- [x] api.rs:28 — auth_header() でトークンを毎回 clone
  - `&str` を返せば不要なアロケーションを回避できる

- [x] Config の Debug 実装でトークン露出のリスク
  - derive(Debug) のままだとログ等でトークンが丸見え
  - カスタム Debug 実装でマスクする

- [x] models.rs:1 — `#![allow(dead_code)]` がファイル全体に適用
  - 未使用フィールドの検出が効かなくなる
  - 必要な構造体にだけ個別に付ける

- [x] main.rs:48-51 — clap の --help 手動呼び出し
  - `arg_required_else_help = true` で自動化できる

## 深刻度：低

- [x] output.rs — `serde_json::to_string_pretty().unwrap()` (3箇所)
  - パニックよりも丁寧なエラーハンドリングが望ましい

- [x] main.rs:74-111 — run_job_log の責務過多
  - ログ取得・フィルタリング・表示が一関数に混在
  - 分割を検討

- [x] 設定ファイルのパーミッション未検証 (Unix)
  - トークンを含む .circleci-logs.toml が誰でも読める状態でも警告なし
