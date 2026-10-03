# 顧客検証計画(2026-10-01)

Startup Mentor(塾長メンター)の枠組みで整理した、Knit の事業仮説の検証計画。
製品の磨き込みに戻る前に、仮説のうち最も壊れやすい点を最小コストで確認する。

## 背景:Web の声と既存KVMの状態(2026-10-01 時点の調査)

- 既存 OSS KVM(Barrier・Input Leap)は disconnect/sleep 系のオープン Issue が各50件超
  (検索上限到達)。Input Leap は 2025 年も「スリープ復帰で接続できない」「Connection cycling」
  「macOS 15.5 でクラッシュ」など新規報告が続く([Barrier Issues](https://github.com/debauchee/barrier/issues)、
  [Input Leap](https://github.com/input-leap/input-leap/issues))
- Reddit の声:スリープ復帰の間欠切断([r/logitech](https://www.reddit.com/r/logitech/comments/y28a8h/logitech_flow_intermittent_disconnections_after))、
  macOS 13.1 でキーボード転送が完全に壊れる([Barrier #1869](https://github.com/debauchee/barrier/issues/1869))、
  「4秒ごとに切断」([r/VFIO](https://www.reddit.com/r/VFIO/comments/6djbbb/vnc_vs_kvm_software_vs_synergy_for_mk_sharing_on/))
- 支払い実勢:Synergy は $9/年([r/SynergyApp](https://www.reddit.com/r/SynergyApp/comments/141jdkj/so_synergy_3_is_9_per_year))、
  セール時 $10 買い切り([r/macapps](https://www.reddit.com/r/macapps/comments/1gqvkty/synergy_for_980))。
  「ライセンス買って後悔。追加機能が要らなかった」([r/MouseReview](https://www.reddit.com/r/MouseReview/comments/11vu5k7/does_anyone_here_have_experience_with_symless))
- Mac↔Windows のファイル受け渡しは AirDrop 非対応のため、クラウド・LocalSend・SMB・自己送信へ分散
  ([知恵袋](https://detail.chiebukuro.yahoo.co.jp/qa/question_detail/q11289085571)、
  [LocalSend 解説](https://curio13.com/blog/localsend/))

## 主要仮説

- **H1(最重要)**: 既存KVMの不満層は、信頼性の高い代替に有償でも乗り換える
- **H2**: Knit の接続は実測で既存より安定している
- **H3**: $10〜30帯の買い切りで一人開発の継続が成立する
- **H4**: 日本語IMEは日本市場限定の差別化。世界向けの第一訴求は「壊れない接続」

## 椫証1: 実在3人へのヒアリング(最優先・今週)

**目的**: H1 の実在顧客での確認。Web の声は「匿名の不満」であり「名前のある顧客」ではない。

**接触先(顧客のいる場所・特定済み)**:Barrier/Input Leap の disconnect/sleep 系 Issue 報告者、
r/MacOS・r/SynergyApp・r/logitech の近い投稿者、周囲の Mac+Windows 併用の開発者・クリエイター。

**接触メッセージ(下書き)**:
> Knit という Mac+Windows の入力・データ共有ソフトを作っています。
> あなたの書いた「(該当 Issue/投稿の内容)」と同じ問題に取り組んでいて、
> 壊れ方の実態を 30 分だけ聞かせてほしい。金銭の要求はありません。
> (開発者の名前+連絡先)

**質問票(4問)**:
1. 今のKVMソフトで壊れる場面を具体的に(スリープ復帰?再起動後?何をしたら直る?)
2. 壊れたときの対処に 1 週間でどれくらいの時間を費やすか
3. 壊れない代替に月いくらまで払うか(0円なら理由も)
4. PC間でファイル・画像をどれほど頻繁に受け渡すか
5. 次の候補のうち、払ってでも欲しいものはどれか:
   接続診断(繋がらない原因を教える)・安定性スコア(接続率の実測表示)・
   App Handoff(VS Code/ターミナルの作業位置ごと移す)・眠っているPCを起こす(Wake)

**判断基準**:
- 前進: 3人中 2 人が①の痛点を具体的な場面で語り、③に 0 円でない金額を口にする
- 撤退: 全員が「Barrierで十分・直す手間は許容」または③全員 0 円 →
  **無償 OSS 公開路線へ転換し、有償事業の検討を閉じる**

## 椫証2: 実機比較の実測

**計測基盤は 2026-10-02 に完成**。両 OS で `[conn-metric]` を壁時計で記録し、
`scripts/stability-report.sh`(過去ログ集計)とプロセス内カウンタ(診断の
「安定性」行)で実測できる。`knit-win.exe --probe-diag` で Windows 側も
ssh から診断できるため、同じ 2 台での Knit/Deskflow 入れ替え実測が実行可能。

**目的**: H2 の実測。「壊れない接続」の訴求は実測が取れてからにする。

**方法**:
1. 同じ 2 台で Knit と Barrier を順に入れ替え、同じシナリオを走らせる
2. シナリオ: 「スリープ→復帰→カーソル移動」を 10 回、「再起動→カーソル移動」を 3 回
3. 指標: 再接続成功率、再接続までの時間、ユーザーが手で何かする必要があったか
4. Knit 側の計測: `[conn-metric]` ログ(接続・断を unix ms で記録)を
   `scripts/stability-report.sh` で集計

**判定**:
- 通過: Knit の再接続成功率と断の長さが Barrier を明確に上回る
- 未通過: 差がなければ「壊れない接続」は訴求軸から外す(IME特化・日本市場戦略へ後退)

## 椫証3: コミュニティ反応(検証1・2 の後)

- 既存KVMの不満スレッドや Issue で「同じ問題を解決する別実装を作っています」と誠実に紹介
- 判断: 「試したい」という反応が複数得られるか
- 注意: 各コミュニティのルールを守る。宣伝と受け取られない形(開発者としての技術交流)で

## 追加機能の候補(検証対象・2026-10-01 追記)

既存の 3 調査([desk-moments](../research/2026-09-28-desk-moments.md)、
[future-ideas](../research/2026-09-28-future-ideas.md)、[本質レビュー](../research/2026-09-28-essence-review.md))は
「境界を消す」理想の方向で絞り込み済み。ここは補完として、**乗り換え層の痛点(既存KVMの不満)から
来る信頼性系**の候補を、判断軸つきで置く。

**判断軸**:① Web の声に根ざすか ② 乗り換えを成立させるか(H1) ③ 境界を意識させないか
(本質レビューの基準) ④ 一人開発で作れる規模 ⑤ Pro 線引きに載るか

| 候補 | 根拠(声) | 規模 | Pro/Free |
|---|---|---|---|
| **接続診断**(繋がらない原因を順に確かめて教える) | 既存KVMの「動かない・原因不明」で修復記事が乱立。サポート負荷の軽減(H3)にも直結 | 中 | Free(信頼の土台) |
| **ネットワーク品質の見える化**(遅延の原因を警告) | 「speedtest が走っている間だけ速い」(Barrier Issue)= WiFi 品質で悩む人が実在。RTT 監視は既存 | 小 | Free |
| **安定性スコア**(今週の接続率・断を UI 表示) | `[conn-metric]` 基盤は完成済み。「壊れない」の証拠を本人が見られる=信頼の構築 | 小〜中(集計の GUI 化) | **Pro 候補** |
| **App Handoff 拡大**(VS Code のリポジトリ・ターミナルの cwd をごと移す) | vision.md §12 の正統な継続。Continue Here の発展。無償フォークが構想すら持たない差別化 | 大(アプリ別に段階的) | **Pro 候補** |
| **眠っているPCを起こす**(Wake) | Barrier の feature request に実在。蓋を閉じたノートPCへの痛点 | 中(許可モデルの設計が要る) | Free/オプション |

**順位の推奨**: 接続診断+品質見える化(Free の土台)→ 安定性スコア → App Handoff(VS Code から)→ Wake。
ただし**着手は検証1の結果を待つ**。候補は「Web の声から推測した需要」であり、実在顧客が
どれに飛びつくかは未知。質問票の 5 問目で候補の反応を聞き、反応が最も強いものから作る。

**取り込まないもの**:DDC/CI モニター制御([desk-continuity で不採用判断済み](../research/2026-09-30-desk-continuity.md))、
画面共有・リモートデスクトップ方向([製品定義で対象外](../product-definition.md))、Cloud アカウント(§21 の MVP 外)。

## 前進条件(実行判断モードへ)

- 椫証1の前進条件達成+検証2通過+検証3で複数の「試したい」反応

## 未決論点

- **ライセンス構造**: MIT 公開済みコードと有償化の整合(コア無償+Pro非公開のデュアル構造、
  または非公開化)。公開済みコードのフォークが先に有償機能を実装し得るため、早期決定が必要
- 価格の具体値($29買い切り仮説)は検証1の③の回答範囲を確認してから確定する
- 一人開発の持続性(サポート・署名・更新の負荷)

## 関係文書

- [vision.md](../vision.md) — 製品ビジョン
- [2026-09-30-global-commercialization-roadmap.md](2026-09-30-global-commercialization-roadmap.md) — 商業化ロードマップ
