---
name: circleci-logs
description: CircleCI のジョブログ・ワークフロー情報・パイプライン情報をコマンドラインから取得するツール。CIの失敗原因調査、ログ検索、テスト結果確認に使う。
---

# circleci-logs

CircleCI のビルド情報を CLI から取得するツール。

## いつ使うか

- CI が落ちた原因を調べたいとき
- ジョブのログを検索・フィルタしたいとき
- テスト結果を確認したいとき
- ワークフローやパイプラインの状態を一覧したいとき

## 前提

- 環境変数 `CIRCLE_TOKEN` が設定されていること
- GitHub/Bitbucket の git リポジトリ内で実行すること（プロジェクト自動検出）

## 基本的な使い方

### CI が落ちた → 失敗ログを見る

```bash
# ジョブ番号がわかっている場合
circleci-logs -j <JOB_NUMBER> --errors-only

# CircleCI の URL をそのまま渡す
circleci-logs "https://app.circleci.com/pipelines/github/org/repo/123/workflows/UUID/jobs/456" --errors-only
```

### ログからエラーを探す

```bash
circleci-logs -j <JOB_NUMBER> --grep "error|Error|ERROR"
```

### テスト結果を確認する

```bash
# 失敗テストだけ表示
circleci-logs -j <JOB_NUMBER> --tests --failed-only
```

### ワークフローのジョブ一覧を見る

```bash
circleci-logs -w <WORKFLOW_UUID>
```

### パイプラインのワークフロー一覧を見る

```bash
circleci-logs -p <PIPELINE_NUMBER>
```

## JSON 出力

`--json` フラグで JSON 形式出力。他ツールとの連携に便利。

```bash
circleci-logs -j <JOB_NUMBER> --json
circleci-logs -w <WORKFLOW_UUID> --json
```

## 終了コード制御

`--fail-on-error` でジョブにエラーがある場合に終了コード 1 を返す。

```bash
circleci-logs -j <JOB_NUMBER> --fail-on-error
```

## CircleCI の階層構造

```
Pipeline (番号)  → 1回の git push や定期実行で起動
 └─ Workflow (UUID) → ジョブの実行順序・依存関係
     └─ Job (番号)    → 個々の実行環境
         └─ Step      → 実際のコマンド実行、ログはここ
```

URL の深さに応じて自動的にモードが選択される：
- `/jobs/N` → ジョブログ表示
- `/workflows/UUID` → ジョブ一覧表示
- パイプライン番号まで → ワークフロー一覧表示
