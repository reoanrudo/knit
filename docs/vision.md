# Cross-OS Workspace

MacとWindowsを「2台のPC」ではなく「1つの作業環境」にする。

---

## 1. プロダクトの定義

この製品は、マウス・キーボード共有アプリではない。

複数のコンピューターを、1つのデスク・1つの作業環境として扱うためのCross-OS Continuity Layerである。

対象は主に、

- Mac + Windowsを併用する開発者
- クリエイター
- 3D/CADユーザー
- AI/GPU用途でWindowsを併用するMacユーザー
- 仕事用PCと私用PCを並べて使うユーザー
- 複数台のPCを常時使うパワーユーザー

である。

既存のソフトウェアKVMは、

«「1組のマウスとキーボードで複数PCを操作できる」»

ところで止まっている。

本製品が目指すのは、

«「今どのPCを操作しているか考えなくていい」»

状態である。

---

## 2. 解決する本当の問題

MacとWindowsを併用しているユーザーは、日常的に次の小さな摩擦を受けている。

- マウスを持ち替える
- キーボードを切り替える
- CtrlとCommandを頭の中で変換する
- 日本語入力状態がズレる
- URLを自分に送る
- ファイルをAirDrop、クラウド、Slackなどで移動する
- Macで見ていたものをWindowsでもう一度探す
- 「このファイルどっちのPCだっけ」と考える
- 別PCのアプリを起動する
- モニター入力を切り替える
- 音声出力を切り替える
- スリープ復帰後に接続し直す

1つ1つは小さい。

だから既存市場では軽視されやすい。

しかし、1日何十回も発生する。

本製品が取り除くのは、

操作コストではなく、認知コストである。

---

## 3. North Star

プロダクト全体を判断する基準は1つにする。

«ユーザーが「別のPCだった」と意識する回数を0に近づける。»

新機能を考えたときも、

「便利か？」

ではなく、

«「これによってPC間の境界を1回消せるか？」»

で判断する。

---

## 4. 基本思想

現在のKVM製品は、

```
PC A
PC B
PC C
```

を接続する。

本製品では違う。

ユーザーが扱うのは、

MY DESK

だけ。

その中に、

```
┌─────────┐
│ Display │
└─────────┘

┌─────────┐
│ Display │
└─────────┘

┌─────────┐
│ Display │
└─────────┘
```

が存在する。

どの画面がどのコンピューターに接続されているかは、システム側が管理する。

ユーザーにClient / Serverなどの概念をできるだけ見せない。

---

## 5. 製品を構成する4レイヤー

### Layer 1: Input Continuity

最初に徹底的に完成させる領域。

**Mouse**

カーソルを画面端まで移動すると、そのまま隣のPCへ移る。

重要なのは機能ではなく、

違和感がないこと。

- カーソル速度
- acceleration
- DPI
- Retina
- Windows scaling
- マルチモニター
- 60 / 120 / 144Hz

などが違っても、手の感覚を変えない。

**Keyboard**

単純にキーコードを転送しない。

内部的には、

```
Physical Input
    ↓
  Intent
    ↓
Target OS Action
```

として扱う。

例えば、

Ctrl + C

を受け取ったら、

COPY

へ変換する。

Windowsでは、

Ctrl + C

Macでは、

Command + C

として実行する。

これによって、

«「WindowsキーボードでMacを操作する」»

ではなく、

«「いつものコピー操作をする」»

になる。

---

## 6. Semantic Shortcut

これは主要な差別化要素にする。

単なる、

Ctrl → Command

ではない。

アプリごとの意味を理解する。

例えば、

**Browser**

- New Tab
- Close Tab
- Restore Tab
- Address Bar
- Developer Tools

**VS Code**

- Command Palette
- Search
- Go to Definition
- Terminal

**Blender**

- Frame Selected
- Save
- Undo
- Render

などを抽象Commandとして扱う。

そのため、

Mac / Windowsの違いをユーザーが覚える必要がなくなる。

将来的にはユーザー独自ルールも登録できる。

---

## 7. 日本語入力を第一級機能にする

海外製アプリが弱くなりやすい部分なので、ここは戦える。

対応するもの：

- JISキーボード
- 英数
- かな
- 半角/全角
- 変換
- 無変換
- Windows IME
- macOS日本語入力

例えばMacで日本語入力中にWindowsへ移動した場合、

Windowsも日本語入力状態へ同期する。

逆も同様。

ただし強制同期だけではなく、

- IME Follow Cursor
- IME Keep Per Device
- IME Keep Per App

