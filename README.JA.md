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

GitHub または Bitbucket の git リポジトリ内で実行すれば、プロジェクトは自動検出されます。設定ファイルは不要です。

## 使い方

```
circleci-logs [OPTIONS] <--jid <JOB_NUMBER>|--wid <WORKFLOW_ID>|--pid <PIPELINE_NUMBER>>
```

3 つのモードがあり、いずれか 1 つを指定します。

### ジョブログの取得 (`-j` / `--jid`)

ジョブ番号を指定して、そのジョブのステップ一覧とログを表示します。

```
circleci-logs -j <JOB_NUMBER>
```

オプション:

- `--errors-only` — 失敗したステップのみ表示
- `--grep <PATTERN>` — 正規表現でログ行をフィルタ
- `--json` — JSON 形式で出力

### ワークフローのジョブ一覧 (`-w` / `--wid`)

ワークフロー ID を指定して、そのワークフローに含まれるジョブの一覧を表示します。

```
circleci-logs -w <WORKFLOW_ID>
```

オプション:

- `--json` — JSON 形式で出力

### パイプラインのワークフロー一覧 (`-p` / `--pid`)

パイプライン番号を指定して、そのパイプラインに含まれるワークフローの一覧を表示します。

```
circleci-logs -p <PIPELINE_NUMBER>
```

オプション:

- `--json` — JSON 形式で出力

### 使用例

ジョブ 12345 のログを取得する:

```
circleci-logs -j 12345
```

失敗したステップのログだけを表示する:

```
circleci-logs -j 12345 --errors-only
```

ログから "error" を含む行だけを抽出する:

```
circleci-logs -j 12345 --grep "error"
```

ワークフローのジョブ一覧を JSON で取得する:

```
circleci-logs -w abc12345-def6-7890-abcd-ef1234567890 --json
```

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

トークンを設定ファイルに記載する場合は、パーミッションを制限してください。

```
chmod 600 .circleci-logs.toml
```

## ライセンス

[MIT](LICENSE)
