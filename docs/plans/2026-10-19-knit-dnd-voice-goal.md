# Knit ドラッグ&ドロップ — ユーザーの声から作るゴール

作成: 2026-10-19 / 対象: Mac⇄Windows のファイル・データのドラッグ&ドロップ
根拠: 既存の競合UX調査(2026-10-18、10製品・称賛9・不満15・出典付き)+今回追加した一次証言12件(URL付き)
状態: ゴール定義。実装は既存の設計(dnd-design の I1〜I9 / W1〜W5)を土台にする。

## 0. ゴール(1文)

**机の境界を越える受け渡しを、同じ机の上で物を動かすのと同じ確信で終えられるようにする。離したら必ず結果が分かり、置きたい場所に置かれ、失敗しても続きからやり直せる。**

### 0.1 達成条件(反証可能)

| # | 条件 | 測定方法 |
|---|---|---|
| G1 | **無言ゼロ**。多様な20回のドラッグ(小/大/フォルダ/相手ビジー/切断)で「結果が分からない終了」が0回。成功は保存先、失敗は理由と次の一手が、離した側のカーソル近傍に出る | ループバック+実機の20回表。各回で結果表示を撮る |
| G2 | **置き場所が離した場所**。Windows 側の Explorer ウィンドウ・デスクトップ・フォルダへの10回のドロップで、10回とも離した位置に入る(「常にデスクトップ」の再発なし) | 実機10ドロップ。保存先をエクスプローラで確認 |
| G3 | **待ちの可視化**。1GB で「離してから最初の進捗」までが1秒未満。Win→Mac では「受け取り中 N%」がカーソル近傍に出る。両側の進捗は1秒以内で一致 | 1GB/5GB/10GB。時計とログ、画面を撮る |
| G4 | **中断と再開**。キャンセルと切断で部分ファイルが0(内容ハッシュで確認)。再試行は送信済み分を再利用し、最初から送り直さない | 切断試験(ケーブル/プロセスkill)+ハッシュ比較+再送バイト数のログ |
| G5 | **双方向・多形式**。Mac→Win と Win→Mac が対称の品質。フォルダ(512ファイル/10GiB)・空フォルダ・画像・URL・テキストのドラッグが通る | 方向×形式の表テスト(自動)+実機 |
| G6 | **衝突は人に選ばせる**。同名時は上書き/スキップ/リネームを事前に選べる。無言の上書き0・勝手な自動リネームのみ0 | 同名ファイルのドラッグ試験。選択肢と結果を記録 |
| G7 | **初見で伝わる**。説明なしの初見10名中9名が5分以内に最初のドラッグを成功させる | 観察試験(返答が要る。設計書の出荷判定と同じ方式) |

## 1. 集めた声(今回追加分・すべて一次証言)