を選択可能にする。

日本市場では、

«Mac + Windows混在環境で一番日本語入力が気持ちいいアプリ»

だけでもかなり明確なポジションを取れる。

---

## 8. Object Continuity

Clipboardという名前から考え直す。

ユーザーが移動させたいのはClipboardではない。

Objectである。

例えば、

- Text
- URL
- Image
- File
- Folder
- PDF
- Code
- Screenshot

これらをデスク全体で扱えるようにする。

---

## 9. Universal Drag & Drop

MacからWindows画面へファイルをドラッグする。

そのまま移動する。

ここまでは他製品にも存在する。

本製品ではさらに、

Drop Targetの意味を理解する。

例えば、

Mac Finder → Windows Photoshop なら、

```
Transfer
    ↓
Open in Photoshop
```

Macから画像をChromeへDropすれば、

```
Transfer
    ↓
Browser Upload
```

ExplorerへDropなら、

Transfer File

になる。

単なるファイル転送ではなく、

Cross-PC Drag & Drop

として扱う。

---

## 10. Universal Clipboard History

全デバイスのClipboardを統合する。

例えば、

```
Clipboard

Windows
────────────────
Screenshot.png
10 sec ago

Mac
────────────────
https://example.com
34 sec ago

Windows
────────────────
API Response
2 min ago
```

検索可能。

カテゴリも、

- Text
- URL
- Image
- File
- Code

で分けられる。

**セキュリティ**

機密情報は別扱いにする。

例えば、

- Password
- OTP
- API Key
- Credit Card
- Private Key

らしき情報を検出した場合、

Local device only

にする。

ユーザーが明示的に許可した場合だけ他PCへ送る。

---

## 11. Continue Here

ここがMVP最大の差別化候補。

例えばMacでWebページを読んでいる。

Windowsへカーソルを移動。

ショートカットを押す。

すると、

Windowsで同じページが開く。

単なるURL共有ではない。

可能なら、

- URL
- Scroll Position
- Selected Text

まで渡す。

---

## 12. App Handoff

Continue Hereをアプリ単位に拡張する。

**Browser**

- URL
- Tab
- Scroll Position

**VS Code**

- Repository
- Branch
- File
- Cursor Position

**Terminal**

- Working Directory
- Repository
- Command Context

**PDF**

- File
- Page
- Zoom

**Blender**

- .blend file
- Scene
- Object
- Frame

**Figma / Notion**

- Document URL
- Current Page

ユーザーからすると、

«作業を別PCへ投げた»

ように感じる。

---

## 13. Throw

Continue Hereを視覚的にする。

ウィンドウを画面端へドラッグする。

隣のPCに、

Open this on Windows?

が出る。

離す。

対象アプリが起動する。

実際にはウィンドウそのものを移しているわけではない。

移動しているのは、

Context

である。

しかしUXとしては、

«ウィンドウを別PCへ投げた»

ように感じられる。

ここはデモ映えも非常に強い。

---

## 14. Global Command Palette

次の大きな差別化。

どのPCにいても同じショートカットで、

Search My Desk

を開く。

例えば、

> Japanese House

と入力すると、

```
Windows
JapaneseHouse.blend
Blender
Opened 20 min ago

Mac
JapaneseHouse_reference.pdf

Mac
JapaneseHouse_notes.md
```

などが横断検索される。

対象：

- Apps
- Files
- Folders
- Browser tabs
- Clipboard
- Commands
- Projects

つまり、

«Spotlight / PowerToysをDesk全体へ広げる。»

---

## 15. Compute Handoff

将来的に特に強い機能。

Macで作業していても、

GPU処理だけWindowsへ送る。

例えば、

Render this

を実行すると、

```
MacBook
CPU/GPU
Estimated 7m 20s

Windows
RTX 4080
Estimated 1m 35s
```

となり、

WindowsへJobを送る。

対象候補：

- Blender Render
- FFmpeg
- AI inference
- Stable Diffusion
- LLM
- Build
- Compression
- Video encoding

これによって、

«どのPCを操作するか»

から、

«どのCompute Resourceを使うか»

へ世界観が変わる。

最終的にはユーザーすら選択せず、自動判断できる。

---

## 16. Audio Continuity

PCを複数使っていると、意外と音が邪魔になる。

そこで、

Audio Source

という概念を作る。

例えば、

```
Current Audio Source
Windows
```

とする。

カーソル移動では勝手に変えない。

代わりに、

- Follow Active App
- Follow Meeting
- Manual

などを選べる。

将来的には、

