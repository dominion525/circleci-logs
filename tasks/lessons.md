# Lessons

## レビュー依頼時のツール選択

- README の構成レビューを外部 AI に依頼するときは、コードレビュー機能（`codex review --base`）ではなく、README の本文をプロンプトとして渡してドキュメントとして評価してもらうこと
- `codex review` はコード差分を分析するツールであり、ドキュメントの構成・読みやすさの評価には向かない
- 目的に合ったツールの使い方を選ぶ：ドキュメントレビューなら `chat` 系、コードレビューなら `review` 系

## clap の requires と group の相互作用

- clap derive で `#[arg(group = "target")]` を付けた引数に対して、別の引数から `requires = "field_name"` を指定しても、バリデーションが正しく発火しないケースがある
- `requires` の ID はフィールド名（`job_number`）でも `long` 名（`jid`）でもなく、後者に至ってはパニックする
- 対策: group 内の特定引数に依存するバリデーションは、`main()` 冒頭で手動チェックする方が確実
