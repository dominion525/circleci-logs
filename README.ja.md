# circleci-logs

CircleCI のジョブログ・ワークフロー情報・パイプライン情報をコマンドラインから取得するツール。

## クイックスタート

インストール:

```
cargo install circleci-logs
```

ジョブログを取得（トークンをインラインで渡す）:

```
CIRCLE_TOKEN=xxx circleci-logs -j 12345
```

繰り返し使うならトークンを環境変数に設定しておくと便利です:

```
export CIRCLE_TOKEN="your-circleci-token"
circleci-logs -j 12345
```

GitHub または Bitbucket の git リポジトリ内で実行すれば、プロジェクトは自動検出されます。
`CIRCLE_TOKEN` は [CircleCI の Personal API Tokens](https://app.circleci.com/settings/user/tokens) から発行できます。

## 使い方

CircleCI では以下の階層でビルドが管理されています。

```
Pipeline (123)         1回のgit pushや定期実行で起動される単位
 └─ Workflow (uuid)    ジョブの実行順序・依存関係を定義
     └─ Job (456)      個々の実行環境で動くステップの集まり
         └─ Step       実際のコマンド実行。ログはここに残る
```

本ツールはパイプライン・ワークフロー・ジョブの各階層に対応する 3 つのモードがあり、いずれか 1 つを指定します。

| 見たいもの | コマンド | ID の場所（CircleCI Web UI） |
|---|---|---|
| ジョブのログ | `circleci-logs -j <番号>` | URL 末尾の `.../jobs/456` |
| ワークフローのジョブ一覧 | `circleci-logs -w <UUID>` | URL 内の `.../workflows/<UUID>` |
| パイプラインのワークフロー一覧 | `circleci-logs -p <番号>` | URL 末尾の `.../pipelines/.../123` |

CircleCI Web UI の URL をそのまま渡すこともできます。URL の深さに応じて自動的にモードが選択されます。

```
# URL 末尾が /jobs/N → ジョブログを表示
circleci-logs "https://app.circleci.com/pipelines/github/org/repo/123/workflows/UUID/jobs/456"

# URL 末尾が /workflows/UUID → ジョブ一覧を表示
circleci-logs "https://app.circleci.com/pipelines/github/org/repo/123/workflows/UUID"

# URL 末尾がパイプライン番号 → ワークフロー一覧を表示
circleci-logs "https://app.circleci.com/pipelines/github/org/repo/123"
```

URL 内のプロジェクト情報と設定ファイル（または git remote）のプロジェクトが異なる場合は、URL 側が優先されます（警告あり）。`--tests`, `--json` 等のオプションも URL と併用できます。

全オプションは `circleci-logs --help` で確認できます。

### ジョブログの取得 (`-j` / `--jid`)

ジョブ番号を指定して、そのジョブのステップ一覧とログを表示します。

```
circleci-logs -j <JOB_NUMBER>
```

出力例:

```
$ circleci-logs -j 4504
Workflow: build-and-test  Job: test
Status: failed

[success] Spin up environment (2s)
[success] Checkout code (1s)
[failed]  Run tests (15s)

--- Run tests ---
FAILED src/app.test.ts:42
  Expected: 200
  Received: 500
```

オプション:

- `--errors-only` — 失敗したステップのみ表示
  ```
  circleci-logs -j 4504 --errors-only
  ```
- `--grep <PATTERN>` — 正規表現でログ行をフィルタ
  ```
  circleci-logs -j 4504 --grep "error"
  ```
- `--json` — JSON 形式で出力
- `--fail-on-error` — ジョブにエラーがある場合、終了コード 1 で終了
  ```
  circleci-logs -j 4504 --fail-on-error
  ```
- `--tests` — テスト結果を表示（`store_test_results` を使っているジョブ向け）
  ```
  circleci-logs -j 4504 --tests
  ```
- `--failed-only` — 失敗テストのみ表示（`--tests` と併用）
  ```
  circleci-logs -j 4504 --tests --failed-only
  ```

`--tests` の出力例:

```
$ circleci-logs -j 4504 --tests
Test Results: Job #4504

RESULT     TIME       FILE                           NAME
------------------------------------------------------------------------------------------
success    0.437s     tests/user.rb                  User.create validates email
failure    0.052s     tests/auth.rb                  Auth.login rejects invalid token
skipped    -          tests/legacy.rb                Legacy.import is deprecated

--- Failed Tests ---

[failure] Auth.login rejects invalid token
  File:  tests/auth.rb
  Class: spec.auth

  Expected true but got false
  at line 42 in auth_spec.rb

Summary: 1 passed, 1 failed, 1 skipped (0.489s)
```

`--tests` と `--errors-only`/`--grep` は排他で、同時には使えません。

### ワークフローのジョブ一覧 (`-w` / `--wid`)

ワークフロー ID を指定して、そのワークフローに含まれるジョブの一覧を表示します。

```
circleci-logs -w <WORKFLOW_ID>
```

出力例:

```
$ circleci-logs -w a1b2c3d4-5678-90ab-cdef-1234567890ab
JOB#     NAME                           STATUS       STARTED              STOPPED
------------------------------------------------------------------------------------------
4501     lint                           success      2025-01-15 19:00:05  2025-01-15 19:00:38
4502     build                          success      2025-01-15 19:00:06  2025-01-15 19:00:30
4503     unit-test                      success      2025-01-15 19:00:32  2025-01-15 19:04:15
4504     integration-test               failed       2025-01-15 19:04:18  2025-01-15 19:08:42
4505     deploy                         canceled     -                    -
```

オプション:

- `--json` — JSON 形式で出力

### 対話モード (`-i` / `--interactive`)

パイプライン → ワークフロー → ジョブ → ステップ → ログ を TUI で階層的にたどれるモードです。番号や UUID を調べる手間なく、選ぶだけで目的のログにたどり着けます。

```
circleci-logs -i
```

#### パイプライン → ワークフロー → ジョブ

```
? Select a pipeline
> #1042     main                           created      2025-03-08 14:32:01
  #1041     feat/new-feature               created      2025-03-08 13:15:42
  #1040     main                           created      2025-03-07 22:08:11

? Select a job
  .. (back to workflows)
  #5678     rspec                success      2025-03-08 14:32:10
  #5679     lint                 success      2025-03-08 14:32:08
> #5680     e2e                  failed       2025-03-08 14:33:01
  .. (back to workflows)
```

#### 並列ジョブのノード選択

並列実行（`parallelism > 1`）のジョブでは、CircleCI の Web UI と同様にノード一覧が表示されます。
各ノードの集約ステータスと合計実行時間が確認できます。

```
? Select a node
  .. (back to jobs)
> node 0    [success] 2m30s
  node 1    [failed ] 1m45s
  node 2    [success] 2m12s
  .. (back to jobs)
```

ノードを選ぶと、そのノードだけのステップ一覧が表示されます。

```
? Select a step
  .. (back to nodes)
  [success] Spin up environment              5s
  [success] Checkout code                    2s
> [failed ] RSpec                            1m38s
  [success] Upload results                   3s
  .. (back to nodes)
```

非並列ジョブの場合はノード選択をスキップして、直接ステップ一覧に入ります。

#### ログ表示

ステップを選択するとログが表示されます。

```
Node: 1  Step: RSpec
  [failed ] (1m38s)

FAILED spec/models/user_spec.rb:42
  Expected: 200
  Received: 500

? Log view
> Back to steps
  Exit
```

#### 操作方法

- ↑↓ キーで選択、Enter で決定
- 各リストの上下に戻り先付きの back があります（例: `.. (back to jobs)`）
- `▼ Load more...` で追加データの読み込み
- Esc で終了

#### URL からの途中開始

URL を渡すと途中の階層から開始できます。

```
circleci-logs -i "https://app.circleci.com/pipelines/github/org/repo/123"
circleci-logs -i "https://app.circleci.com/pipelines/github/org/repo/123/workflows/UUID"
```

注意事項:
- `-i` は `-j`/`-w`/`-p` および `--json`/`--errors-only`/`--grep`/`--tests`/`--failed-only`/`--fail-on-error` とは排他です
- ターミナル（TTY）が必要です。パイプ入力では動作しません

### パイプラインのワークフロー一覧 (`-p` / `--pid`)

パイプライン番号を指定して、そのパイプラインに含まれるワークフローの一覧を表示します。

```
circleci-logs -p <PIPELINE_NUMBER>
```

出力例:

```
$ circleci-logs -p 142
WORKFLOW ID                            NAME                      STATUS       CREATED              STOPPED
-------------------------------------------------------------------------------------------------------------------
a1b2c3d4-5678-90ab-cdef-1234567890ab   build-and-test            failed       2025-01-15 19:00:01  2025-01-15 19:08:42
b2c3d4e5-6789-01bc-defa-234567890abc   deploy                    canceled     2025-01-15 19:00:01  2025-01-15 19:08:45
```

オプション:

- `--json` — JSON 形式で出力

## プロジェクトの解決

プロジェクト（`vcs_type/org/repo`）は以下の優先順位で解決されます。

1. 設定ファイルの `project` フィールド
2. `git remote get-url origin` からの自動検出（GitHub / Bitbucket）
3. どちらも得られない場合はエラー

ほとんどのケースでは、GitHub/Bitbucket リポジトリ内で実行するだけで自動検出されるため、明示的な指定は不要です。

## 設定ファイル（任意）

プロジェクトのルートディレクトリに `.circleci-logs.toml` を作成すると、プロジェクトやトークンを明示的に指定できます。

```toml
project = "github/your-org/your-repo"   # 省略可（git remote から自動検出）
token = "your-circleci-token"            # 省略可（環境変数を推奨）
```

### トークンの優先順位

1. 環境変数 `CIRCLE_TOKEN`
2. 設定ファイル内の `token` フィールド

どちらも設定されていない場合はエラーになります。

### 探索ルール

カレントディレクトリから親ディレクトリ方向に `.circleci-logs.toml` を探索します。最初に見つかったファイルが使用されます。

```
/home/user/projects/myapp/src/   ← ここで実行
/home/user/projects/myapp/       ← .circleci-logs.toml があればこれを使用
/home/user/projects/
/home/user/
...
```

これにより、リポジトリのルートに設定ファイルを一つ置くだけで、サブディレクトリからも利用できます。

### パーミッション

トークンを設定ファイルに記載する場合は、パーミッションを制限し、`.gitignore` に追加してください。

```
chmod 600 .circleci-logs.toml
echo '.circleci-logs.toml' >> .gitignore
```

## AI エージェント連携

Claude Code 等の AI エージェントのスキルとして登録できます：

```bash
mkdir -p ~/.claude/skills/circleci-logs && circleci-logs --help-skill > ~/.claude/skills/circleci-logs/SKILL.md
```

## ライセンス

[MIT](LICENSE)
