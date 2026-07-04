# minicraft 開発ルール

- 開発セッションの開始時に `suggestion_box/README.md`(運用ルール)を読み、`status: open` の提案と `PRIORITIES.md` に目を通すこと。
- バージョンスコープの選定は `PRIORITIES.md` の遊び手優先度を最優先の入力とする。独断で見送る項目は PR 本文か suggestion_box に「見送りリスト」として必ず明記する(会話内だけに残さない)。
- バージョン開発の完了時に、開発フローの振り返りを `suggestion_box/` に1ファイル追加し、`PRIORITIES.md` の候補リストを更新する。
- 実装の地雷・教訓は `PROGRESS.md` の「ハマりポイント」に追記し、サブエージェントへの指示書に注入する。
- 環境ノート: macOS(coreutils なし、`timeout` コマンド不在)。テストのフルスイートは正常なら数秒で完走する(分単位はハングを疑う)。macroquad の API 確認は `~/.cargo/registry/src/*/macroquad-0.4.15/` を grep。
- worktree 分離のサブエージェントを起動する際は、分岐元ブランチを指示書に明記し、起動直後に `git log --oneline -3` で対象コードの存在を確認させること。
