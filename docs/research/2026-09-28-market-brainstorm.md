# 市場調査とブレインストーミング: 利用者は何を欲しがっているか

> **2026-09-28 追記:** ここの案は [本質を疑う](2026-09-28-essence-review.md) で取捨を判定した。採否はそちらに従う。

作成: 2026-09-28。公開情報(下記の出典)の調査と、その上での発想。
利用者への聞き取りはまだ行っていない。「欲しがっているもの」は、競合の限界・不具合報告・
回避策の存在から推し量った**仮説**であり、検証の方法を最後に置く。

---

## 1. 市場の地図

### PC 同士(ソフトウェア KVM)

| 製品 | 強み | 限界 |
|---|---|---|
| Deskflow(無料・OSS)/ Synergy(有料) | Win・Mac・Linux。暗号化が既定 | クリップボードが「時々しか効かない」報告が続く。OS 更新で権限が外れる。日本語入力の切替は 2008 年頃から未解決(回避策は AutoHotkey・パッチ版) |
| Input Leap / Barrier | 無料 | **Input Leap は 2026-07 に開発終了(アーカイブ)**。Barrier も終了 |
| ShareMouse(有料) | Mac⇄Win、ファイルのドラッグ | 2 OS のみ |
| Mouse Without Borders(PowerToys) | 無料、4 台 | Windows のみ |
| Logitech Flow | 端で越える体験 | Logitech の対応マウス・キーボードが必要。PC 2〜3 台のみ、タブレット不可 |
| Apple Universal Control | iPad まで含めた最高の体験 | Apple 製品・同じ iCloud のみ |

### PC → タブレット・スマホ

| 製品 | 方式 | 限界 |
|---|---|---|
| Samsung Multi Control | 端で越える | Galaxy Book と Galaxy 端末だけ(他社 PC は非公式の抜け道) |
| Xiaomi Interconnectivity | 端で越える・拡張画面 | 原則 Xiaomi の PC だけ |
| Lenovo / Motorola Cross Control | 端で越える | 自社端末の組み合わせ |
| Microsoft Phone Link | 画面を窓に映して操作 | 端で越えない(ミラー)。深い連携は Samsung 中心 |
| InputShare / InputShare2 | adb(開発者向けオプション必須) | PC 側は Windows のみ |
| Deskflow Android | ユーザー補助+専用キーボード | 専用キーボードへ強制的に切り替わる。スクロールとジェスチャーが弱い。Android 14 以上 |
| One Mouse | ユーザー補助 | PC 側は Windows。Android 11 以上 |
| scrcpy OTG | USB の AOA で本物の HID | **USB デバッグ不要**。ただし有線のみ・コマンド操作 |

## 2. 市場から読み取れること

1. **いちばん良い体験は、メーカーの壁の内側にしかない。** Universal Control・Multi Control・
   Xiaomi・Lenovo は、どれも「同じメーカーでそろえた人」だけのもの。Mac と Windows と
   Android を混ぜて使う人(= Knit の対象)には、試作段階のような道具しか残っていない
2. **「動くけれど信用できない」が最大の不満。** クリップボードが時々止まる、OS 更新で
   権限が外れる、版の違いで壊れる。機能の多さより、毎日確実に動くことが価値になる
3. **日本語入力は 18 年放置された不満。** 英数/かなと半角/全角の対応、JIS と US の配列違いは、
   利用者が AutoHotkey やパッチ版で自力回避している。ここを最初から解けば、日本で明確な居場所になる
4. **Android は「本物のポインタ」か「デバッグ不要」かの二択を迫られている。** ユーザー補助方式は
   キーボードの強制切替とスクロールの弱さが残り、adb 方式は一般の人に使えない
5. **乗り換え先を探す人がいる。** Input Leap の終了、Synergy のライセンス変更の混乱で、
   無料・OSS 系の利用者が移動している最中
6. **権限への不安。** この種のアプリは広い権限(ユーザー補助・常駐・通信)を求める。
   何のために何を使うかを見せられること自体が差別化になる

## 3. 利用者が欲しがっているもの(仮説)

