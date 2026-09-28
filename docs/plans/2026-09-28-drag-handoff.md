# ドラッグ＆ドロップ 引き継ぎ書

作成：2026-09-28。実装は別のエージェントが行う。本書だけで着手できるように書く。
設計：[アップグレード設計](2026-09-28-drag-upgrade-design.md)。レビュー：[コードレビュー](../reviews/2026-09-28-drag-review.md)。

## 1. 現在の状態

- ブランチ `main`。2026-09-28 のセッションで Mac→Windows の受け渡しID化と関連修正を行い、両アプリへ配備した（Mac `build-20260928-123220-fb6ce7a`、Windows は同時刻の `win-…-fb6ce7a`）。ソースと文書の変更はコミット `735069a` にまとまっている。
- 変更範囲：`crates/common/src/drag.rs`、`crates/mac/src/{main.rs,file_drag.rs,gui.rs}`、`crates/win/src/{main.rs,dragdrop.rs,dragdrop/host.rs}`、`CHANGELOG.md`（[未リリース]）、`docs/drag-drop-investigation.md`、本書と設計書・レビュー。なお配備スクリプトが `BUILD_ID` 定数をソースへ書き込むため、配備後は `git status` が汚れる。
- **作業は `735069a` 以降に積み、自分の変更と混ぜない。**
- レビュー判定は変更要求（HIGH 5件）。L5（コメントの関数名）だけは本セッションで直した。
- 検証済み：Mac・共通の通常テスト95件、Windows 実機の通常テスト8件。未確認：Finder⇄Explorer の実操作（設計書の段階0）。

## 2. 環境の注意（必読）

1. `cargo` は `source $HOME/.cargo/env` の後に使う。PATH の既定 cargo には Windows ターゲットが無い。clippy は未導入。
2. Windows のビルド：`cargo test --locked -p tsunagu-win --target x86_64-pc-windows-gnu --no-run`。テスト実行はビルド出力の exe を `scp` で `home:C:/Users/owner/tsunagu/` へ送り、`ssh home` で実行後に削除する。exe 名のハッシュは依存で変わるので、出力の `Executable` 行から取る。
3. 配備スクリプト `scripts/deploy-win.sh`・`scripts/restart-mac.sh` は gitignore 済みのローカル専用。`deploy-win.sh` の Windows パスは `<user>` のまま（公開前の掃除の名残）で、そのまま実行すると壊れる。`sed 's/<user>/owner/g' scripts/deploy-win.sh > scripts/.deploy-win-tmp.sh` で一時コピーを作って実行し、実行後に削除する。両スクリプトとも `BUILD_ID` 定数をソースへ書き込む。
4. 配備は利用者が使用中のアプリを数秒止める。Windows 側から先に配備し、次に Mac を再起動する。配備後は両ログで `established` と `[bulk] established` を確認する。
5. `scripts/verify.sh` は入力とクリップボードを変更するため、全体では実行しない。
6. Windows の対話試験（`shared_mouse_release_completes_native_shell_drop` 等、`#[ignore]`）は対話セッションが必要。一時スケジュールタスクで実行し、**終了後に必ずタスクを削除**する。試験中は通常アプリを止め、失敗しても `tsunagu_run` と `tsunagu_watch` を復帰させる。
7. 同じリポジトリを別エージェントが編集することがある。大きな編集の前に対象ファイルの mtime と `git status` を確認し、衝突しそうなら worktree で作業する。
8. プロトコルの版を変える変更は、Mac と Windows を同時に配備する。旧版とも接続を保つ（`MIN_VERSION` は 11）。
9. 実機の GUI 操作（Computer Use での Finder 操作）は過去に長時間停止した。実操作の確認は利用者に依頼する。
10. 音が出る試験はしない。

## 3. 進め方

設計書の段階順に進める。各段階の完了条件は「実装・単体テスト・Windows 実機テスト・独立レビュー・配備・利用者による実操作確認」。実操作が未確認のうちは「修正済み」と書かない。

