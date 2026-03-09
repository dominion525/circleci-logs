# CircleCI Private Output API

[English](circleci-private-output-api.md)

CircleCI ジョブのステップ出力を生テキストで取得する非公式・非公開 API。
実行中のジョブのログも取得可能。

> **注意**: これは非公開・非公式の API です。予告なく変更・廃止される可能性があります。

## エンドポイント

### ステップの標準出力（stdout）

```
GET /api/private/output/raw/{vcs}/{org}/{repo}/{job_number}/output/{task_index}/{step_id}
```

### ステップの標準エラー（stderr）

```
GET /api/private/output/raw/{vcs}/{org}/{repo}/{job_number}/error/{task_index}/{step_id}
```

## パスパラメータ

| パラメータ    | 型     | 説明                                                          |
|-------------|--------|--------------------------------------------------------------|
| vcs         | string | VCS 種別（`gh` = GitHub, `bb` = Bitbucket）                    |
| org         | string | Organization またはユーザー名                                    |
| repo        | string | リポジトリ名                                                    |
| job_number  | int    | CircleCI ジョブ番号                                             |
| task_index  | int    | v1.1 API アクションの `index` フィールド（並列ノード番号、0始まり）   |
| step_id     | int    | v1.1 API アクションの `step` フィールド（ステップ配列のインデックスではない）|

### step_id について

`step_id` は v1.1 API のアクションオブジェクトの `step` フィールドに対応する。
`steps[]` 配列のインデックスでは**ない**ことに注意。
値は連番ではなく飛び番になる（例: 0, 99, 101, 102, ...）。

正しい `step_id` を得るには、まず v1.1 API でジョブ詳細を取得する：

```
GET /api/v1.1/project/{vcs}/{org}/{repo}/{job_number}
```

レスポンス内の `action.step`（配列インデックスではない）を `step_id` として使う。

## 認証

```
Circle-Token: <APIトークン>
```

未認証リクエストは `404 {"message": "Build not found"}` を返す。

## レスポンス

### Content-Type

`application/octet-stream`

### ボディ

ステップの生テキスト出力。ターミナルカラー用の ANSI エスケープシーケンスを含む。
`error` エンドポイントの場合は stderr の内容。

### 主要レスポンスヘッダ

| ヘッダ                  | 値の例   | 説明                             |
|-----------------------|---------|----------------------------------|
| X-Terminal            | true    | ターミナル制御コードを含むことを示す    |
| X-RateLimit-Limit     | 300     | レート制限の上限                    |
| X-RateLimit-Remaining | 299     | 残りリクエスト数                    |
| X-RateLimit-Reset     | 1       | レート制限リセットまでの秒数          |
| Cache-Control         | private, max-age=3600 | キャッシュポリシー      |

## HTTP ステータスコード

| コード | 条件                                           |
|-------|-----------------------------------------------|
| 200   | 成功（ボディに出力テキスト）                        |
| 204   | コンテンツなし（例: stderr が空）                   |
| 404   | ビルドが見つからない、または認証失敗                  |

注: 無効な `step_id` は 404 ではなく 200（空ボディ）を返す。

## 使用例

```sh
# step_id=106、ノード0 の stdout を取得
curl -H "Circle-Token: $CIRCLE_TOKEN" \
  "https://circleci.com/api/private/output/raw/gh/myorg/myrepo/12345/output/0/106"

# 同ステップの stderr を取得
curl -H "Circle-Token: $CIRCLE_TOKEN" \
  "https://circleci.com/api/private/output/raw/gh/myorg/myrepo/12345/error/0/106"
```

## 参考実装

- [CircleCI MCP Server](https://github.com/CircleCI-Public/mcp-server-circleci) -
  `src/clients/circleci-private/jobsPrivate.ts`
- [CircleCI Kotlin Client](https://github.com/unhappychoice/circleci) -
  API クライアントライブラリ（v1.1 の output_url を使用、private API は未使用）
