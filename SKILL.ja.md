---
name: circleci-logs
description: CircleCI のジョブログ・テスト結果・ワークフロー状態・パイプライン情報を CLI から取得する。CI 失敗時のログ確認、テスト失敗の調査、ワークフローのジョブ一覧、パイプライン状態の確認に使う。ジョブ番号、ワークフロー UUID、パイプライン番号、CircleCI URL を直接受け付ける。
---

## 前提条件

- 環境変数 `CIRCLE_TOKEN` が設定されていること
- git リポジトリ内で実行すること（リモート URL からプロジェクトを自動検出）
- 任意: リポジトリルートの `.circleci-logs.toml` でプロジェクト設定を上書き可能

## クイックリファレンス

機械処理用には常に `--json` を使うこと。

| 目的                          | コマンド                                               |
|-------------------------------|--------------------------------------------------------|
| 失敗ステップのみ表示          | `circleci-logs -j JOB --errors-only --json`            |
| ジョブログを検索              | `circleci-logs -j JOB --grep "PATTERN" --json`         |
| テスト結果を取得              | `circleci-logs -j JOB --tests --json`                  |
| 失敗テストのみ取得            | `circleci-logs -j JOB --tests --failed-only --json`    |
| ワークフローのジョブ一覧      | `circleci-logs -w WORKFLOW_UUID --json`                 |
| パイプラインのワークフロー一覧| `circleci-logs -p PIPELINE_NUMBER --json`               |
| CircleCI URL を直接使用       | `circleci-logs "https://app.circleci.com/..." --json`   |

## CI 失敗の調査

典型的なドリルダウンフロー：

```bash
# 1. パイプライン番号からワークフロー一覧を表示
circleci-logs -p 142 --json

# 2. 失敗ワークフローの UUID でジョブ一覧を表示
circleci-logs -w "UUID" --json

# 3. 失敗ジョブの番号でエラーログを表示
circleci-logs -j 456 --errors-only --json

# 4. ログ全体から特定パターンを検索
circleci-logs -j 456 --grep "error|panic|FAILED" --json
```

CircleCI URL がある場合はドリルダウン不要：

```bash
# /jobs/N を含む URL → ジョブログを直接表示
circleci-logs "https://app.circleci.com/pipelines/github/org/repo/142/workflows/UUID/jobs/456" --errors-only --json
```

## テスト結果の確認

```bash
# 全テスト結果
circleci-logs -j JOB --tests --json

# 失敗テストのみ（最も有用）
circleci-logs -j JOB --tests --failed-only --json
```

注意: ジョブが CircleCI の `store_test_results` ステップを使用している必要がある。

## CircleCI URL の直接使用

CircleCI URL を位置引数として渡す。URL の深さに応じてモードを自動判定：

| URL の末尾               | モード                     |
|--------------------------|----------------------------|
| `/jobs/N`                | ジョブログ表示             |
| `/workflows/UUID`        | ワークフローのジョブ一覧   |
| `/pipelines/.../NUMBER`  | パイプラインのワークフロー一覧 |

```bash
circleci-logs "https://app.circleci.com/pipelines/github/org/repo/142/workflows/UUID/jobs/456" --json
```

## JSON 出力スキーマ

### ジョブログ (`-j JOB --json`)

```json
{
  "build_num": 456,
  "status": "failed",
  "steps": [
    {
      "name": "Run tests",
      "actions": [
        {
          "name": "Run tests",
          "status": "failed",
          "run_time_millis": 15000
        }
      ]
    }
  ],
  "logs": [
    {
      "step": "Run tests",
      "output": "... ログ全文 ..."
    }
  ]
}
```

フィールド: `build_num` (number|null), `status` (string|null), `steps` (array|null), `logs` (array)。
Action の status 値: `"success"`, `"failed"`, `"timedout"`, `"infrastructure_fail"`, `"canceled"`, `"running"`。

### ワークフロージョブ (`-w UUID --json`)

```json
[
  {
    "id": "job-uuid",
    "name": "build",
    "status": "success",
    "job_number": 456,
    "type": "build",
    "started_at": "2025-01-15T10:00:00Z",
    "stopped_at": "2025-01-15T10:02:30Z"
  }
]
```

フィールド: `id` (string), `name` (string), `status` (string), `job_number` (number|null), `type` (string|null), `started_at` (string|null), `stopped_at` (string|null)。

### パイプラインワークフロー (`-p NUMBER --json`)

```json
[
  {
    "id": "workflow-uuid",
    "name": "build-and-test",
    "status": "failed",
    "created_at": "2025-01-15T10:00:00Z",
    "stopped_at": "2025-01-15T10:05:00Z",
    "pipeline_number": 142
  }
]
```

フィールド: `id` (string), `name` (string), `status` (string), `created_at` (string|null), `stopped_at` (string|null), `pipeline_number` (number|null)。

### テスト結果 (`-j JOB --tests --json`)

```json
[
  {
    "name": "test_login",
    "classname": "AuthSpec",
    "result": "failure",
    "message": "Expected true got false",
    "run_time": 0.437,
    "source": "rspec",
    "file": "spec/auth_spec.rb"
  }
]
```

フィールド: すべて任意 (string|null、`run_time` のみ number|null)。`result` の値: `"success"`, `"failure"`, `"skipped"`。

## 終了コード

- デフォルト: 常に 0 で終了
- `--fail-on-error` 指定時: ジョブステータスが `"success"` でない場合（ログモード）またはテストに `"failure"`/`"failed"` がある場合（テストモード）に終了コード 1

## 制約

- `-j`, `-w`, `-p` は排他
- `--errors-only`, `--grep`, `--fail-on-error`, `--tests` は `-j` が必要
- `--failed-only` は `--tests` が必要
- `--tests` は `--errors-only`・`--grep` と併用不可
- URL は `-j`, `-w`, `-p` と併用不可