| 欲求 | 根拠 | Knit の現状 |
|---|---|---|
| 1 組のマウス・キーボードで、手元の全部の機器を使いたい(メーカー・OS を問わず) | メーカー縛りの製品ばかり | Mac・Win・Android(adb) |
| 毎日、何もしなくても確実につながっていてほしい | クリップボード・権限・版違いの不具合報告 | 自動再接続あり。自己診断はない |
| コピーしたものが、どこでも貼れてほしい | 最多の不具合報告 | 画面を移る時に同期 |
| 日本語入力が PC ごとにずれないでほしい | 18 年分の回避策 | 英数/かな・IME 状態の同期あり |
| 設定の知識なしで始めたい | 開発者向けオプション・IP・ポートが壁 | 6 桁登録あり。Android は壁が残る |
| タブレットを「映す」のでなく「机の一員」にしたい | Phone Link はミラーのみ | adb で実現。アプリ未着手 |
| 何を共有しているか分かり、止められること | 権限への不安 | 部分的 |
| 全部の音を 1 つのイヤホンで聞きたい | 利用者本人の要望 | Win→Mac のみ |

## 4. 機能のブレインストーミング

評価: 価値(◎大 〇中 △小)/ 実現性(◎容易 〇要設計 △難・要検証)。発想を広く出したもので、採否は未決定。

### 信頼(毎日確実に)
- **健康診断パネル**: 入力・クリップボード・音声・権限・経路を 1 画面で ✓/✗ 表示し、直し方へ案内する(◎/◎)
- **OS 更新後の権限の自己修復案内**: 権限が外れたことを検知し、その設定画面へ直行(◎/〇)
- **両端の自動更新**: 版違いによる故障をなくす(◎/〇)
- **つながり方の自動選択**: 同じ LAN・ケーブル直結・Tailscale・USB を自動で選び、理由を表示(〇/〇)

### 入力
- **日本語の完全対応**: JIS/US/ISO 配列の自動判別、英数/かな・半角/全角・変換/無変換の対応表、
  IME Follow Cursor / 端末ごと / アプリごと(◎/〇)
- **越える意図の判定**: 速さ・向き・滞在時間で誤って越えるのを防ぐ(ビジョン§24)(〇/〇)
- **アプリごとのショートカット翻訳**: ターミナル・Office・ブラウザで ⌘ と Ctrl の扱いを変える(〇/〇)
- **ゲーム・全画面動画での自動ロック**(〇/◎。一部実装済み)

### データ
- **どこでも貼れるクリップボード**: テキスト・画像・ファイル、全端末共通の履歴(◎/〇)
- **Throw(投げる)**: ファイルやタブを画面端へはじくと隣の機器で開く(〇/〇)
- **Continue Here**: 見ていた URL・文書を隣で開く(〇/◎。一部実装済み)

### タブレット
- **有線ならデバッグ不要の本物の操作(AOA)**: USB でつなぐだけで、本物のポインタと全キー(◎/〇、下の 5 章)
- **アプリの標準モード**: ユーザー補助+専用キーボード。滑らかなスクロールと長押しの作り込み(◎/△)
- **タブレットのペン・タッチで PC を操作**: 板タブ代わり(〇/△)
- ~~タブレットを PC の追加画面に~~ → 対象外(各機器は自分の OS のまま使う。2026-09-28 利用者の決定)

### 音・画面
- **全端末の音を Mac のイヤホンへ**(◎/〇。指示文作成済み)
- **マイクの行き先の切替**: 会議を別 PC で受ける人向け(△/△)

### 作業場所
- **MY DESK の配置画面**: 端末と画面をドラッグで並べる(◎/〇)
- **場所ごとの構成**: 自宅・職場で配置と共有範囲を切り替え(〇/〇)

### 大胆な案
- **Knit ドングル**: USB の小さな機器がキーボード・マウスになりすます。ソフトを入れられない
  **会社支給の PC**、iPad、Android のどれにも挿すだけで使える。「会社 PC にアプリを入れられない」
  という大きな層に届く(◎/△、ハードウェア事業になる)
- **スマホをトラックパッド・キーボードに**: 逆向きの入力(△/〇)

## 5. Android 戦略への新しい示唆: 有線ならデバッグ不要で「本物」にできる可能性

