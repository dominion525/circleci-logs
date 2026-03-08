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

## Round 2（Codex レビュー指摘）

- [x] api.rs:77 — JSON パース失敗の握りつぶし
  - `fetch_action_output` で `resp.json().await.unwrap_or_default()` を使用
  - パース失敗時に空ログとして扱われ、ユーザーは「ログが無い」のか「取得失敗」なのか判別不能
  - `.context(...)` でエラーを返し、呼び出し側の警告経路に乗せる

- [x] output.rs:67-70 — JSON 出力時に --grep フィルタが無効化される
  - `--jid ... --json --grep ...` の組み合わせでフィルタが効かない
  - テキスト出力では grep が効くが、JSON 分岐では生ログがそのまま出力される
  - JSON 分岐でも `filter_log_lines` を適用するか、組み合わせを明示的にエラーにする

## Round 3（Codex レビュー指摘）

- [x] output.rs — `--errors-only --json` で JSON 側にフィルタが反映されない
  - テキスト出力では失敗ステップのみ表示されるが、JSON ではすべてのアクションが含まれる
  - JSON 分岐でも同じフィルタを適用すべき

- [x] config.rs — userinfo 付き HTTPS URL の誤解析
  - `https://user@github.com/org/repo.git` でホスト名が `user@github.com` と誤認される
  - `@` より前を除去してホスト名だけで判定する必要がある

- [x] main.rs — `-w`/`-p` で `--errors-only`/`--grep` がサイレントに無視される
  - `-j` 専用オプションが `-w`/`-p` と組み合わせても受理されるが何も起こらない
  - clap の `requires` 等で `-j` 指定時のみ許可するか、明示的にエラーにすべき

## 機能追加

- [x] output.rs — タイムスタンプをローカルタイムゾーンで表示する
  - `chrono` クレート追加、`format_timestamp()` ヘルパーで UTC → ローカルタイム変換
  - `-w` の started_at/stopped_at、`-p` の created_at/stopped_at に適用
  - JSON 出力は API 生データのまま維持
  - ISO 8601 ベースの固定フォーマット (`YYYY-MM-DD HH:MM:SS`) を採用

- [ ] 対話型 TUI ドリルダウンモード (`-i` / `--interactive`)
  - パイプライン → ワークフロー → ジョブ → ログと階層的に選択・表示
  - ライブラリ: `dialoguer`（Select / FuzzySelect）
  - 実装ステップ:
    1. Cargo.toml に `dialoguer` 追加
    2. models.rs — `Pipeline` 構造体に branch, trigger 情報を追加（API レスポンスに含まれる）
    3. api.rs — `find_pipeline_uuid()` をリファクタし `fetch_pipelines()` として公開化（エンドポイント自体は既存）
    4. src/interactive.rs (新規) — ドリルダウンループ
       - TTY チェック（非 TTY なら「Use -j/-w/-p for non-interactive usage.」で終了）
       - パイプライン一覧（直近20件: 番号, ブランチ, ステータス, 作成日時）→ 選択
       - ワークフロー一覧 → 選択（先頭に「.. (back)」で戻れる導線）
       - ジョブ一覧 → 選択（同上）
       - ログ表示は既存の `print_job_log()` を再利用
    5. main.rs — `-i` フラグを `group = "target"` に追加、対話モードへの分岐
  - 規模: 小〜中（新規約120行、既存変更約50行、追加クレート1つ）
  - 注意: `--json`/`--errors-only`/`--grep` との組み合わせはエラーにする

- [x] テスト結果の取得 (`-j <JOB_NUMBER> --tests`)
  - CircleCI API v2: `GET /api/v2/project/{slug}/{job_number}/tests`
  - `store_test_results` で保存された構造化テスト結果を表示
  - テスト名、クラス、成否、実行時間、失敗メッセージを表示
  - `--failed-only` で失敗テストのみ表示
  - サマリー行: "45 passed, 2 failed, 3 skipped (12.345s)"
  - `--json` 対応
  - `store_test_results` 未使用のプロジェクトでは空結果を返す（エラーにはしない）

- [x] URL 入力サポート
  - CircleCI の URL を直接引数に渡せるようにする
  - `circleci-logs "https://circleci.com/gh/org/repo/12345"` → ジョブログ取得
  - `circleci-logs "https://app.circleci.com/pipelines/github/org/repo/123/workflows/abc/jobs/456"` → 同上
  - URL からプロジェクト・ジョブ番号等を自動パース
  - 既存の `-j`/`-w`/`-p` との共存: URL が渡されたら自動判別

- [x] `--fail-on-error` オプション
  - `-j` 専用。ジョブステータスが success 以外なら終了コード 1
  - `run_job_log` は `Result<bool>` を返し、`process::exit(1)` は main 側で実行（テスタブル）

- [ ] Claude Code Skills の整備
  - MCP サーバーは不要（CLI を Bash 経由で直接呼べば十分）
  - Skills で CLI の使い方・典型的なワークフローを記述
  - トークン消費を抑えつつ AI アシスタントとの連携を実現