Macの音をWindowsにつないだヘッドホンから聞く、

といったAudio Routingまで拡張可能。

---

## 17. Display Control

対応ディスプレイならDDC/CIを使う。

アプリから、

- Input Source
- Brightness
- Volume

などを変更する。

これによって、

Monitor
Keyboard
Mouse
Audio
Applications

を1つのWorkspaceとして扱える。

---

## 18. Workspace

例えば、

```
Development
  Mac:     Safari / Notion
  Windows: VS Code / Docker / Local AI

Blender
  Mac:     Reference / ChatGPT
  Windows: Blender

Gaming
  Windows: Main display / Audio / Mouse locked
  Mac:     Dim screen
```

これを保存する。

そして、

Activate Workspace

一発で復元する。

---

## 19. 最終的なプロダクト構造

```
                MY DESK

                  ↓

        ┌─────────────────┐
        │    Workspace    │
        │ Search / Audio  │
        │ Display / Task  │
        └────────┬────────┘
                 │
        ┌────────▼────────┐
        │     Context     │
        │ Handoff / Throw │
        └────────┬────────┘
                 │
        ┌────────▼────────┐
        │      Object     │
        │ File / Clipboard│
        └────────┬────────┘
                 │
        ┌────────▼────────┐
        │      Input      │
        │Mouse / Keyboard │
        │IME / Trackpad   │
        └─────────────────┘
```

重要なのは、

下から順番に完成させること。

Inputが不安定なのにAI Command Paletteを作っても意味がない。

カーソルがたまに引っかかる製品に「AI Workspace」と書いてあっても、人間はちゃんと腹を立てる。

---

## 20. MVP

最初の製品は欲張らない。

**MVP 1**: Mouse Continuity — 絶対に途切れない。

**MVP 2**: Semantic Keyboard — Ctrl / Commandを意識しない。

**MVP 3**: Perfect Japanese Input — IME/JIS対応。

**MVP 4**: Universal Clipboard — Text / Image / File。

**MVP 5**: Cross-PC Drag & Drop — ファイルを画面間で移動。

**MVP 6**: Continue Here — Browser URLを別PCへ送れる。

これだけで十分。

---

## 21. MVPで入れないもの

初期版では以下を切る。

- AI
- Audio streaming
- Remote desktop
- GPU scheduling
- Monitor DDC
- Cloud account
- Mobile
- Linux
- Workspace restore
- Plugin marketplace

魅力的だが、

すべて後。

初期版の評価は、

«MacとWindowsを並べて使ったときに「もう元へ戻りたくない」と感じるか»

だけを見る。

---

## 22. 最初の5分

ここは極端にこだわる。

**Step 1**: Macへインストール。

**Step 2**: Windowsへインストール。

**Step 3**: 自動的にお互いを発見。Windows PC found

**Step 4**: 6桁コードでPair。

**Step 5**: 画面配置。

```
[ Mac ] [ Windows ]
```

ドラッグだけ。

**Step 6**: 表示。Move your mouse to the right.

**Step 7**: Windowsへカーソルが移る。

そして、

Try Ctrl+C on Windows
and paste on Mac.

コピーできる。

ここで初めて、

«「これは違う»»

と思わせる。

IPアドレス入力、ポート番号、Server/Client設定、証明書設定。

そういうものは全部隠す。

---

## 23. 一番重要なUX

実は設定画面ではない。

Cursor Crossing

である。

画面端へカーソルを動かした瞬間、

```
Mac
 ↓
Windows
```

へ入る。

この0.1秒前後の感覚が商品そのものになる。

ここが気持ち悪ければ全製品が失敗。

だから、

- Latency
- Acceleration
- Crossing threshold
- Edge resistance
- DPI conversion
- Coordinate mapping

には異常なくらいこだわる。

---

## 24. Crossing Intelligence

さらに賢くする。

画面端へ偶然カーソルが触れただけなら移動しない。

例えば、

- Velocity
- Direction
- Edge dwell
- Cursor angle

を使って、

«隣の画面へ行こうとしている»

ことを判定する。

これによって誤Crossを減らす。

Gaming中なら自動Lock。

Fullscreen動画でもLock。

ドラッグ中ならCross可能。

---

## 25. Connection設計

ユーザーにネットワークを意識させない。

基本は、

Local Network

最優先。

Internet Relayは将来。

接続優先順位：

```
Ethernet
  ↓
Wi-Fi Direct / LAN
  ↓
Normal LAN
  ↓
Relay
```

ユーザーから見れば、

Connected

だけ。

---