| 順 | 段階 | 担当の目安 | 着手条件 |
|---|---|---|---|
| 1 | 段階0 実機確認 | 利用者＋tester | すぐ |
| 2 | レビューの HIGH 以上の修正（H1・H2・H4・H5 はコードで閉じる。H3 は実機で挙動を確かめてから） | builder | 段階0と並行可 |
| 3 | 段階1 転送層の信頼性 | builder＋data-worker | 2の完了 |
| 4 | 段階2 状態管理の共通化 | builder | 3の完了 |
| 5 | 段階3 PoC → 本実装 | builder（PoC は単独で） | 4の完了。PoC の可否で分岐 |
| 6 | 段階4 フォルダ | builder | 3の完了（2・5と並行可） |
| 7 | 段階5 体験 | builder＋designer | 2の完了 |
| — | 段階6 検証基盤 | tester | 各段階と並行 |

レビューは各段階の終わりに reviewer（別コンテキスト・読み取り専用）で行う。作業の完了時は `~/.claude/os/SNAPSHOT.md` を更新する。

## 4. 貼り付け用の指示文

### 4.1 段階0（実機確認の準備）

```text
Tsunagu(/Users/taguchireo/ZCodeProject/tsunagu)のドラッグ&ドロップを実機確認する準備をしてください。日本語で対応。
docs/plans/2026-09-28-drag-handoff.md の「環境の注意」を守ること。コードは変更しない。
1. docs/plans/2026-09-28-drag-upgrade-design.md の段階0の表を、利用者が上から順に実行できる手順書にして output/2026-09-28-drag-e2e/checklist.md に置く。各手順に「確認するログの行(Mac /tmp/tsunagu-mac.log、Windows C:\Users\owner\tsunagu\tsunagu-win.log の [drag] 行)」を添える。
2. 利用者が実行した後に両ログから [drag]・[file]・[mode] 行を抜き出し、手順ごとの合否を output/2026-09-28-drag-e2e/result.json に記録するスクリプト(読み取りのみ)を用意する。
3. 手順書を利用者に示して止まる。実操作は利用者が行う。
```

### 4.2 レビュー指摘の修正

```text
Tsunagu(/Users/taguchireo/ZCodeProject/tsunagu)で、docs/reviews/2026-09-28-drag-review.md の重大度 HIGH 以上(CRITICAL・HIGH)の指摘を修正してください。日本語で対応。
docs/plans/2026-09-28-drag-handoff.md の「環境の注意」を守ること。着手前に未コミット差分の扱いを利用者に確認する。
- 指摘ごとに、まず失敗する単体テスト(共通クレートの純粋関数で書けるものはそこに)を書き、次に最小の修正を行う。
- 「要実機確認」の指摘は、修正後の確認手順を output/2026-09-28-drag-e2e/checklist.md に追記する。
- Mac・共通の cargo test と Windows 実機の通常テストを実行し、結果を出力ごと報告する。
- 別コンテキストのレビューを受けてから配備する。配備後に両側の再接続を確認する。
- レビュー文書の該当指摘に「対応済み(コミットID)」を追記する。
```

### 4.3 段階1（転送層の信頼性）

```text
Tsunagu(/Users/taguchireo/ZCodeProject/tsunagu)で docs/plans/2026-09-28-drag-upgrade-design.md の段階1(転送層の信頼性)を実装してください。日本語で対応。
docs/plans/2026-09-28-drag-handoff.md の「環境の注意」と設計書 §5 の互換方針を守ること。
範囲: 一時保存(.tsunagu-part)→検証→rename、FILE_END へのハッシュ、CANCEL フレームと転送ID、受信側の進捗イベント、Mac の FILE_TX_BUSY を直列キューへ置換、hello への caps 追加(VERSION 14)。
- 依存クレートの追加(ハッシュ)は理由と代替案を示し、利用者の承認を得てから行う。
- crates/common/src/lib.rs の bulk は巨大なので、変更前に該当範囲だけを読む。bulk モジュールの分割が必要なら、先に挙動を変えない分割コミットを作る。
- 受け入れ: 中断・再起動・同名・送信中の元ファイル更新の結合テストを crates/common/tests/ に追加し、旧版(caps なし)の相手との互換テストも書く。
- 両側を同時に配備し、通常のファイル送信・クリップボード・掴みドラッグ(両方向)が退行していないことを利用者に確認してもらう。
```