scrcpy の OTG モードは、USB の **AOA(Android Open Accessory)2.0 の HID 機能**を使い、
**USB デバッグなしで** Android へ本物のキーボード・マウスとして振る舞う(scrcpy の公式文書)。
Knit の Mac 側がこれを実装すれば、次の 3 段構えになる。

| つなぎ方 | 必要なもの | 操作の質 | 対象 |
|---|---|---|---|
| **有線(AOA)** | USB ケーブルだけ | 本物のポインタ・全キー | 一般の人 |
| **無線(アプリ・標準モード)** | Knit アプリ+許可 2 つ | 疑似カーソル・合成タッチ | 一般の人 |
| 無線(adb・精密モード) | 開発者向けオプション | 本物のポインタ・全キー | 詳しい人 |

未確認の点: 接続時にタブレット側で許可を求める画面が出るか、Xiaomi(HyperOS)で AOA の
HID が有効か、macOS から USB 機器を扱う権限、充電しながら使えるか。**試作で最初に確かめる価値が高い**。

## 6. Knit の立ち位置(案)

**メーカーをそろえなくても、手元の全部の機器を「1 つの机」として使える唯一の道具。**
しかも毎日確実に動き、日本語入力が気持ちよく、何を共有しているかが見える。

- Apple・Samsung・Xiaomi の体験を、メーカーの壁の外で
- Deskflow 系の「動くけれど信用できない」を、健康診断と自己修復で
- 海外製が弱い日本語入力を、第一級の機能として

## 7. 検証の方法(仮説を確かめる)

1. **聞き取り 8〜10 人**: 複数の PC・タブレットを使い分けている人(開発者・デザイナー・
   事務職・学生を混ぜる)。今の道具・不満・諦めていること・払ってよい金額を聞く
2. **アンケート**: 使っている機器の組み合わせ、困りごとの順位、日本語入力の困りごと
3. **試してもらう**: 環境の違う 5〜10 人に最初の 5 分を録画してもらい、つまずいた場所を数える
4. **技術の試作**: AOA 有線、アプリの疑似カーソル、日本語入力の解き方

## 出典

- [7 Synergy alternatives compared for 2026 | ShareCursor](https://sharecursor.com/synergy-alternatives.html)
- [deskflow/deskflow(GitHub)](https://github.com/deskflow/deskflow)
- [Deskflow Issue #8165: Clipboard sharing is only working sometimes](https://github.com/deskflow/deskflow/issues/8165)
- [Deskflow Issue #8899: Incorrect keyboard layout conversion](https://github.com/deskflow/deskflow/issues/8899)
- [Deskflow Issue #7631: modifier keys activate Chinese input on macOS](https://github.com/deskflow/deskflow/issues/7631)
- [Deskflow で Mac の英数キーを Windows でも Mac 風に使う方法(Qiita)](https://qiita.com/mu-zilch_321/items/79998da1d53758c8b36a)
- [Deskflow で共通化し Mac も…と欲張ったら詰んでいる件(note)](https://note.com/mayucausaldigger/n/ncc25d31afac3)
- [saburahu/synergy(日本語キー対応のパッチ版)](https://github.com/saburahu/synergy/releases)
- [Samsung: Use Multi control](https://www.samsung.com/us/support/answer/ANS10001485/)
- [XDA: Samsung Multi Control with non-Samsung PC](https://xdaforums.com/t/how-to-samsung-multi-control-with-non-samsung-windows-pc.4640205/)
- [XDA: Xiaomi HyperConnect PC client on any PC](https://xdaforums.com/t/xiaomi-hyperconnect-pc-client-on-any-pc-how-to-guide.4702330/)
- [Logitech Flow](https://www.logitech.com/en-us/software/flow)
- [Windows Latest: Windows 11 の Android 連携](https://www.windowslatest.com/2025/07/29/windows-11s-new-android-integration-lets-you-control-pc-transfer-files-and-more-hands-on-demo/)
- [InputShare(GitHub)](https://github.com/InputShare/InputShare)
- [Deskflow Android(F-Droid)](https://f-droid.org/packages/org.tfv.deskflow/)
- [One Mouse(Google Play)](https://play.google.com/store/apps/details?id=com.mouselink.app&hl=en_US)
- [scrcpy OTG モードの文書](https://github.com/Genymobile/scrcpy/blob/master/doc/otg.md)