## 26. セキュリティポジション

このカテゴリではかなり重要。

メッセージはシンプルにする。

«Your keyboard never needs the cloud.»

基本通信はLAN。

通信は暗号化。

Clipboardもローカル。

Account不要でも使える。

将来的なCloud機能はOpt-in。

企業利用では、

LAN Only

をPolicyとして強制可能にする。

---

## 27. Free / Pro

無料版はケチらない。

**Free**

- 2 computers
- Mouse
- Keyboard
- Text Clipboard
- Basic shortcut translation

これで普及する。

**Pro**

例えば、

- Unlimited devices
- File/Image Clipboard
- Drag & Drop
- Clipboard History
- Continue Here
- Semantic Shortcuts
- IME Advanced
- Workspace
- Global Search

月額より、

年間ライセンスまたは買い切り + Major Upgrade

との相性も良い。

このジャンルのユーザーは、

「マウス共有に毎月払う」

より、

「毎日使うPCユーティリティを買う」

方が心理的に自然。

---

## 28. 最初に狙うユーザー

一般人を狙わない。

最初は、

Mac + Windows Power User

だけでいい。

特に、

- Software developers
- AI developers
- 3D artists
- CAD users
- Video creators
- Designers
- Streamers

この層。

なぜなら、

複数PCを使う必然性がある。

そして不便にも敏感。

---

## 29. キャッチコピー

機能説明から入らない。

第一候補：

«Two computers. One workspace.»

その下に、

«Use your Mac and Windows PC like one computer.»

日本語なら、

«MacとWindowsを、ひとつの作業環境に。»

そして説明：

«マウス、キーボード、ファイル、クリップボード、そして作業そのものを、PCの境界を意識せず行き来できます。»

---

## 30. 本当の競争相手

Barrierではない。

Deskflowでもない。

ShareMouseでもない。

最終的な競争相手は、

«「PCはそれぞれ独立したもの」という常識»

である。

Appleは自社エコシステム内ではContinuityを作っている。

MicrosoftもWindows内部ではPC連携を進めている。

しかしMac + Windows混在環境は、いまだにユーザーが自力で繋いでいる。

そこを取る。

---

## 31. 製品としての最終ビジョン

ユーザーが、

MacBook
Windows Desktop
Work Laptop
Mini PC

を持っていても、

意識するのは、

MY DESK

だけ。

ファイルがどこにあるか。

どのOSなのか。

どのPCなのか。

どのGPUなのか。

どのキーボードなのか。

そういうコンピューター側の事情を、できる限りシステム側へ押し込む。

そして、

«人間は「何をしたいか」だけ考える。»

これがこの製品の完成形。

---

## 一文で定義するなら

«Cross-OS Workspaceは、MacとWindowsの入力・データ・アプリ・作業コンテキストを接続し、複数のコンピューターを1つのデスクとして使えるようにするソフトウェアである。»

最初は最高のソフトウェアKVMとして始める。

次にContinuityになる。

最後には、

«DeskそのものがOSになる。»

そこまで伸ばせるプロダクト。

---

## 実装状況メモ(2026-09-26 時点・改善ループ460)

§19「下から順番に完成させる」に対する現在地。

| レイヤー | テーマ | 状態 |
|---|---|---|
| Input | マウス/キーボード/ショートカット翻訳/音量・メディアキー | 実装済み・品質磨き中(改善ループ416〜459) |
| Object | クリップボード同期(テキスト/画像/ファイル)・ドラッグ&ドロップ・掴みドラッグ | 実装済み(履歴 §10 は未) |
| Context | Continue Here / App Handoff / Throw | 未実装 → 次の差別化候補 |
| Workspace | Global Search / Audio Routing / Display | 未実装(Windows→Mac 音声転送のみ試験実装) |

MVP(§20)との照合:

1. **Mouse Continuity** — 済み(LAN直優先・経路昇格・遅延表示)
2. **Semantic Keyboard** — 基本の Ctrl/Cmd 変換は済み。アプリ別セマンティック(§6)は将来
3. **Perfect Japanese Input** — キー転送は済み。IME状態同期(§7)は未
4. **Universal Clipboard** — 済み(画面を移る時だけ同期の設計)
5. **Cross-PC Drag & Drop** — 済み
6. **Continue Here** — 未実装

§22「最初の5分」(自動発見・6桁コード)は並行作業(ペアリング)が対応中。
§25「Connection設計」の「LAN最優先・ユーザーにはConnectedだけ」は
LAN直優先接続(改善ループ447)で方針どおり実現済み。