### 4.4 段階2（状態管理の共通化）

```text
Tsunagu(/Users/taguchireo/ZCodeProject/tsunagu)で docs/plans/2026-09-28-drag-upgrade-design.md の段階2(ドラッグ状態管理の共通化)を実装してください。日本語で対応。
docs/plans/2026-09-28-drag-handoff.md の「環境の注意」を守ること。
- crates/common/src/drag.rs に Session 状態機械(入力イベント→命令列)を作り、全遷移を表形式の単体テストで固定してから、既存の drag::Incoming / drag::Carried / win edge の Pending / mac incoming_drag の ACTIVE・COMMIT を置き換える。
- OS 依存部(COM/objc)は命令の実行だけにする。挙動を変える変更と置き換えのコミットを分ける。
- Mac→Win でも DragDone を返し、通知を OS の結果に合わせる。複数台接続では非アクティブな相手の遷移を止める(取消だけは受ける)。
- 既存の Windows 対話試験を本番経路で再実行し、一時タスクを必ず削除する。
```

### 4.5 段階3（PoC）

```text
Tsunagu(/Users/taguchireo/ZCodeProject/tsunagu)で docs/plans/2026-09-28-drag-upgrade-design.md の段階3「PoC で先に確かめること」1〜4を検証してください。製品コードは変更せず、crates/win/examples/ と crates/mac/examples/ に検証用プログラムを置く。日本語で対応。
docs/plans/2026-09-28-drag-handoff.md の「環境の注意」を守ること。Windows の GUI 試験は一時スケジュールタスクで実行し、終了後に削除する。
- Windows: CFSTR_FILEDESCRIPTORW + CFSTR_FILECONTENTS(IStream) + IDataObjectAsyncCapability を持つ IDataObject で、Explorer のフォルダへドロップさせ、IStream の読み出しを意図的に遅らせる・途中で失敗させる。DoDragDrop 終了後の読み出し継続、部分ファイルの残り方、COM の寿命を記録する。
- Mac: NSFilePromiseProvider で Finder へドロップさせ、書き出しを遅らせる・失敗させる。
- 自前 vtbl は SDK の IID とメソッド順を照合するテストを先に書く。
- 結果を output/2026-09-28-drag-lazy-poc/ に JSON と要約で残し、段階3を本実装するか(go/no-go)を根拠つきで提案して止まる。
```

### 4.6 段階4（フォルダ・多数項目）

```text
Tsunagu(/Users/taguchireo/ZCodeProject/tsunagu)で docs/plans/2026-09-28-drag-upgrade-design.md の段階4(フォルダ・多数項目)を実装してください。段階1の caps が入っていることが前提。日本語で対応。
docs/plans/2026-09-28-drag-handoff.md の「環境の注意」を守ること。
- 目録(相対パス・種別・サイズ)と FILE_BEGIN の rel、受信側の構造再現(一意なトップフォルダ、空フォルダ)。
- 名前規則(Windows 予約名、末尾の点・空白、禁止文字、大文字小文字衝突、260文字超)を crates/common の files モジュールで検出し、変換か拒否理由を返す。表形式の単体テストで固定する。
- シンボリックリンクは辿らず目録で除外と示す。範囲外への書き込み(.. や絶対パス)を拒否するテストを必ず書く。
- 相手の caps に bulk.tree が無い場合はフォルダを越える前に拒否し、理由を通知する。
```
