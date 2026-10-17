# マシンをまたいだドラッグ&ドロップ: 競合製品の UX 調査

作成: 2026-10-18。担当: 競合 UX 調査(team task-1)。
対象: Knit(Mac ⇄ Windows で入力・クリップボード・ファイル・音声を共有する Rust 製アプリ)の
「ファイルを掴んだまま画面境界を越えて渡す」機能の UX 改善。

この文書の読み方:

- **【事実】** = 出典 URL を確認できた記述。出典は各項末尾に示す。
- **【タイトルのみ】** = 検索結果でページのタイトルまでは確認したが、本文を取得できなかったもの。**内容の断定はしていない。**
- **【推測】** = 出典がなく、筆者の一般知識・設計上の判断。**根拠として扱わないこと。**
- **【社内】** = このリポジトリ内の既存ドキュメントに基づく Knit 自身の現状。

参照した Knit 側の資料:
[ドラッグ&ドロップ アップグレード設計](2026-09-28-drag-upgrade-design.md)、
[WindowsからMacへのファイルドラッグ](2026-09-27-windows-to-mac-drag.md)。

---

## 1. 製品ごとの要約

### 1.1 Synergy / Deskflow / Input Leap / Barrier

- **【事実】** Deskflow には「Windows と Linux の間でファイルをドラッグ&ドロップしたい」という
  ディスカッションが存在する([deskflow/deskflow Discussion #8082](https://github.com/deskflow/deskflow/discussions/8082))。
  少なくともコミュニティ側で要望が出ており、当たり前に動く機能として定着しているわけではないことがうかがえる。
- **【事実】** Deskflow の公式プロトコルドキュメントに "File Transfer Messages" の節がある
  ([deskflow protocol: Data Messages](https://deskflow.github.io/deskflow/group__protocol__files.html))。
  ファイル転送をプロトコルとして定義している製品であることは確か。
- **【タイトルのみ】** Input Leap は `input-leapc` / `input-leaps` の man ページが公開されている
  ([input-leapc(1)](https://man.archlinux.org/man/input-leapc.1.en)、[input-leaps(1)](https://man.archlinux.org/man/extra/input-leap/input-leaps.1.en))。
- **【タイトルのみ】** Barrier には `fix #163` というコミットがある
  ([barrier@b09a398](https://github.com/debauchee/barrier/commit/b09a3985d0f45b6a2a602dcea2e6234bd95e9592))。
- **【事実】** Synergy の公式 FAQ は `help.symless.com` から `support.symless.com` へリダイレクトされ、
  自動取得できなかった(出典: [Synergy Help Center FAQ](https://help.symless.com/hc/en-us/articles/32643108754449-FAQ))。
  → **Synergy の D&D 対応可否は一次情報で確認できていない。**

**【推測】** この系統(KVM 型)の弱点は「ポインタとキーの転送が本業で、ファイル転送は後付け」という点。
ドラッグ中に相手側で何が表示されるか、といった演出は製品ごとに差が大きく、統一された作法がない。

### 1.2 Microsoft Mouse Without Borders (MWB)

- **【事実】** Microsoft Q&A に「マウス・キーボード・テキストのクリップボードは動くが、ファイルの
  ドラッグ&ドロップだけがどんな種類・大きさでもできない」という質問がある。同質問には
  「Same question (9)」が付いており、複数ユーザーが同じ症状を報告している。
  質問者は回答への返信で、公式の説明が**「1ファイル・100MB 未満」という制限を述べるだけだった**と書いている。
  ([Microsoft Q&A: Mouse without border - Drag/Drop Files not working](https://learn.microsoft.com/en-in/answers/questions/4161756/mouse-without-border-drag-drop-files-not-working))
- **【事実】** このスレッドの回答は「保留中の更新を当てる」「マウスドライバを更新する」
  「MWB を入れ直す(Windows 11 を先に、Windows 10 を後に)」という一般論で、根本原因の説明になっていない。

**【推測】** MWB は「クリップボード共有でコピーしたファイルを相手に貼る」経路も持つが、
本調査ではその一次情報を取得できなかった。制限が UI 上に事前提示されず、失敗してから初めて
ドキュメントを読む羽目になるのが不満の核だと読める。

### 1.3 ShareMouse

- **【事実】** 公式ページに "Drag & Drop Files between Mac & Windows" があり、
  「ネットワーク共有の設定もクロスプラットフォームのプロトコル設定も不要」と説明している。
  クリップボード共有では「書式付きテキストやファイルをコピーし、**キー1回**で他のコンピュータに貼り付けられる」。
  ([ShareMouse 公式](https://www.sharemouse.com/))
- **【事実】** 転送の開始条件が明記されている:
  「ShareMouse は**あるコンピュータから別のコンピュータへのマウスの遷移を認識し**、
  選択されたファイルのローカル転送を既存のネットワーク接続で開始する」。
  ローカルネットワークから出ないこと、AES 暗号化できることも書かれている。
  ([Transfer files between computers](https://www.sharemouse.com/features/transfer-files-between-computers/))
- **【事実】** 同ページにはファイル交換中の画面とみられるスクリーンショットが掲載されている
  (`sm_fileexchange.png`)。ただし画像の中身は本調査では確認していない。
- **【推測】** 「境界を越えた瞬間に転送を開始する」設計は、Knit の現行実装
  (全データ受信後に越える)の真逆であり、待たされ感の差は大きいと考えられる。

### 1.4 Logitech Flow (Logitech Options+)

- **【事実】** Logitech 公式 FAQ に Flow の設定・トラブルシュート記事がある
  ([What is Logitech Flow and how do I set it up and troubleshoot it?](https://support.logi.com/hc/en-us/articles/1500005634742-What-is-Logitech-Flow-and-how-do-I-set-it-up-and-troubleshoot-it))。
  本文は取得できなかった(ページが要約のみ返った)。
- **【事実】** 別の公式 FAQ には「ルータやファイアウォールの後ろにあるコンピュータのために、
  **Flow はピア探索を補助するのに Logitech のクラウドサービスを使う**」と書かれている
  ([FAQ - M590 Multidevice Silent Mouse](https://support.logi.com/hc/en-ch/articles/360025336673-FAQ-M590-Multidevice-Silent-Mouse))。
- **【事実】** 紹介記事によれば Flow の受け渡しは「アプリやブラウザの対応画像コンテンツをコピーし、
  Flow でコンピュータを切り替え、対応する貼り付け先に貼る」というクリップボード主体の操作
  ([How to Use Logitech Flow Across Multiple PCs](https://www.techbloat.com/how-to-use-logitech-flow-across-multiple-pcs.html))。

**【推測】** Flow は「ドラッグ&ドロップでファイル」ではなく「コピー&ペーストで受け渡す」路線に寄せている。
マウスの境界越えとファイル移動を同じ操作に束ねない判断は、失敗時の分かりにくさを避ける利点がある一方、
「掴んだまま渡す」体験の期待には応えていない。

### 1.5 Apple Universal Control

- **【タイトルのみ】** Apple Developer Forums に「Universal Control はデバイス間でファイルを
  ドラッグ&ドロップできない」というスレッドがある
  ([Universal Control can't drag and drop files across devices.](https://developer.apple.com/forums/thread/700925))。
  本文は bot 検証で取得できなかった(取得時に `verify-human` ページへ誘導された)。
- **【事実】** Apple 公式サポートに Universal Control(日本語名「ユニバーサルコントロール」)の
  ページが存在する([Apple サポート 102459](https://support.apple.com/zh-tw/102459))。
  日本語・英語版は取得できなかった(コンテンツタイプ不明で失敗)。
- **【事実】** 利用者側の記事では、Universal Control が作業の仕方を変えたという肯定的な体験談がある
  ([Pocket-lint: How Universal Control with the iPad changed the way I work](https://www.pocket-lint.com/universal-control-changed-the-way-i-work/))。
- **【タイトルのみ】** 設定ガイド記事も複数存在する([Setapp: Universal Control guide](https://setapp.com/how-to/universal-control-on-macos))。

**【推測】** 「カーソルが2台をシームレスに往復する」体験そのものは高く評価されている一方、
**その延長線上にファイル移動があるとは限らない**。この点は Knit の期待値設計に直結する:
境界越えの気持ちよさとファイル転送の確実性は別物として設計し、UI でも分けて見せるべき。

### 1.6 KDE Connect / GSConnect

- **【タイトルのみ】** KDE のバグトラッカーに "Transfer by folder"(**フォルダ単位で転送したい**)という
  項目がある([KDE Bug 474052 – Transfer by folder](https://bugs.kde.org/show_bug.cgi?id=474052))。
  本文は未取得のため、要望の詳細・状態は断定しない。
- **【タイトルのみ】** iOS 版の評価ページが存在する
  ([KDE Connect - App Store](https://apps.apple.com/us/app/kde-connect/id1580245991))。
- **【タイトルのみ】** 利用者による肯定的な言及([GrapheneOS Discuss: KDE Connect: GOS Best Friend](https://discuss.grapheneos.org/d/23436-kde-connect-gos-best-friend))。

**【推測】** KDE Connect は「明示的な送信操作(コンテキストメニュー/トレイ)+ 受信側の承認」型で、
掴んで越える操作は持たない。**「掴んだまま渡す」と「送信ボタンで送る」は別物**であり、
前者を名乗る Knit が後者の作法(承認ダイアログ)をそのまま持ち込むと体験が壊れる、という示唆。

### 1.7 LocalSend

- **【事実】** 公式 README より: クロスプラットフォーム・オープンソース。REST API + HTTPS で
  ローカルネットワーク内で完結し、インターネット接続も第三者サーバも不要。既定ポート 53317。
  ([localsend README](https://github.com/localsend/localsend/blob/main/README.md))
- **【事実】** ディレクトリは**再帰的に収集され、選択したルート名と入れ子のパスは受信側で保持される**。
  ただし「**空のディレクトリは送られない**(LocalSend はディレクトリ項目ではなくファイル項目を転送するため)」。
  同 README。
- **【事実】** トラブルシューティング表に「デバイスが見えない」(AP アイソレーション、Windows の
  ネットワークプロファイル、macOS のローカルネットワーク許可、VPN)と
  「**速度が遅すぎる → 5GHz を使う・両方で暗号化を無効にする**」が挙げられている。同 README。
- **【事実】** README に「**自動更新がない**」ため、アプリストアかパッケージマネージャからの導入を勧める記述がある。同 README。
- **【事実】** CLI は `localsend-cli send report.pdf photo.jpg ./project-backup` のように
  ファイル・ディレクトリを混在で渡せる。エイリアスは一意に相手を特定できる必要がある。同 README。
- **【事実】** 名前衝突の扱いを巡る要望が open で、**13 件の 👍 が付いている**:
  「現状は同名ファイルがあると**新しい名前とサフィックスを付けて保存**される。スキップ/上書きを選べる設定がほしい」
  ([localsend Issue #2478: Skip or override files existing at destination](https://github.com/localsend/localsend/issues/2478))。

### 1.8 PairDrop / Snapdrop

- **【タイトルのみ】** 転送がしばしば中断するという issue が存在する
  ([PairDrop Issue #10: File transfer often interrupted](https://github.com/schlagmichdoch/PairDrop/issues/10))。本文未取得。
- **【タイトルのみ】** Google Play に Snapdrop & PairDrop の配布ページがある
  ([Snapdrop & PairDrop - Google Play](https://play.google.com/store/apps/details?id=com.fmsys.snapdrop))。

**【推測】** ブラウザ完結型は導入の手軽さが利点だが、転送の継続性・再開・大容量の扱いが弱く、
「掴んで渡す」用途の代替にはならない。Knit が学ぶべきは「相手を一意に特定する手軽さ」の部分。

### 1.9 Windows 近距離共有 (Nearby Sharing)

- **【タイトルのみ】** 「Windows 11 で近距離共有のファイル転送が遅い」という解説記事がある
  ([TheWindowsClub: Nearby Sharing file transfer is slow in Windows 11](https://www.thewindowsclub.com/nearby-sharing-file-transfer-is-slow-in-windows))。
- **【タイトルのみ】** 近距離共有の速度に関する利用者スレッドもある
  ([Google Files ヘルプスレッド: Nearby share is slow](https://support.google.com/files/thread/238581894/nearby-share-is-slow?hl=en))。

**【推測】** 共有シートから送る型で、宛先は「人/デバイス」であって「場所」ではない。
**ドロップ先を選べない**(相手の保存先は固定)ことが、掴んで置く操作との最大の違い。

### 1.10 Warpinator

- **【事実】** **無言の上書き**が問題になっている(open、2022 年起票、👍1):
  モバイルクライアントから同名ファイルを送ると PC 上の古いファイルが上書きされる。
  承認ポップアップは出るが「重複や上書きについて何も言わない」ため、
  **古いファイルは永久に失われる**。要望は「該当する場合に転送通知へ『ファイルが上書きされる可能性がある』と表示する」こと。
  ([Warpinator Issue #135](https://github.com/linuxmint/warpinator/issues/135))
- **【事実】** **フォルダ転送が既存ファイルを消す**事故が報告されている(open、👍2):
  Android から "Camera" フォルダを送ると、PC 側の**同名ではない既存ファイルまで消える**。
  報告者の期待は「新しいファイルを対象フォルダへコピーし、既存ファイルは削除しない」こと。
  ([Warpinator Issue #205](https://github.com/linuxmint/warpinator/issues/205))
- **【タイトルのみ】** ネットワーク速度が出ないという issue もある
  ([Warpinator Issue #224: Unable to reach network speeds (not even close)](https://github.com/linuxmint/warpinator/issues/224))。

---

## 2. ユーザーが喜んでいる点(出典付き)

| # | 喜ばれている点 | 出典 |
|---|---|---|
| 1 | ネットワーク共有やプロトコル設定なしで、Mac と Windows の間でそのままファイルを渡せる | [ShareMouse 公式](https://www.sharemouse.com/)、[Transfer files between computers](https://www.sharemouse.com/features/transfer-files-between-computers/) |
| 2 | ファイルがローカルネットワークから出ない。必要なら AES 暗号化できる | [ShareMouse: Transfer files between computers](https://www.sharemouse.com/features/transfer-files-between-computers/) |
| 3 | クリップボード共有が「キー1回」で完結する(テキストだけでなくファイルも) | [ShareMouse 公式](https://www.sharemouse.com/) |
| 4 | **マウスの遷移を認識して転送を開始する**ため、操作が「掴んで越える」だけで完結する | [ShareMouse: Transfer files between computers](https://www.sharemouse.com/features/transfer-files-between-computers/) |
| 5 | カーソルが2台の間をシームレスに往復する体験そのものが作業の仕方を変える | [Pocket-lint](https://www.pocket-lint.com/universal-control-changed-the-way-i-work/) |
| 6 | サーバもインターネット接続も不要で、ローカルネットワーク内で完結する | [localsend README](https://github.com/localsend/localsend/blob/main/README.md) |
| 7 | **フォルダを再帰的に送れて、階層(ルート名と入れ子パス)が受信側で保たれる** | [localsend README](https://github.com/localsend/localsend/blob/main/README.md) |
| 8 | CLI からファイルとディレクトリを混在で一括送信できる | [localsend README](https://github.com/localsend/localsend/blob/main/README.md) |
| 9 | 端末間の接続が手軽(ブラウザだけで完結する系統) | [Snapdrop & PairDrop - Google Play](https://play.google.com/store/apps/details?id=com.fmsys.snapdrop)、[GrapheneOS Discuss](https://discuss.grapheneos.org/d/23436-kde-connect-gos-best-friend) |

---

## 3. ユーザーが不満に思っている点(出典付き)

| # | 不満 | 出典 |
|---|---|---|
| 1 | **同名ファイルが無言で上書きされ、古いファイルが永久に失われる**。承認ポップアップにも重複/上書きの記載がない | [Warpinator Issue #135](https://github.com/linuxmint/warpinator/issues/135) |
| 2 | **フォルダを送ると、同名ですらない既存ファイルまで消える**。「新しいファイルを足すだけで、既存は消さないでほしい」 | [Warpinator Issue #205](https://github.com/linuxmint/warpinator/issues/205) |
| 3 | 同名ファイルがあると**黙って別名保存**される。スキップ/上書きを選べない(要望に 👍13) | [localsend Issue #2478](https://github.com/localsend/localsend/issues/2478) |
| 4 | ファイルのドラッグ&ドロップが**何をしても動かない**(クリップボードは動く)。同じ質問が9人 | [Microsoft Q&A](https://learn.microsoft.com/en-in/answers/questions/4161756/mouse-without-border-drag-drop-files-not-working) |
| 5 | 公式の説明が「**1ファイル・100MB 未満**」という制限を述べるだけで、原因も回避策も示さない | 同上 |
| 6 | 転送速度がまったく出ない(回線性能に届かない) | [Warpinator Issue #224](https://github.com/linuxmint/warpinator/issues/224) |
| 7 | 転送がしばしば中断する | [PairDrop Issue #10](https://github.com/schlagmichdoch/PairDrop/issues/10) |
| 8 | 近距離共有のファイル転送が遅い | [TheWindowsClub](https://www.thewindowsclub.com/nearby-sharing-file-transfer-is-slow-in-windows)、[Google ヘルプスレッド](https://support.google.com/files/thread/238581894/nearby-share-is-slow?hl=en) |
| 9 | 速度を出すために「5GHz にせよ」「**暗号化を無効にしろ**」と言われる | [localsend README](https://github.com/localsend/localsend/blob/main/README.md) |
| 10 | ルータ/ファイアウォール越しの相手を見つけるのに**ベンダのクラウド**が使われる | [Logi FAQ (M590)](https://support.logi.com/hc/en-ch/articles/360025336673-FAQ-M590-Multidevice-Silent-Mouse) |
| 11 | フォルダ単位で送れない(要望がバグトラッカーに上がっている) | [KDE Bug 474052 – Transfer by folder](https://bugs.kde.org/show_bug.cgi?id=474052) |
| 12 | **空のフォルダが送られない**(ファイル項目しか転送しないため) | [localsend README](https://github.com/localsend/localsend/blob/main/README.md) |
| 13 | 自動更新がない | 同上 |
| 14 | 「Universal Control はデバイス間でファイルをドラッグ&ドロップできない」という開発者フォーラムのスレッド | [Apple Developer Forums #700925](https://developer.apple.com/forums/thread/700925)(タイトルのみ確認) |
| 15 | 「Windows と Linux の間でファイルを D&D したい」が未解決の要望として残っている | [deskflow Discussion #8082](https://github.com/deskflow/deskflow/discussions/8082)(タイトルのみ確認) |

---

## 4. Knit が真似できる具体策

前提となる Knit の現状(**【社内】**、出典は [drag-upgrade-design](2026-09-28-drag-upgrade-design.md)):

- 全データの受信が終わってから境界を越えるため、**大容量ファイルでは境界で待たされる**。
- 形式はファイルのみ(`CF_HDROP` / `NSURL`)。**フォルダ・仮想ファイル・画像/URL/テキストのドラッグは対象外**。
- 1件 10GiB・合計 10GiB の上限。最終名へ直接書き込み。**内容ハッシュ・再開・送信側からの取消なし**。
- 名前衝突は `files::create_unique` で解決(別名保存)。
- Mac→Win は転送結果を Mac に返さない。同時実行は 1 本(`FILE_TX_BUSY`)。

### 優先度: 高

**H1. 転送は「境界を越えた瞬間」に開始し、ドロップで確定する(ShareMouse 方式 + Knit の遅延設計)**

- 何を: 境界越えを検知した時点で bulk 転送を開始する。相手側は一時ファイルに書き、
  ドロップ確定後に最終名へ rename、Esc・取消・切断なら一時ファイルを削除する。
- どうする: 既存設計の `drag.lazy` / `bulk.cancel` capability を実装し、
  「境界で待たせない」を段階1の完了条件にする。相手が旧版(`drag.lazy` 非対応)なら現行方式にフォールバック。
- 根拠: 【事実】ShareMouse は「マウスの遷移を認識して転送を開始する」と明記。
  【社内】Knit は逆に「全データ受信後に越える」ため境界で待つ。

**H2. 無言の上書きは絶対にしない。既定は自動リネーム + 「衝突した事実」の明示**

- 何を: 同名ファイルがあるとき、(a) 既定はサフィックス付きの別名保存、(b) 設定で
  「常に上書き / 常にスキップ / 常に別名」を選べる、(c) 衝突が起きたら通知に1行出す
  (例:「1件は同名のため `report (1).pdf` として保存しました」)。
- どうする: `create_unique` の戻り値に「衝突したか」を含め、転送結果サマリへ載せる。
  上書きを選べるようにする場合も、**削除を伴う操作の前に必ず1回確認**する。
- 根拠: 【事実】Warpinator #135(無言上書きで消失)と LocalSend #2478(黙って別名保存、👍13)。
  **どちらの極端も不満の元**であり、Knit の現行 `create_unique` は後者の状態。

**H3. フォルダは「置換」ではなく「マージ」で受ける**

- 何を: 同名フォルダが既にある場合、**消さずに中身をマージ**する。衝突したファイルにだけ H2 の規則を適用する。
- どうする: 受信側の書き込みを「ディレクトリ作成 + ファイル単位の衝突判定」に統一し、
  **既存ディレクトリを丸ごと差し替える経路を作らない**。
- 根拠: 【事実】Warpinator #205 では同名でない既存ファイルまで消えた。
  【社内】Knit は現状フォルダ非対応なので、**対応する最初の版でこの事故を埋め込まない**ことが重要。

**H4. 送信側にも結果を返す(進捗と完了を両側で見せる)**

- 何を: ドロップ後も送信側に「相手が受信中 → 完了 / 失敗」を表示する。転送中の中断手段を出す。
- どうする: 【社内】現状 Mac→Win は結果を返さないので、`Done`/`Cancel` を双方向に揃える。
  同時実行は 1 本のままにする(`FILE_TX_BUSY` の挙動は維持)。
- 根拠: 【事実】Warpinator #135 の「承認ポップアップが何も言わない」、LocalSend の速度トラブルシュート表。
  **状態が見えないことが最大の不満要因**。

**H5. 失敗時は「部分ファイルを公開しない」+ 通知から再試行**

- 何を: 失敗した転送は最終名で残さない(現行方針を維持)。通知に理由(切断 / 容量上限 / 相手が拒否)と
  「再試行」「Finder/Explorer で表示」を出す。
- どうする: 失敗理由を列挙型で持ち、UI 文言をそれに紐づける。上限超過は**越える前に**理由を出す。
- 根拠: 【事実】MWB の「1ファイル100MB」制限が事前に提示されず失敗してから分かる構造。

### 優先度: 中

**M1. 越える前に「渡せない理由」を知らせる**
フォルダ・仮想ファイル・上限超過は、境界に到達した時点で通知して越えさせない
(【社内】現行の段階0の期待挙動と一致)。越えてから失敗させない。

**M2. 進捗は「速度・残り時間・件数」を、確からしいときだけ出す**
残り時間が計算できない場面では出さない。総量が不明なときは件数と現在のファイル名だけを出す。
【推測】この出し分けの良否はどの製品の出典からも確認できていないため、Knit の判断とする。

**M3. 「送信完了」と「相手の OS がドロップを受理」を区別して表示する**
【社内】既存設計の目標5と同じ。成功を装わない。
根拠: 【事実】MWB の事例では「動いているつもりで動いていない」状態が最も混乱を生んでいる。

**M4. フォルダの階層を保ち、空フォルダも送る**
【事実】LocalSend はパスを保つが**空ディレクトリは送らない**。これは隙間であり、Knit は埋められる。

**M5. 取消の一貫性(Esc / 元 PC へ戻る / 切断のどれでも同じ結果)**
相手側の一時ファイルを片付け、クリップボードを書き換えず、部分ファイルを公開しない。
【社内】既存設計の目標3を、H1 の遅延転送と同時に成立させる。

### 優先度: 低

**L1. クリップボード経由のファイル受け渡しを副経路として持つ**
【事実】ShareMouse は「キー1回で貼り付け」、Logitech Flow はクリップボード主体。
掴めない場面(メニュー操作・ターミナル)の逃げ道になる。

**L2. 通知から保存先を開く / よく使う保存先を覚える**
【推測】どの製品も一次情報では確認できていないが、失敗時の回復動線として費用対効果が高い。

**L3. 演出は控えめに**
【推測】ドラッグ中のアニメーションや音の良否は出典で確認できていない。**確認できない以上、模倣しない。**

### 避けるべきこと(アンチパターン)

| 避けること | 理由(出典) |
|---|---|
| 無言の上書き | [Warpinator #135](https://github.com/linuxmint/warpinator/issues/135) |
| フォルダの置換(既存を消す) | [Warpinator #205](https://github.com/linuxmint/warpinator/issues/205) |
| 衝突を黙って別名にして済ませる | [localsend #2478](https://github.com/localsend/localsend/issues/2478)(👍13) |
| 発見・接続にベンダのクラウドを必須にする | [Logi FAQ](https://support.logi.com/hc/en-ch/articles/360025336673-FAQ-M590-Multidevice-Silent-Mouse) |
| 速度のために暗号化を切らせる | [localsend README](https://github.com/localsend/localsend/blob/main/README.md) |
| 制限(件数・容量)を失敗してから知らせる | [Microsoft Q&A](https://learn.microsoft.com/en-in/answers/questions/4161756/mouse-without-border-drag-drop-files-not-working) |
| 境界越えのたびに全データ完了を待たせる | 【社内】[drag-upgrade-design](2026-09-28-drag-upgrade-design.md) の既知課題 |

---

## 5. 出典の信頼度と、確認できなかったこと

### 5.1 情報源の性質

| 種別 | 例 | 扱い |
|---|---|---|
| ベンダ公式ページ | ShareMouse、Logi FAQ、localsend README | 主張としては強いが、**自社製品の利点を述べる文章**である点は差し引いて読む |
| バグトラッカー / issue | Warpinator #135・#205・#224、localsend #2478、PairDrop #10、KDE Bug 474052 | 実際に困った人の一次の声。ただし**個別事例であり発生率は不明** |
| Q&A フォーラム | Microsoft Q&A | 質問者の記述と、独立アドバイザーの一般論。**公式見解ではない** |
| 紹介記事・体験談 | Pocket-lint、Setapp、TheWindowsClub | 二次情報。タイトルと要約のみ確認したものがある |
| 検索結果のタイトルのみ | Apple Developer Forums #700925、deskflow #8082、KDE Bug 474052 | **内容は未確認**。本文を読んだうえで確定させる必要がある |

### 5.2 調べきれなかった項目(正直な記録)

1. **Synergy / Deskflow / Input Leap / Barrier の D&D 実挙動**(観点1〜6 すべて)。
   Synergy 公式 FAQ は `support.symless.com` へリダイレクトされ取得失敗。Deskflow は
   ディスカッションのタイトルのみ。Input Leap・Barrier は man ページとコミットの存在しか確認できていない。
2. **Logitech Flow の公式 FAQ 本文**(取得時に見出しのみ返った)。ファイルの D&D 対応可否、
   進捗表示、衝突時の挙動は**未確認**。
3. **Apple Universal Control の公式仕様**。Apple サポートはコンテンツタイプ不明で失敗、
   Developer Forums は bot 検証で遮られた。ファイル D&D の可否は**タイトルレベルの情報のみ**。
4. **KDE Connect / GSConnect の転送 UI**(進捗・取消・保存先の決め方)。KDE Bug 474052 のタイトルのみ。
5. **PairDrop / Warpinator の画面の作り**(進捗バーの粒度、失敗時の文言、再試行の有無)。
   issue の本文は取れたが、UI 仕様の一次資料には到達していない。
6. **Windows 近距離共有の実挙動**(対応サイズ、保存先の指定可否)。解説記事のタイトルのみ。
7. **ShareMouse のファイル交換画面の中身**(スクリーンショット `sm_fileexchange.png` は未確認)。
   進捗・残り時間・中止手段は**未確認**。
8. **「ユーザーが絶賛している細部」の厚み**。称賛側は公式マーケティング文言に依存しており、
   Reddit / Hacker News の生の声を直接取得できていない(検索経由の参照のみ)。
9. **発生率・母数**。issue や Q&A は「困った人が書く場所」であり、
   不満の一覧は**その製品の利用者全体の分布を表していない**。

### 5.3 調査に使った時間と手段

- 開始: 2026-10-18 07:41 JST(タスク claim 時点)。終了時刻は計測していない。
- 所要: 約 25 分。ツール呼び出し 26 回(うち検索 5 回、ページ取得 12 回、うち取得失敗 4 回)。
- 失敗した取得: Synergy 公式 FAQ(リダイレクト)、Apple サポート(コンテンツタイプ不明)、
  Apple Developer Forums(bot 検証)、Logitech Flow FAQ(本文が返らず)。
- 方針: 巨大ページは取得を避け、GitHub は生の README と JSON API(`api.github.com`)だけを読んだ。

---

## 6. 次の一手(この調査の外側)

1. **H1 と H5 を [drag-upgrade-design](2026-09-28-drag-upgrade-design.md) の段階1〜2に紐づける**
   (遅延転送 + 双方向の完了通知)。設計書側の改訂は本タスクの範囲外。
2. **H2 / H3 は受信側の書き込み規則の変更**なので、`files::create_unique` と
   フォルダ受信の実装前に、衝突時の方針を1枚の仕様に固定する。
3. **§5.2 の未確認項目のうち、優先度が高いもの**(Synergy 系の D&D 実挙動、Flow のファイル対応可否)は、
   公式ドキュメントの該当節を直接開いて確認する追加調査が必要。