| # | 声(引用) | 出典 |
|---|---|---|
| V1 | 「Drag and drop was removed and is no longer supported. Consider using LocalSend.」「The feature is extremely buggy though, as it was implemented badly. … I seem to be able to drag from a macOS computer (but not to one), and I can drag to a Windows computer (but I can't drag from it). … Also, **the drop target is always desktop** which is a bit annoying.」 | [deskflow Discussion #8082(nbolton・メンテナ)](https://github.com/deskflow/deskflow/discussions/8082) |
| V2 | 「It does not work well」「There are better ways to share files, between computers.」 | 同上(sithlord48) |
| V3 | 「**Drag and drop is broken on all platforms in one case or another.** Remove this broken feature」 | [deskflow #8459](https://github.com/deskflow/deskflow/issues/8459) |
| V4 | 「ERROR: failed to get desktop path, no drop target available, error=2」「drag-and-drop file transfer does not work」/ 回避策のコメント「my keyboard has turn on scroll lock accidentally and makes it not work」 | [deskflow #5971(Synergy 1.8.8)](https://github.com/deskflow/deskflow/issues/5971) |
| V5 | 「Fixed **drag-and-drop error messages** appearing in older versions after connecting to a newer installation.」 | [Synergy 3.6.0 リリースノート(2026-03)](https://symless.com/synergy/download/release-notes/synergy3-personal/v3.6.0) |
| V6 | 「fixes an **OSX crash** when the drag pasteboard holds a Finder bookmark」/ 実装は「DropHelper (writes dropped files to **~/Downloads**)」 | [deskflow #10218(2026-10 の再導入試作)](https://github.com/deskflow/deskflow/issues/10218) |
| V7 | 「I can't drag drop files to one PC from another, **whatever the type or the weight of it**」「Same question (9)」/ 回答は「limitation (**one file, under 100MB**)」を述べるだけ | [Microsoft Q&A(Mouse Without Borders)](https://learn.microsoft.com/en-in/answers/questions/4161756/mouse-without-border-drag-drop-files-not-working) |
| V8 | 「a folder of 312 MB … **took Half hour to send 40%** files」「THe speed drops to 100 kbs」「It should send this much size of data in less than 2-3 mins.」 | [LocalSend #2558](https://github.com/localsend/localsend/issues/2558) |
| V9 | 「transfer **slows to a crawl** gradually with large batches — Windows ONLY」/ 期待は「consistent 8-10 Mbps」 | [LocalSend #2533](https://github.com/localsend/localsend/issues/2533) |
| V10 | 「if file exists, the same file will be saved with **new name and suffix**」「better to skip or override existing files but **this option doesn't exist yet**」 | [LocalSend #2478](https://github.com/localsend/localsend/issues/2478) |
| V11 | 「I'm a huge fan of Universal Control … updated both devices … **can't seem to get Universal Control working**」(47コメント) | [BGR(2023)経由の Reddit 証言](https://www.bgr.com/tech/universal-control-breaks-after-latest-os-updates-and-apple-hasnt-addressed-it/) |
| V12 | 「Sharemouse or Synergy (the former having **working drag n drop and copy paste** between windows and osx」 | [Hacker News コメント](https://news.ycombinator.com/item?id=32765466) |

既存調査([dnd-ux-research](2026-10-18-knit-dnd-ux-research.md))の不満15件も同じゴールの根拠に使う。要点: Warpinator の無言上書き(#135)/既存ファイルまで消える(#205)、LocalSend の無言リネーム(#2478 👍13)、MWB の「何をしても動かない」が9人、速度が出ない(#224)、中断が頻発(PairDrop #10)、クラウド経由の発見(Logi FAQ)、フォルダ非対応(KDE Bug 474052)、**空フォルダが送れない**、Universal Control はファイル D&D 不可(Apple Developer Forums #700925)、Windows/Linux 間の D&D は未解決(deskflow #8082)。

**声から読めること。** 既存製品は「動く」ことより前の段階で止まっている。Deskflow 系は**削除で解決**した(V3)。残っている製品は片方向・デスクトップ固定(V1)、上限と無言の失敗(V7)、速度と衝突の扱い(V8〜V10)でつまずく。一方で、ShareMouse の D&D が「動く」こと自体が評価されている(V12)。**動けば差異になる。**

## 2. 声から導く理想の6原則

1. **無言で終わらない。** 結果が分からない終了は最悪(V3, V4, V7。既存 F1/F2/F10 も同旨)
2. **離した場所に置かれる。** 常にデスクトップ固定は不満(V1)。受け手の意図を尊重する
3. **待たされる前に知らせる。** 進捗・残量・見込みが、見ている画面に出る(V8, V9。F2/F4)
4. **失敗は失敗として、続きから。** 無言の上書き/消失/自動リネームをしない(V10, 既存不満1〜3, F7/F9)
5. **片方向・片OSの専用機能にしない。** Mac→Win だけ、片方向だけは不可(V1)。空フォルダも落とさない(既存不満12)
6. **衝突は人に選ばせる。** 上書き/スキップ/リネームを事前に(V10)

## 3. 声 → 達成条件 → 検証

| 声 | 条件 | 検証 |
|---|---|---|
| V1(片方向), 既存不満14/15 | G5 双方向・多形式 | 方向×形式の表テスト+実機 |
| V1(常にデスクトップ) | G2 置き場所 | 実機10ドロップ |
| V3, V4, V7, 既存不満4/5 | G1 無言ゼロ | 20回表+結果表示の撮影 |
| V8, V9, 既存不満6/7/9 | G3 待ちの可視化 | 1/5/10GB の時計・ログ |
| 既存不満1/2/3, V10 | G4/G6 失敗と衝突 | 切断試験+ハッシュ+同名試験 |
| V7(9人同じ質問) | G7 初見 | 観察試験10名 |

## 4. 現状とのギャップ(設計書 §11 の実装状況を根拠に)

**既にある(ゴールに近い土台)。** I1(掴み判定を値に)・I2(離す前の予報)・I3(カーソル近傍の理由)・I5(3状態の結果)・I6(回復動線)・I7〜I8b(遅延提供・目録)、W1(受理を先に。**既定は無効**)、W2(待ちの可視化)、W4(結果を送信側へ)、W5(通知から受信フォルダ)。実機E2Eは W1 が未検証。

**足りない。**

1. **I9 が未着手**(Mac が受け取る File Promise)。Win→Mac は全データ到着後にドラッグが始まるため、**1GB 以上で「離すまで待つ」が残る**。G3 に直結
2. **W1 が既定で無効+実機E2E未検証**。G2(置き場所)と待ちの消滅は、既定化の判断が要る
3. **非ファイルのドラッグ**: 画像・URL・テキスト・仮想ファイルは現状対象外(ファイル形式のみ)。G5
4. **中断と再開**: 原子的公開(一時ファイル→rename)とキャンセル時の削除は**実装済み**(受信経路の調査で確認)。足りないのは fsync(電源断の耐久)、ネストした一時ファイルの掃除、中断した非ドラッグバッチの空ディレクトリ、そして**レジューム(送信済みバイトの再利用)**。G4
5. **衝突の選択肢**: 受信は同名自動リネーム。上書き/スキップ/リネームの芯(N5a)は実装済みで、選択の入口は環境変数 `KNIT_COLLISION`。設定画面の選択肢は未実装(N5b)。G6
6. **初見試験**: G7 は未実施(設計書の出荷判定でも「未達」)
7. **入力の多様性**: フォルダ内の空ディレクトリ・シンボリックリンク・パーミッションの扱いが仕様化されていない

## 5. 増分計画(次に作るもの・1増分=1コミット)

| 増分 | 内容 | 検証 | 前提 |
|---|---|---|---|
| N1 | I9 の PoC(実機): Finder が書き出し完了を待つか、失敗時の残骸、寿命を4点で確認し、実装するかを決める | ランナップ §5.6 の4点。結果で設計を更新 | 実機2台 |
| N2 | W1 の既定化判断(実機E2Eが緑になってから)。置き場所の表(G2)を実機で10回 | 実機10ドロップ。既定化の可否を決定ログに残す | 実機2台 |
| N3 | 非ファイルペイロード(画像/URL/テキスト)のドラッグ。既存の遅延提供の型に載せる | 形式ごとの表テスト+実機 | N2 の後 |
| N4a | 耐久と掃除: fsync(実体+親ディレクトリ)、ネストした一時ファイルの再帰掃除、中断した非ドラッグバッチの空ディレクトリ回収 | ループバックの残骸検査+実機のkill試験 | — |
| N4b | レジューム: 送信済みバイトの再利用、明示中止と切断の区別(ABORT)、版ゲート(PeerFeatures.resume) | 2接続試験で再送量を計測+prefix照合 | 設計判断(版18) |
| N5a | 衝突ポリシーの芯: Rename(既定)/Overwrite/Skip、件数の報告、通知の文言、`KNIT_COLLISION` での選択 | 同名試験(置換・スキップ・退避) | — |
| N5b | 設定画面の選択(上書き/スキップ/リネーム)。既定は「確認」に近づける | 実機の設定操作 | N5a・UI の実機確認 |
| N6 | 初見10名の観察試験(ゴールの最終判定) | 10名中9名が5分以内 | N1〜N5 の後 |

## 6. やらないこと

1. **常にデスクトップへ落とす簡略化**(V1 が不満と明言)。置き場所を犠牲にしない
2. **暗号化を切って速度を出す**(既存不満9)。速度は経路(Wi-Fi 実効 30MB/s 実測)の範囲で正直に出す
3. **クラウド中継による発見**(既存不満10)。LAN/Tailscale のまま
4. **Android の D&D を今回のゴールに含める**(別経路。ファイル転送の受信のみ対象)
5. **見えない UI の既定化**(W3 と同じ理由。実機で見てから)

## 7. 検証計画

- 自動: ループバックで方向×形式×サイズの表テスト、切断時0残骸のハッシュ検査、再送量のログ検査。ベンチは既存の `scripts/bench.sh`
- 実機: [計測手順](2026-10-19-knit-dnd-goal-runbook.md)の表(G1 20回・G2 10ドロップ・G3 サイズ別・G4 切断・G5 形式・G6 同名)に記録する。20回表は結果の種類(成功/理由付き失敗/無言)を数える
- 人手: 初見10名(G7)。返答が要る試験として最後に置く

## 8. 弱点・未決(正直に)

1. **X(旧Twitter)の声は今回取れていない**(検索基盤が未ログイン)。Web/SNS の証言は偏りがある(不満を書く人が多い)
2. **実機2台が要る項目(N1/N2/N6)は、このマシンでは完了できない**。判断はランナップの結果待ち
3. **V6 #10218 は試作**(closed)。実装として採用されたかは未確認
4. **速度の目標値は未測定の提案**。Wi-Fi 実測(30MB/s)を前提にした「回線の範囲で」を守る
5. **G2 の「離した場所」は、Windows のアプリによっては受け取れない**(ドロップ非対応アプリ)。表記は「受け取れる場所に離したとき」に限定して数える

## 9. 出典(今回追加分)

V1/V2: https://github.com/deskflow/deskflow/discussions/8082 / V3: https://github.com/deskflow/deskflow/issues/8459 / V4: https://github.com/deskflow/deskflow/issues/5971 / V5: https://symless.com/synergy/download/release-notes/synergy3-personal/v3.6.0 / V6: https://github.com/deskflow/deskflow/issues/10218 / V7: https://learn.microsoft.com/en-in/answers/questions/4161756/mouse-without-border-drag-drop-files-not-working / V8: https://github.com/localsend/localsend/issues/2558 / V9: https://github.com/localsend/localsend/issues/2533 / V10: https://github.com/localsend/localsend/issues/2478 / V11: https://www.bgr.com/tech/universal-control-breaks-after-latest-os-updates-and-apple-hasnt-addressed-it/ / V12: https://news.ycombinator.com/item?id=32765466

既存: [dnd-ux-research](2026-10-18-knit-dnd-ux-research.md) の §2/§3(称賛9・不満15、各出典)、[dnd-friction](2026-10-18-knit-dnd-friction.md) の F1〜F12、[dnd-design](2026-10-18-knit-dnd-design.md) §11 実装状況、[ランナップ](2026-10-18-knit-dnd-verification-runbook.md) §5.6。
