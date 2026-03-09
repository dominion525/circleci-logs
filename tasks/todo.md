# TODO

現在未完了の項目はありません。

## 完了済み（アーカイブ）

### v0.1.0 コード品質改善
- エラーメッセージの英語統一、silent failure 修正、auth_header clone 排除
- Config Debug マスク、dead_code 個別化、clap arg_required_else_help
- serde_json unwrap 排除、run_job_log 分割、設定ファイルパーミッション警告
- JSON パース握りつぶし修正、--grep/--errors-only の JSON 対応
- userinfo 付き URL 解析修正、-w/-p での -j 専用オプション排除

### v0.1.0 機能追加
- タイムスタンプのローカルタイム表示
- 対話型 TUI (`-i` / `--interactive`)
- テスト結果取得 (`--tests` / `--failed-only`)
- URL 入力サポート
- `--fail-on-error` オプション
- Claude Code Skills 整備

### v0.2.1 パッチリリース
- `--version` / `-V` フラグ追加
- SKILL.md の `--json` ガイダンス緩和
- interactive.rs 配列アクセス安全性修正（5箇所）
- MSRV 明示 (`rust-version = "1.87.0"`)
- format_timestamp / colorize_status の共通化
- assert テスト修正、futures → futures-util 軽量化
