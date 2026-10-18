# Knit D&D 増分の敵対的検証(verifier / team task-6)

- 作成: 2026-10-18 / 担当: 敵対的検証(task-6)
- 対象: [設計](2026-10-18-knit-dnd-design.md) §4 の増分 I1〜 / [摩擦](2026-10-18-knit-dnd-friction.md)
- 基準リビジョン: **`a3f9512`**(設計・摩擦docの対象と同じ。548 passed / 0 failed、作業ツリーはクリーン)
- 立場: **実装役の自己申告を信用しない。** 同じコマンドを自分で走らせ、差分を自分で読み、反例を探す。
  この doc の価値は「合格と言うこと」ではなく「壊れている箇所を見つけること」にある。
- 制約: **製品コードは変更しない**(検証と本 doc のみ)。`crates/win` は触らない。判定不能な点は「未検証」と書く。
- 本 doc は増分ごとに**追記**する(既存の記述は消さない。判定を覆すときは「訂正」として残す)。

---

## 0. 第1段階の成果(この doc の使い方)

| 成果物 | 場所 |
|---|---|
| 文言ゴールデンリスト(抽出器) | 本 doc §1.4 に全文。実行体は `/tmp/dndverify/golden.py`(再作成可) |
| 基準値(rev `a3f9512`) | §1.2 のハッシュ3種。増分後に再計算して `diff` する |
| 掴み経路の文言(逐語) | §1.3 = **I1 の合否を決める一次資料** |
| 拒否分岐の地図 | §2 = 8分岐の実体(うち1つは**拒否ではない**。最重要の罠) |
| 検証手順 | §3(共通手順 C1〜C9)と §4(増分別の重点) |
| 反例探索の型 | §3.3(X1〜X10) |
| 判定の記録 | §6(増分ごとに追記) |

---

## 1. ユーザー可視文言のゴールデンリスト

### 1.1 目的と定義

設計 I1 は「**文言を1文字も変えない**」と明記している(設計 §4 I1)。したがって増分の前後で
ユーザーが目にする文字列が変化していないことを、**目視ではなく機械的に**判定できる基準が要る。
ここでは次の3層で固定する。

| 層 | 何を固定するか | 不変条件 |
|---|---|---|
| **Tier A** | 掴み経路(D&D)の通知・状態行・中止・確認の文言(逐語) | **1文字も変わらない**(バイト一致) |
| **Tier B** | `crates/mac/src` と `crates/common/src` の全 **日本語を含む文字列リテラル** | 多重集合(テキストのみ)が一致。追加は §3.1 C3 の規則で1件ずつ説明を要する |
| **Tier C** | 表示関数の文脈にある **ASCII のみ**のリテラル(`"Knit"` 等) | Tier B と同じ |

Tier B/C は「テストの assert メッセージ」も含む。ユーザー可視ではないが、
**実装の写しではない固定**(characterization)として価値があるため除外しない。
逆に、Tier B に含まれるからといって「ユーザー可視」と主張してはいけない(§1.6 の限界を参照)。

### 1.2 基準値(自分で実行して採取。rev `a3f9512`)

```
$ python3 /tmp/dndverify/golden.py a3f9512
ASCII-only display-context literals: 76 unique: 23
rev=a3f9512 occurrences=2494 unique_text=1926 unique_filetext=2233
H_multiset = 8ecdd62624c63c4f4a5d938de8ebb78bd1fb5ad146446fd79b9f3e80982670a3
H_filetext = 15aa590267d9bad263603225aaea34d31b2a6d8ad7e8812960ecba67ce861f67
per-file: localization.rs:253 prefs.rs:237 gui.rs:229 refined.rs:172 history.rs:169 tap.rs:126
          diagnose.rs:113 main.rs:108 setup.rs:97 android.rs:71 files.rs:70 clip.rs:69 bulk.rs:52
          lib.rs:52 conn.rs:52 desktop_settings.rs:48 preferences.rs:44 session.rs:42 audio.rs:41
          (以下略。全 56 ファイル)
```

| 指標 | 定義 | 値 |
|---|---|---|
| `occurrences` | 日本語リテラルの出現数(重複込み) | **2494** |
| `unique_text` | テキストのみの一意集合(**ファイル間の移動に不変**) | **1926** |
| `unique_filetext` | (ファイル, テキスト) の一意集合 | **2233** |
| `H_multiset` | 上の「テキストのみ」多重集合の sha256。**移動に不変** | `8ecdd626…670a3` |
| `H_filetext` | (ファイル, テキスト) 集合の sha256。**移動で変化する** | `15aa5902…861f67` |
| Tier C | 表示文脈の ASCII リテラル出現数 / 一意 | 76 / 23 |

**判定に使うのは `H_multiset`。** 文言を書き換えずに置き場所だけ変える増分(I1 がまさにこれ)では
`H_filetext` は必ず変わる。`H_multiset` が変わったときだけ「文言が動いた」と判定する。

#### 基準のビルド・テスト(2026-10-18 に verifier が自分で実行した実測)

```
$ cargo build > /tmp/verify_build_base.log 2>&1; echo $?      -> 0
$ cargo test  > /tmp/verify_test_base.log  2>&1; echo $?      -> 0
$ grep -E "^test result" /tmp/verify_test_base.log | awk '{p+=$4; f+=$6; i+=$8} END{...}'
passed_total=548 failed_total=0 ignored_total=4 binaries=11
$ grep -c "^warning" /tmp/verify_build_base.log               -> 10
$ git rev-parse --short HEAD                                  -> a3f9512
```

**申告されていた 548 passed / 0 failed は事実と一致した**(設計 §2 AC6 の基準値を自分で確認できた)。
増分の判定は `548 + 新規テスト数` を期待値とし、**`ignored` の増加も検出する**(基準は 4)。

#### 【訂正】計器の走査範囲(2026-10-18 I2 検証時に発見・修正)

初版の抽出器は `crates/mac/src` と `crates/common/src` だけを走査しており、
**統合テスト(`crates/common/tests/`・`crates/mac/tests/`)が漏れていた**。
§1.1 で「テストの assert メッセージも含む」と書いた以上、これは計器の欠陥である
(I1 の検証時には気づかず、I2 の検証で気づいた)。走査対象を4ディレクトリに広げて再基準化した。

| 指標 | 初版(production のみ) | **訂正後(src + tests)** |
|---|---|---|
| `occurrences` | 2494 | **2567** |
| `unique_text` | 1926 | **1990** |
| `unique_filetext` | 2233 | **2299** |
| `H_multiset`(基準 `a3f9512`) | `8ecdd626…670a3`(旧・以後使わない) | **`7719419478265c1eebef0cc07c06d51b66feaad34b040555389addd9a58cbbe7`** |
| `H_filetext`(基準 `a3f9512`) | `15aa5902…861f67`(旧) | **`12dbe9ebdce0f466d7130ff0122a8ab3eb51b4bebfe2763578a2a616684c4e12`** |

上の §1.2 の表(2494/1926/2233)は**初版の値**であり、この訂正表が現在の基準である。
**I1 の結論は変わらない**: 拡張スキャンで `a3f9512 → 43de2e2` を再比較した結果、
production 側の変化は L6 のリテラル分割1件のみで、残りの差分はすべて
`crates/common/tests/drag_handoff.rs` に足されたテスト期待値・assert メッセージだった。
**教訓**: 計器の走査範囲は「ユーザー可視の文言が現れ得る場所」と一致させる。
`crates/common/tests/` を外したままでは、テスト側で文言を書き換えても検出できなかった。

### 1.3 Tier A: 掴み経路のユーザー可視文言(逐語・`files.rs` 行番号は `a3f9512`)

I1 が触る範囲。**この表の文字列が1文字でも変われば I1 は不合格**(設計 §4 I1)。

| # | 位置 | 種別 | 文字列(逐語) |
|---|---|---|---|
| **L1** | `crates/mac/src/files.rs:286` | notify | `掴んだまま渡せるのは {} フォルダ/ファイルまでです`(`{}` = `drag::MAX_FILES`) |
| **L2** | `crates/mac/src/files.rs:306` | notify | `ファイルの転送経路がまだ準備中です。少し待ってもう一度掴んでください` |
| **L3** | `crates/mac/src/files.rs:341` | notify | `接続の状態が変わったため、ファイルをもう一度掴み直してください` |
| **L4** | `crates/mac/src/files.rs:361` | notify | `ファイルの確認がまだ終わっていません。少し待ってからもう一度境界へ動かしてください` |
| **L5** | `crates/mac/src/files.rs:374` | notify | `掴んだまま渡せません({})`(`{}` = `bulk::send_error_label(&e)`) |
| **L6** | `crates/mac/src/files.rs:388` | notify | `掴んだまま渡せるのは通常のファイルとフォルダだけです{skip_note}` ← **拒否ではない**(§2) |
| **L7** | `crates/mac/src/files.rs:393` | notify | `掴んだ項目の中に送れるものがありません` |
| **L8** | `crates/mac/src/files.rs:400` | notify | `前のファイルを転送中です。完了後にもう一度掴んでください` |
| L9 | `crates/mac/src/files.rs:496` | notify | `ファイル転送を中止しました` |
| L10 | `crates/mac/src/files.rs:501` | notify | `{peer} へファイルを渡せませんでした({}。接続を確認してもう一度掴んでください)` |
| L11 | `crates/mac/src/incoming_drag.rs:508` | notify | `Macのドラッグを開始できませんでした。受信ファイルはDownloads/Knitに保存されています` |
| L12 | `crates/mac/src/files.rs:785` | 状態行 | `転送中 {pct}%・{}/s・{}` |
| L13 | `crates/mac/src/files.rs:802` | 状態行 | `ファイルを受信中` |
| L14 | `crates/mac/src/gui.rs:1999` | 状態行 | `ファイルを送信中({line})` |
| L15 | `crates/mac/src/gui.rs:2005` | 状態行 | `ファイルを受信中` |
| L16 | `crates/mac/src/gui.rs:2346` | タイトル | `ファイルを受信中` |
| L17 | `crates/mac/src/gui.rs:2388` | メニュー | `転送を中止する({line}・Esc でも可)` |
| L18 | `crates/mac/src/gui.rs:2011` | 確認文 | `{}。終了すると中止され、相手側でも中途半端なままになります。` |
| L19 | `crates/mac/src/gui.rs:2028` | 確認文 | `ファイル転送中です` / `中止して終了` |
| L20 | `crates/mac/src/gui.rs:2821` | メニュー | `転送: なし` / `ファイルを送る…`(`:2834`) |

**L5 の引数**は `bulk::send_error_label(&e)`(`crates/common/src/bulk.rs:369-416`)。この関数が出す
11種の文言も Tier A 相当として固定する(掴み経路の拒否理由がここに合流するため)。

| 位置(`common/src/bulk.rs`) | 文字列 |
|---|---|
| `:375` | `ファイル転送経路が未接続(接続の回復を待ってから再送してください)` |
| `:377` | `ファイルの共有が許可されていません` |
| `:378` | `転送中にファイルが変わりました` |
| `:379` | `中止しました` |
| `:383` | `相手のアプリが古く、フォルダの受け渡しに対応していません(ファイル単体を選んでください)` |
| `:390` | `展開後 {actual} 件以上(上限 {limit} 件)` |
| `:401` | `合計 {actual_label}(上限 {})` |
| `:404` | `件数または容量が上限を超えています(1回は展開後 {} 件・合計 {} まで)` |
| `:410` | `転送が時間切れになりました` |
| `:411` | `接続が切れました` |
| `:412` | `転送に失敗しました` |

**注意(§1.3 の罠)**: `ファイル転送経路が未接続(接続の回復を待ってから再送してください)` は
`bulk.rs:375` と `bulk.rs:1542`(テスト内)の**2箇所**にあり、`ファイル転送を中止しました` は
`files.rs:179`(通常送信)と `files.rs:496`(掴み経路)の**2箇所**にある。
I1 が「同じ理由は1箇所に」と称して共通化すると、**別経路の文言まで巻き込んで変える**危険がある。
Tier B の多重集合で**出現数の増減**まで見るのはこのため。

### 1.4 Tier B/C の抽出器(全文。これが再現手段)

```python
#!/usr/bin/env python3
"""Knit ユーザー可視文言ゴールデンリスト抽出器 (verifier/task-6)
使い方: python3 golden.py [git-rev]   -> /tmp/dndverify/ に baseline を書く
rev 省略時は作業ツリーを読む。"""
import re,os,sys,hashlib,subprocess,collections
ROOT="/Users/taguchireo/Documents/八幡平市地域おこし協力隊/05_個人/knit"
os.chdir(ROOT)
REV=sys.argv[1] if len(sys.argv)>1 else None
DIRS=["crates/mac/src","crates/common/src","crates/mac/tests","crates/common/tests"]
tag0=(sys.argv[1] if len(sys.argv)>1 else "worktree")
JP=re.compile(r'[ぁ-んァ-ヴー一-龯、。「」『』（）〈〉・…：？！％〜／]')
CTX=re.compile(r'notify\(|confirm_alert|set_?[Tt]itle|menu_item|setMenuItem|tooltip|ToolTip|balloon|toast|\.text\(|label:|title:|body:|message')
def files():
    if REV:
        out=subprocess.run(["git","ls-tree","-r","--name-only",REV],capture_output=True,text=True).stdout.split()
        return [p for p in out if any(p.startswith(d+"/") for d in DIRS) and p.endswith(".rs")]
    r=[]
    for d in DIRS:
        for dp,_,fs in os.walk(os.path.join(ROOT,d)):
            r+= [os.path.relpath(os.path.join(dp,f),ROOT) for f in sorted(fs) if f.endswith(".rs")]
    return sorted(r)
def read(rel):
    if REV: return subprocess.run(["git","show",f"{REV}:{rel}"],capture_output=True,text=True).stdout
    return open(os.path.join(ROOT,rel),encoding="utf-8").read()
def literals(src):
    i=0;n=len(src);out=[];line=1
    while i<n:
        c=src[i]
        if c=='/' and i+1<n and src[i+1]=='/':
            j=src.find('\n',i); i=n if j==-1 else j; continue
        if c=='/' and i+1<n and src[i+1]=='*':
            j=src.find('*/',i+2); j=n if j==-1 else j+2
            line+=src.count('\n',i,j); i=j; continue
        if c=='\n': line+=1; i+=1; continue
        if c=='r' and i+1<n and src[i+1] in '#"':
            m=re.compile(r'r(#*)"').match(src,i)
            if m:
                h=m.group(1); end=src.find('"'+h,m.end()); end=n if end==-1 else end
                out.append((line,src[m.end():end])); i=end+1+len(h); continue
        if c=='"':
            j=i+1;buf=[]
            while j<n:
                if src[j]=='\\': buf.append(src[j:j+2]); j+=2; continue
                if src[j]=='"' or src[j]=='\n': break
                buf.append(src[j]); j+=1
            out.append((line,''.join(buf))); line+=src.count('\n',i,j); i=j+1; continue
        if c=="'":
            m=re.compile(r"'(?:\\.|[^\\'])'").match(src,i)
            if m: i=m.end(); continue
            i+=1; continue
        i+=1
    return out
rows=[]
for rel in files():
    src=read(rel)
    for ln,s in literals(src):
        if JP.search(s) and s.strip():
            ctx=CTX.search(src.split("\n")[ln-1]) or ""
            rows.append((rel,ln,s,bool(ctx)))
rows.sort(key=lambda r:(r[0],r[1]))
ctxrows=[]
for rel in files():
    src=read(rel)
    for ln,s2 in literals(src):
        if not JP.search(s2) and CTX.search(src.split("\n")[ln-1]) and s2.strip() and len(s2)>1:
            ctxrows.append((rel,ln,s2))
ctxrows.sort(key=lambda r:(r[0],r[1]))
open(f"/tmp/dndverify/asciictx_{tag0}.txt","w").write("".join(f"{a}\t{b}\t{c}\n" for a,b,c in ctxrows))
print("ASCII-only display-context literals:",len(ctxrows),"unique:",len({c for _,_,c in ctxrows}))
ms=sorted(s for _,_,s,_ in rows)                      # ファイル非依存の多重集合(移動に不変)
ft=sorted({(a,c) for a,_,c,_ in rows})                 # (file,text) 一意集合
d=lambda xs: hashlib.sha256("\n".join(xs).encode()).hexdigest()
tag=REV or "worktree"
open(f"/tmp/dndverify/multiset_{tag}.txt","w").write("\n".join(ms)+"\n")
open(f"/tmp/dndverify/filetext_{tag}.txt","w").write("\n".join(f"{a}\x01{b}" for a,b in ft)+"\n")
open(f"/tmp/dndverify/rows_{tag}.tsv","w").write("".join(f"{a}\t{b}\t{int(d_)}\t{c}\n" for a,b,c,d_ in rows))
print(f"rev={tag} occurrences={len(rows)} unique_text={len(set(ms))} unique_filetext={len(ft)}")
print("H_multiset =",d(ms)); print("H_filetext =",d([f"{a}\x01{b}" for a,b in ft]))
per=collections.Counter(a for a,_,_,_ in rows)
print("per-file:", " ".join(f"{k.split('/')[-1]}:{v}" for k,v in per.most_common()))
```

**増分後の差分の取り方**(これを毎回そのまま走らせる)。

```bash
cd "/Users/taguchireo/Documents/八幡平市地域おこし協力隊/05_個人/knit"
V=<検証対象のコミット>          # 例 I1 のコミット
python3 /tmp/dndverify/golden.py a3f9512   # 基準(不変)
python3 /tmp/dndverify/golden.py $V        # 検証対象のコミット
diff /tmp/dndverify/multiset_a3f9512.txt /tmp/dndverify/multiset_$V.txt   # 文言の増減(移動に不変)
diff /tmp/dndverify/filetext_a3f9512.txt /tmp/dndverify/filetext_$V.txt   # 移動・重複の検出
```

**必ずコミットを指定して比較する(`rev` 省略 = 作業ツリーを読む)。** 2026-10-18 時点で
作業ツリーには `M crates/common/tests/drag_handoff.rs` があり、**実装役の作業中の変更が混ざる**。
作業ツリーを基準にすると、検証対象のコミットに含まれない変更を「その増分の変更」と誤認する。

- `multiset` の差分が **空** なら「文言は1文字も変わっていない」と**機械的に言い切れる**(I1 の必要条件)。
- `multiset` の差分が `>`(追加)のみで、そのすべてが **まだ表示経路に繋がっていない値**(例: `Refusal` の
  `next_step`)である場合に限り、「表示文言は不変」と判定してよい。**判定には追加文字列ごとの根拠
  (その文字列を表示する呼び出しが同じコミットに無いこと)を `grep` で示す。**
- `multiset` に `<`(削除)や置換が出たら、**原則は不合格**として差分を提示する。
- **例外(2026-10-18 追記。I1 の検証で実際に踏んだ)**: リテラルの**分割**であり、元の式と新しい式を
  両方引用して**描画結果がバイト一致**することを示せる場合は「文言不変」と判定してよい。
  補間(`{skip_note}`)をリテラルから書式引数へ移す変更がこれに当たる。判定には必ず
  「元の式 / 新しい式 / 同一である理由」を記録する(推測で通してはいけない)。

### 1.5 検証環境の再現性(毎回自分で確認する)

```bash
git rev-parse --short HEAD          # 申告されたコミットと一致するか
git status --porcelain              # 空であること(未コミットの変更を検証しない)
cargo build > /tmp/verify_build.log 2>&1; echo "exit=$?"; tail -5 /tmp/verify_build.log
cargo test  > /tmp/verify_test.log  2>&1; echo "exit=$?"; grep -E "^test result" /tmp/verify_test.log | tail -5
```

基準(rev `a3f9512`)は **548 passed / 0 failed**(設計 §2 AC6)。増分後の passed は
`548 + 新規テスト数` でなければならない。**新規テスト数の申告と実測が食い違えばそれ自体を欠陥として報告する。**

### 1.6 限界(この golden list で検出できないもの)

1. **ASCII のみの本文**は `CTX` に一致する行にしか掛からない(`"Knit"`・`"Downloads/Knit"` 等)。
   検出行をまたぐ `format!` の断片や、変数経由の文言は取り逃す。
2. `.rs` 以外(`gui/localization.rs` は `.rs` なので対象内だが、HTML/JSON/plist 資産)は対象外。
   **未確認**: そのような資産が macOS の通知・メニュー文言を持っているかは本 doc では確認していない。
3. 「同じ意味の文を別リテラルで書き直した」場合は検出できる(削除＋追加になる)が、
   **「表示されない文字列を削った」**は区別できない。差分が出たら人が読む。
4. Tier B はテスト用の assert メッセージも含むため、「Tier B に無い = ユーザー可視」ではない。
   ユーザー可視の判定は §1.3(Tier A)と §4(b)の経路追跡で行う。

---

## 2. 拒否分岐の地図(`a3f9512` で自分が読んだ結果)

`offer_drag_to_win`(`crates/mac/src/files.rs:281-422`)が `None`(越境しない)を返す経路は **7つ**、
`Some(id)` を返すが通知を出す経路が **1つ**ある。設計 §2 AC3 と摩擦doc F1 は「8分岐」と呼ぶが、
**8つ目(L6)は拒否ではない。**

| # | 行 | 条件 | 文言 | 戻り値 | 副作用 |
|---|---|---|---|---|---|
| B1 | `:282-291` | `paths.is_empty()` **または** `len > MAX_FILES` | L1 | `None` | なし |
| B2 | `:299-312` | `!BULK_LINK.is_up_fast()` かつ猶予窓(10秒)の外 | L2 | `None` | なし |
| B3 | `:335-344` | キャッシュがあり `paths`/`allow_dirs`/`keep_empty_dirs` のいずれかが不一致 | L3 | `None` | なし |
| B4 | `:345-364` | キャッシュが来ないまま 10ms×15(150ms 未満)を使い切った | L4 | `None` | **sleep を最大 14 回**(tap スレッド) |
| B5 | `:366-380` | `collect` が `Err` | `format!(L5, send_error_label(&e))` | `None` | なし |
| **B6** | `:384-390` | `skip_note` が空でない(送れない項目を落とした) | L6 | **`Some(id)` に進む** | **通知のみ。越境はする** |
| B7 | `:391-395` | `entries.is_empty()` | L7 | `None` | なし |
| B8 | `:396-403` | `FILE_TX_BUSY.swap(true)` が真(前の転送中) | L8 | `None` | **busy フラグを立てる**(成功時は転送側で解除) |

呼び出し元は `crates/mac/src/tap.rs:2101-2108`。`None` のとき `FILE_DRAG.refuse()` +
`EDGE_AT_EDGE`/`EDGE_LAST_HIT_MS`/`EDGE_STAY_SINCE_MS` をリセットして `return event`(越境しない)。
`Some(id)` のときは `handoff = Some((files, id))` として越境が続く。

**検証上の含意(I1 で突くべき罠)**

- **T1**: B6 を `Refusal` に畳んで `None` を返すと、「一部に送れない物がある掴み」が
  **越境できなくなる**(挙動の退行)。`Verdict` は少なくとも「拒否」と「注意つきで通す」を
  区別できる必要がある。設計 I1 の文言 `Verdict::{Ready, Refused}` だけでは B6 を表現できない。
- **T2**: B8 は `FILE_TX_BUSY` を**立ててから** `None` を返す唯一の分岐。順序を入れ替える
  (判定を先に値化し、後で `swap` する)と、`Some(id)` のあとに `swap` が来て
  二重送信の窓ができる。**値化した判定と副作用の順序が保存されているか**を diff で見る。
- **T3**: B1 は空(`paths.is_empty()`)と超過(`> MAX_FILES`)を同じ文言で扱う。
  `Refusal::TooMany` に畳むと「0件なのに64件までと言われる」理由の食い違いが出る
  (文言は同じなので Tier B では検出できない。**値の粒度を見る**)。
- **T4**: B4 は `sleep` を伴う。I1 の「判定を純関数へ」で sleep まで common に移すと、
  純関数が tap スレッドを 150ms 止める。**sleep は mac 側に残っているか**。

---

## 3. 全増分に共通の検証手順

### 3.1 手順(この順で実行し、各段の生出力を残す)

| # | やること | コマンド / 見方 | 落ちたら |
|---|---|---|---|
| **C1** | 申告との突き合わせ | `git rev-parse --short HEAD`, `git status --porcelain` | 未コミット・別 rev なら検証を中断して報告 |
| **C2** | ビルドを自分で回す | `cargo build > /tmp/verify_build.log 2>&1; echo $?; tail -5` | 失敗は即不合格 |
| **C3** | テストを自分で回す | `cargo test > /tmp/verify_test.log 2>&1; echo $?; grep "^test result" \| tail -3` | passed 不一致・failed≠0 は即不合格 |
| **C4** | 文言の機械照合 | §1.4 の3コマンド。`multiset` の差分を列挙 | 削除・置換があれば不合格 |
| **C5** | 差分を全文読む | `git show <commit>`(stat だけで済ませない) | — |
| **C6** | 設計要件 (a) 値/表示の分離 | 判定が `common` の純関数/enum か。`mac` 側に判断が残っていないか | `mac` に分岐条件が残っていたら条件付き |
| **C7** | 設計要件 (b) 文言不変 | §1.3 の逐語と diff の突き合わせ | 1文字でも違えば不合格 |
| **C8** | 設計要件 (c) 分岐の網羅 | §2 の B1〜B8 を1つずつ追い、**値を通ったか**を確認 | 値を残さず素通りする分岐があれば不合格 |
| **C9** | 設計要件 (d) §6 の16項目 | §5 のチェックリストを当てる | 該当項目に退行があれば不合格 |

### 3.2 「値を残さず素通り」の判定方法(C8 の具体化)

各分岐について次を1行ずつ証拠付きで書く。

```
B1: files.rs:282 -> verdict_for(...) の ? -> Refusal::TooMany が値として返る / notify は値から組まれる
B3: ... (分岐の条件式が値の入力になっているか)
B6: Refusal ではなく Ready{note} 相当か(§2 T1)
```

判定の型:
- **素通り**: 分岐の条件式が変わったのに `Verdict`/`Refusal` に新しい値が無い、
  または `let _ =` / `unwrap_or` で値が捨てられている。
- **値を通った**: 分岐の条件が enum のバリアント(またはその生成)に写っており、
  表示文字列がその値から引かれている(文言リテラルが `mac` の分岐内に残っていない)。

### 3.3 反例探索の型(X1〜X10。増分ごとに該当するものを実行する)

| # | 探す反例 | 具体的な突き方 |
|---|---|---|
| **X1** | **理由と表示の食い違い**: `Verdict` の理由と、実際に `notify` へ渡る文字列が別の分岐のもの | 表テストの期待値を**リテラル**で書き、`grep -n` で各文言の出現が1箇所か確認 |
| **X2** | **同じ理由が2箇所で別文言**: 経路未接続は `files.rs:306`(掴み)と `bulk.rs:375`(通常送信)に別文言である。共通化で片方が変わる | Tier B の多重集合で当該文字列の出現数変化を見る |
| **X3** | **テストが実装の写し**: 期待文字列を実装と同じ定数・関数から取っている(両方一緒に動くので退行を検出できない) | テスト内の期待値が**リテラル**か。`use ...::TEXT` や `reason.text()` を期待値に使っていたら写し |
| **X4** | **分岐の取りこぼし**: B6(拒否でない)や B1 の空ケースが値から落ちている | §2 T1・T3 |
| **X5** | **静かな副作用の移動**: `FILE_TX_BUSY` / `DRAG_TX` / `DRAG_ENTRIES_CACHE` の順序が変わる | `git show` で副作用行の前後関係を追う(§2 T2) |
| **X6** | **死んだ値**: 値を作ったが誰も読まない(`let _ = verdict;`)、あるいは `if` の条件が以前と同値でない | 値の読み手を `grep` で示す。読み手ゼロなら「配線されていない」= 条件付き |
| **X7** | **文言の二重管理**: 同じリテラルが `common` と `mac` の両方に残る(設計 I1 のリスク欄) | `multiset` の出現数が増えていないか。`filetext` で同一テキストが2ファイルに出ていないか |
| **X8** | **I2 固有: 予報の嘘**。`refusal` が入っているのに `peer_lazy` を優先して `WillPlace` を返す(「今離すと置けます」と言いながら経路が拒否されている) | `ForecastInput{ refusal: Some(..), peer_lazy: true }` の優先順位を表テストで確認。設計 §4 I2 (d) は `peer_lazy` 優先と読めるが、**拒否が確定している入力を覆してはならない** |
| **X9** | **I2 固有: 0除算と過大な ETA**。`total == 0`、`sent > total`、サンプル1点で ETA を出す | 表テスト (a) の否定形。`total==0` で `percent`/`eta` を出したら不合格 |
| **X10** | **I3 固有: tap スレッドでの UI 生成**。`NSPanel` を tap から触ると落ちる/出ない | `hud::` の呼び出しが `spawn`/タイマー投入のみか。`incoming_drag.rs:237-300` の作法と同型か |

---

## 4. 増分別の重点(第2段階で使う)

### I1(掴み判定を理由つきの値にする)

- 実行: C1〜C9 の全部。加えて X1・X2・X3・X4・X5・X6・X7。
- 合格条件(私の判定基準。設計 §4 I1 の要求をそのまま使う):
  1. `cargo test` の passed が申告どおり(基準 548 + 新規)。failed 0。
  2. `multiset` の差分に**削除・置換が無い**こと(追加は §1.4 の規則で説明可能なもののみ)。
  3. B1〜B8 の**8分岐すべて**が値を通る。B6 が拒否になっていない(§2 T1)。
  4. 判定が `crates/common` の純関数で、`crates/mac` 側に「どの拒否か」の判断が残っていない。
  5. 表テストの期待文言が**リテラル**で、`a3f9512` の文言とバイト一致(X3)。
- 特に見る: B4 の `sleep` が common に移っていないか(T4)。B8 の `swap` の順序(T2)。
  `Refusal` の粒度が B1 の空ケースを潰していないか(T3)。

### I2(離す前の予報を1つの値にする)

- 実行: C1〜C9 + X8・X9(最重要)・X6。
- 合格条件: 設計 §4 I2 の表テスト (a)〜(e) が**リテラル期待値**で存在し、
  `Unknown` を返す条件が明示されている。`WillPlace` を返す入力に拒否が混ざっていない(X8)。
- 未検証のまま残る点(先に書いておく): 「入力の出所が両側で揃わないと嘘の予報になる」(設計 I2 リスク)。
  common のテストは入力 struct を直接作るので、**mac 側がその struct を正しく埋めているか**は別問題。
  `peer_lazy`・`refusal` を渡す mac 側の配線を `git show` で追い、渡し忘れ(`Default`)が無いかを見る。

### I3(拒否理由をカーソル近傍に出す)

- 実行: C1〜C9 + X10・X5。加えて「HUD が通知と**同じ値**から作られているか」を確認する
  (HUD と通知で別文言になったら X1 の変種)。
- 合格条件: 設計 §4 I3 の `place` の表テスト(4隅・負座標・パネルが画面より大きい)。
  HUD 生成がメインスレッドのタイマー経由。**通知は併用で残っている**(設計 I3 リスク欄)。
- 目視は私には実行できない(コードとテストのみ)。**「目視確認は未実施」と明記する。**

---

## 5. §6「壊してはいけない既存の良さ」の当て方(16項目)

| # | 対応する増分 | 私が機械的に見る手段 |
|---|---|---|
| 1 掴み判定の条件を変えない | I1 | `file_drag.rs` の diff が**ゼロ**であること(ファイル単位で確認) |
| 2 押下を待たせない | I3・I4 | tap から呼ばれる新規処理が `spawn`/タイマーのみ(X10) |
| 3 重い I/O を tap の外へ | I2・I4 | 新規コードに `read_dir`/`metadata`/`symlink_metadata` が無いか `grep` |
| 4 越境前の経路確認と猶予窓 | I1 | `drag_offer_link_ok` の本体 diff ゼロ、既存テスト(`files.rs:854-895`)が緑 |
| 5 スキップを名前つきで通知 | I1・I5 | B6 が `Some` のまま(T1)。`skip_note` の生成が無変更 |
| 6 注意時は必ず通知 | I5 | `notice` の `kind` に「必ず出す」区分があるか |
| 7 中断時の「保存済み N 件」 | I5 | `saved_count` の引き継ぎ(表テスト) |
| 8 中止項目の位置と文言 | I6 | `gui.rs:2388` の文言(L17)と挿入位置の diff |
| 9 進捗%の契約 | I4 | `xfer_title`/`xfer_line` の既存テスト(`files.rs:814-852`)が無変更で緑 |
| 10 バッチ整合(件数一致) | I8 | `drag.rs:26-64` と `:246-296` のテストが無変更で緑 |
| 11 終端の片付け | I8・I9 | 一時ファイル削除が終端で「一度だけ」(表テスト) |
| 12 `TxGuard` | I1・I8 | `TxGuard` を通らない新経路が無いか `grep TxGuard` |
| 13 終了時確認 | I6 | L18・L19 の文言 diff |
| 14 1GiB 以上の注意 | I1 | 閾値 `1<<30` と L1 の関係が無変更 |
| 15 一時ファイル作法 | I8・I9 | `.knit-` の書式が増えていないか `grep '\.knit-'` |
| 16 クリップボードを勝手に書き換えない | I5 | `set_clipboard` 系の呼び出しが I5 で増えていないか |

---

## 6. 判定の記録(増分ごとに追記)

### 記録の様式

```
### I<n>: <一言> / 判定: 合格 | 条件付き | 不合格
- 検証 rev: <short hash>(申告) / 実測 `git rev-parse`: <hash>
- ビルド: `cargo build` exit=<n>(出力末尾 5 行)
- テスト: `test result: ok. <N> passed; 0 failed`(申告 <M> と [一致/不一致])
- 文言: `diff multiset_a3f9512 multiset_worktree` = <差分行数 / 内容>
- 8分岐: B1..B8 = [値を通った / 素通り / 対象外]
- §6: 影響項目 = [番号](退行の有無)
- 見つけた欠陥: <file:line + 再現手順 + 生出力>
- 未検証の点: <目視・実機・crates/win など>
```

### I1: 掴み判定を理由つきの値にする / 判定: **合格**(軽微な欠陥3件・挙動の退行は検出できず)

検証日: 2026-10-18 / 検証 rev: **`43de2e2`**(HEAD、申告どおり)

**再検証(計器の走査範囲を統合テストへ拡張した後)**: `H_multiset` = `3e26aae0…085f6c9`。
拡張スキャンでの `a3f9512 → 43de2e2` の差分は、production 側が **L6 のリテラル分割1件のみ**で、
残りは `crates/common/tests/drag_handoff.rs` に足されたテスト期待値(7理由の本文)と assert メッセージ。
**判定(合格)に変更はない。**

**C1 対象の確定**

```
$ git rev-parse --short HEAD            -> 43de2e2
$ git status --porcelain                -> (空。作業ツリーはコミットと一致)
$ git diff --stat a3f9512 43de2e2
 crates/common/src/drag.rs                | 120 ++++++++
 crates/common/tests/drag_handoff.rs      |  33 +++
 crates/mac/src/files.rs                  | 165 ++++++-----
 docs/plans/2026-10-18-knit-dnd-verify.md | 471 +++++++++++++++++++++++
```
製品コードは3ファイルのみ。**`tap.rs`・`file_drag.rs`・`gui.rs`・`incoming_drag.rs`・`bulk.rs`・`proto.rs` は無変更**
(§6-1 掴み判定の条件 / §6-8 中止項目 / §6-9 進捗の契約 / §6-15 一時ファイル作法 が構造的に守られている)。

**C2 ビルド / C3 テスト**(自分で実行)

```
$ cargo build > /tmp/verify_build_i1.log 2>&1; echo $?      -> 0
$ grep -c '^warning' /tmp/verify_build_i1.log               -> 10(基準 10 と同数)
$ cargo test  > /tmp/verify_test_i1.log  2>&1; echo $?      -> 0
$ grep -E "^test result" ... | awk '{p+=$4;f+=$6;i+=$8}'
passed_total=549 failed_total=0 ignored_total=4 binaries=11
```
基準 548 / 0 / 4 → **+1 passed、failed 0、ignored 4(増加なし)**。申告(549)と一致。

**C4 文言**(golden list の機械照合)

```
$ python3 /tmp/dndverify/golden.py a3f9512 | grep H_multiset -> 8ecdd626…670a3
$ python3 /tmp/dndverify/golden.py 43de2e2 | grep H_multiset -> dcd9eba1…536af
$ diff multiset_a3f9512.txt multiset_43de2e2.txt
1814c1814
< 掴んだまま渡せるのは通常のファイルとフォルダだけです{skip_note}
---
> 掴んだまま渡せるのは通常のファイルとフォルダだけです
```
**差分は1件のみ。** これは §1.4 の「例外」に該当する(下の「文言の同一性の証明」)。
`filetext` の差分は **8件の files.rs → drag.rs 移動のみ**(files.rs 70→62 / drag.rs 12→20)。
**同一テキストが両ファイルに残る「二重管理」は無い**(X7 ✓)。

**文言の同一性の証明(推測ではなく式の引用)**

| | 式 |
|---|---|
| 基準 `a3f9512`(`files.rs:388`) | `&format!("掴んだまま渡せるのは通常のファイルとフォルダだけです{skip_note}")` |
| I1 `43de2e2`(`files.rs:404`) | `&format!("{}{skip_note}", drag::SKIPPED_NOTICE)` |
| 定数(`drag.rs:58`) | `pub const SKIPPED_NOTICE: &str = "掴んだまま渡せるのは通常のファイルとフォルダだけです";` |

Rust の inline format args は書式指定が無ければ `Display`、`{}` と `{skip_note}` の間に区切り文字は無い。
したがって描画結果は `SKIPPED_NOTICE ++ skip_note` の**バイト一致**で、基準と同じ文字列になる。

**C5 差分の読解**(値と表示の分離)

- 判定は `crates/common/src/drag.rs:101 verdict_for(&Signals) -> Verdict` の**純関数のみ**。
  分岐順は基準と同じ「件数 → 経路 → 集計(古い/待ち切れ/失敗/空) → 送信枠」。
- 文言は `Refusal::notice_body()`(`drag.rs:32`)に**1箇所だけ**。mac 側は値を受けて
  ログと通知を出すだけ(`files.rs:299 refuse_drag`)。**同じ理由が2箇所で別文言になる経路は無い**(X1・X2 ✓)。
- 副作用(送信枠の予約)は値ではなく従来どおり `swap`(`files.rs:428`)。判定は `load`(`drag.rs:118` の `s.busy`)。
- 集計待ちの `sleep` は mac 側に残っている(`files.rs:388`、ループは `:351`。基準 `:349-351`)。**B4 の懸念(T4)は解消**。

**C8 8分岐の追跡(すべて値を通った)**

| # | 基準での条件 | I1 での値 | 本文/ログ | 判定 |
|---|---|---|---|---|
| B1 | 0件 or >64 | `Refusal::TooMany`(`drag.rs:13,35`) | L1 / ログなし(基準どおり) | 値 ✓ |
| B2 | 経路未接続・猶予窓外 | `Refusal::LinkDown` | L2 / 基準どおりのログ | 値 ✓ |
| B3 | 集計条件が古い | `Collect::Stale` → `Refusal::CollectStale` | L3 / 同 | 値 ✓ |
| B4 | 集計が間に合わない | ループ中 `Collect::Pending` → **`Verdict::Wait`**(断らない)、ループ後 `Expired` → `Refusal::CollectPending` | L4 / 同 | 値 ✓ |
| B5 | `collect` が `Err` | `Collect::Failed{label}` + `raw` → `Refusal::CollectFailed` | L5(引数は `bulk::send_error_label`) / ログは**基準どおり生のエラー**(`refusal_log` の `raw`) | 値 ✓ |
| **B6** | スキップ警告 | **値にしない**(`SKIPPED_NOTICE` の定数のみ)。`files.rs:399-406` で通知後、**そのまま越境して `Some(id)` を返す** | L6 | **拒否になっていない ✓(挙動退行なし)** |
| B7 | 送れる物が無い | `Collect::Done{0}` → `Refusal::NothingToSend`(`drag.rs:116`) | L7 / 同 | 値 ✓ |
| B8 | 前の転送中 | `busy` → `Refusal::Busy`、予約は `swap`(`files.rs:428`)。`load` 後に他スレッドが取った場合も同じ理由で断る | L8 / 同 | 値 ✓ |

**通知とログの数が差分の説明と厳密に一致**(取りこぼし・二重通知が無いことの構造的な裏取り):

```
notify(  files.rs 全体:  a3f9512 = 21  ->  43de2e2 = 15   (-6)
    内訳: 基準の関数内 8箇所(L1..L8) -> 新: refuse_drag 1 + スキップ 1 = 2。8-2 = -6 ✓
eprintln!(files.rs 全体: a3f9512 = 27  ->  43de2e2 = 22   (-5)
    内訳: 基準の関数内 7箇所(拒否6+猶予窓1) -> 新: refusal_log の eprintln 1 + 猶予窓 1 = 2。7-2 = -5 ✓
#[test] files.rs = 7 / drag.rs = 10 / drag_handoff.rs = 5 -> 6(既存テストの削除・骨抜きなし)
```

**C9 §6(影響する項目のみ)**

| # | 項目 | 結果 |
|---|---|---|
| 1 | 掴み誤爆の条件 | `file_drag.rs` 無変更 ✓ |
| 4 | 経路確認と10秒の猶予窓 | `drag_offer_link_ok` 本体・既存テストとも無変更 ✓ |
| 5 | スキップを名前つきで通知 | B6 が `Some` のまま ✓(`skip_note` 関数も無変更) |
| 9 | 進捗%の契約 | `files.rs:814-852` の既存テスト無変更で緑 ✓ |
| 12 | `TxGuard` | 新経路なし(無変更) ✓ |
| 14 | 1GiB の注意 | 無変更 ✓ |

**反例探索の結果**

| # | 探した反例 | 結果 |
|---|---|---|
| X1 | 理由と表示の食い違い | 7理由すべて 1:1。表テストが**リテラル期待値**で固定(`drag_handoff.rs:149-176`)✓ |
| X2 | 同じ理由が2箇所で別文言 | 8文字列が drag.rs へ移動し、files.rs に残存なし(filetext diff)✓ |
| X3 | テストが実装の写し | `assert_eq!(reason.notice_body(), *body)` の右辺は**リテラル** ✓(写しではない) |
| X4 | 分岐の取りこぼし | B1〜B8 すべて値または定数を通る。B6 は非拒否のまま ✓ |
| X5 | 副作用の順序 | `take()` → sleep → `swap` の順序・回数は基準どおり。`swap` が予約を担う点も維持 ✓ |
| X6 | 死んだ値 | `Verdict`/`Refusal` は全て読まれる。新規コードに `unwrap`/`panic!` 経路なし ✓ |
| X7 | 文言の二重管理 | multiset の出現数に増加なし ✓ |

**見つけた欠陥(いずれも挙動の退行ではない。重要度順)**

1. **[軽微] 網羅 assert が手書き配列に依存している。** `drag_handoff.rs:169-172` の
   `let all = [...]` は手で並べた7理由で、`Refusal` に**新しいバリアントを足してもテストは落ちない**
   (コメント「理由を足したら表も足す」が唯一の防波堤)。網羅はコンパイラに守らせるべき:
   テスト側で `match r { Refusal::TooMany => .., ... }` の**網羅 match** を書けば、新バリアント追加時に
   コンパイルエラーになる。再現: `Refusal` に `pub enum Refusal { …, Foo }` を足して
   `cargo test -p knit-common refusal_reason_covers_every_reject_branch` → **緑のまま**(落ちない)。
2. **[軽微] スキップ通知の「描画」を固定するテストが無い。** テストは
   `assert_eq!(SKIPPED_NOTICE, "…だけです")`(定数)だけを固定しており、
   `format!("{}{skip_note}", SKIPPED_NOTICE)`(連結)は固定していない。
   仮に `format!("{SKIPPED_NOTICE} {skip_note}")` のように区切りを足しても**テストは緑のまま**で、
   ユーザーに見える文だけが1文字変わる(そして multiset の差分にすら現れない=検出できない)。
   → 今回の「描画は同一」という判定は**コード読解に依存**している(私の証明は §C4 のとおりだが、
   回帰を止める仕組みは無い)。提案: files.rs 側ではなく common 側に
   `pub fn skipped_notice(skip_note: &str) -> String` を置き、`assert_eq!(skipped_notice(" ※…"), "…だけです ※…")`
   のように**連結結果**を固定する。
3. **[軽微] 命名と意味の食い違い: `Refusal::CollectPending` は「待ち切れ(Expired)」を意味する。**
   `Collect::Pending`(まだ集計中 → `Verdict::Wait` で断らない)と名前が衝突しており、
   `notice_body` の文言も「まだ終わっていません」なので、読み手は「集計中」と誤読する。
   I3 の HUD がこの値を読むとき、`Wait`(断らない)と `CollectPending`(断る)を取り違える事故が起きやすい。
   → `Refusal::CollectTimeout` などへの改名を推奨(文言は変えずに済む)。
4. **[観察] `Verdict::Ready` の整合チェック(`files.rs:419-423`)は到達不能。**
   `count` は同じ `cached` から作られるため食い違いは起こり得ず、この分岐はテストで叩けない。
   「無言にしない」という要求は満たしているが、守っている不変条件は **mac 側だけ**にある
   (`verdict_for` は `Signals` に実体が無いので `Ready{count}` の正しさを検証できない)。
5. **[I3 への持ち越し] `Refusal::TooMany` は「0件」も同じ理由・同じ文言で扱う**(基準の挙動を維持)。
   I3 でカーソル近傍に理由を出すと、**0件の掴みに対して「64件まで」と表示される**経路が表面化する。
   I1 では文言を変えてはいけないので正しい判断だが、I3 の受入(AC3「拒否の理由が読める」)では
   理由の粒度が問われる。I3 の検証時に必ず突く。

**未検証の点**

- `refusal_log`(mac 私有関数)と `refuse_drag` の**ログ行そのもの**はユニットテストで固定されていない。
  私の確認は `git show` の読解と `notify`/`eprintln!` の出現数照合までで、実行時のログ出力は見ていない。
- 目視・実機の確認はしていない(通知の実表示、HUD は I3 以降)。
- `crates/win` は対象外。

### a420d11: I1 の指摘3件の修正 / 判定: **合格**

検証日: 2026-10-18 / rev **`a420d11`**

- ビルド・テスト: `cargo test` = **549 passed / 0 failed / 4 ignored**(43de2e2 と同数。追記のみでテスト件数は増えない)
- 文言: `diff multiset_43de2e2 multiset_a420d11` = **追加2件のみ**、いずれも統合テスト内の文字列
  (` ※読めない・リンク等の 2 件をスキップ: a.txt ほか1件` と、その連結結果)。**production の文言は不変**。
- **指摘1(網羅 assert)を変異で実証**: `Refusal` に `MutantProbe` を足すと
  `error[E0004]: non-exhaustive patterns: &Refusal::MutantProbe not covered --> crates/common/src/drag.rs:36:15`
  でコンパイルが止まる(exit 101)。
  **ただし効いているのは `notice_body` の match であり、テスト側の網羅 match ではない**
  (lib がコンパイルできないので統合テストの match まで到達しない)。
  テスト側の match は「`notice_body` に将来ワイルドカードが入った場合」の二重の網として価値がある、が正確な整理。
- **指摘2(連結の固定)を変異で実証**: `skipped_notice` を `format!("{SKIPPED_NOTICE} {skip_note}")`(空白1文字)に
  変えると `refusal_reason_covers_every_reject_branch` が赤(exit 101、`assertion left == right failed`)。
  連結が common の1箇所に集約され、**私の golden list では検出できない種類の退行が塞がれた**。
- **指摘3(改名)**: `grep -rn CollectPending crates/` = **0件**。`Collect::Pending` は残り、
  doc コメントで「`Verdict::Wait` とは別物」と明記されている ✓

### I2: 「今離すとどうなるか」の予報を値にする / 判定: **合格**(テスト欠落2件・doc 1件の欠陥。挙動は正しい)

検証日: 2026-10-18 / rev **`da6129c`**

- 差分: `crates/common/src/drag/forecast.rs`(新規 301行)と `drag.rs`(+4:
  `pub mod forecast;` と**明示的な**再エクスポート `pub use forecast::{forecast_for, CarryPhase, FailReason, Forecast, ForecastInput, Why};`)。
  **consumer はまだ無い**(mac の配線は設計どおり I4)。値だけを先に置く増分。
- `cargo build` exit 0(警告行 10 = 基準と同数)/ `cargo test` = **556 passed / 0 failed / 4 ignored**(549+7、申告どおり)
- 文言: `diff multiset_a420d11 multiset_da6129c` = **追加4件のみ**、すべて `forecast.rs` の module テスト内
  (`panic!`/assert メッセージ)。**production のユーザー可視文言は1文字も増減していない**。
  (`H_multiset`: a420d11 `381e462e…8f46a6` → da6129c `3fd54fd4…309d1f`)
- 設計 §4 I2 の検証項目 (a)〜(e) はすべて表テストで固定 ✓
  (a) `total==0` → ETA を出さない / (b) 期限超過 → `WillFail{PeerTimeout}` / (c) 完了前の離し → `WillSaveOnly` /
  (d) `peer_lazy` → `WillPlace` 側 / (e) 材料不足 → `Unknown`。出力の型も設計より精緻(`WillFail{reason: FailReason}`)。
- 表テストの置き場所が設計の想定(統合テスト)と違い module 内だが、`cargo test -p knit-common` で走る点は同じ。
- **行数の目安 140 に対し実測 301 行**(`#[cfg(test)]` は `:140` なのでテスト約161行)。
  設計 §4 前文の「150行を超える場合は増分を分割」に字面上は抵触する(判定には影響させないが記録する)。

**lead の重点4点への回答**

1. **`WillPlace` で「実際には置けないのに置ける」と言う経路** — 実装の順序は正しいが、穴が2つある。
   - `peer_lazy == true` → `WillPlace`(`forecast.rs:116-118`)は**能力**だけを見ており、
     **今回の遅延提供が受理されたか**を見ていない(`ForecastInput` に受理状態が無い)。
     `peer_lazy` は I7 の版ゲートで決まる能力フラグなので「相手が lazy 対応」=「今この掴みを置ける」ではない。
     現在は `peer_lazy=false` が既定なので実害は無いが、I7/I8 以降で I8 の `Accepted` を入力に足す必要がある。
   - **「離した後に届き終えた」場合の順序がテストで固定されていない**(下の欠陥1)。
2. **`WillSaveOnly{dir: None}`** — 型は `Option<String>` で表現できる ✓。ただし (a) consumer がまだ無い、
   (b) テストは `dir: Some("Downloads/Knit")` の1ケースのみで **`None` の経路が未固定**(欠陥2)。
   I5 のコンポーザへ「保存先が不明」を表現できることの申し送り。
3. **期限の境界** — `elapsed_ms > peer_deadline_ms` で期限切れ、`==` は待てる ✓
   (`forecast.rs:127`)。摩擦doc `:51` の引用(現行 Windows は `!p.sending && p.began.elapsed() > 30s` で取消)と
   **同じ「超えたら」**で整合 ✓。テストも両側(30_001 → `WillFail` / 30_000 → `WaitingForPeer`)で固定 ✓。
   ただし **`elapsed_ms` の起点は呼び出し側が `began` に合わせる必要がある**(I4 の申し送り。I2 では検証不能)。
   また「完了を期限より先に見る」判断(`forecast.rs:124-126`)は、摩擦doc の `!p.sending` 条件が
   **完了後も成立し得る**ため、「完了したから置ける」が偽り得る余地がある。crates/win を読めないので
   **未検証の前提**として残す(設計 §9-6 と同じ扱い)。
4. **変異テスト**(私の手で実施。`/tmp` の detached worktree = `git worktree add --detach` で行い、
   本体の作業ツリーは汚していない。最後に `git worktree remove` と `git status --porcelain` が空を確認):

| 変異 | 内容 | 結果 |
|---|---|---|
| m1 | `eta_ms` の `total <= sent` → `total < sent`(lead 例) | **生存**(258 passed) |
| m2 | `speed_samples < 2` → `< 1` | 赤(`no_eta_without_two_speed_samples_or_a_known_total`) |
| m3 | `ReleasedEarly` 分岐を削除(lead 例) | 赤(`releasing_before_completion_saves_instead_of_placing`) |
| m4 | 拒否の早期 return を削除 | 赤(`a_refusal_beats_even_lazy_delivery`)= X8 の防御が実効 |
| m5 | 期限 `>` → `>=` | 赤(`a_deadline_that_has_passed_means_the_peer_gives_up`) |
| m6 | 完了判定を `if false` で無効化 | 赤(3件) |
| m7 | `Unknown` の理由 `TotalUnknown` → `NoTransferStarted` | 赤(X9 の理由の区別が実効) |
| **m8** | `ReleasedEarly && sent < total` に条件を狭める(私の追加) | **生存**(258 passed)← **欠陥1** |
| m9 | 完了の境界 `>=` → `>`(私の追加) | 赤(2件) |
| m10 | `Refusal` にバリアント追加(指摘1の実効性。**a420d11 で実施**) | コンパイルエラー(exit 101)✓ |
| m11 | `skipped_notice` に空白を1文字追加(指摘2の実効性。**a420d11 で実施**) | 赤 ✓ |

   **m1 が生存したのは「テストの盲点」ではなく「到達不能なガード」(等価変異)**。
   `eta_ms` は最後の `WaitingForPeer` からしか呼ばれず、そこへ来るのは `sent < total` のときだけなので
   `total <= sent` は決して真にならない(冗長なガード。削るか「到達しない」とコメントするのが良い)。

**見つけた欠陥**

1. **[テスト欠落・重要] 「完了前に離した後で、転送が届き終えた」入力が未固定。**
   実装は `ReleasedEarly` を `sent >= total` より先に見るので**正しい**
   (保存のみだったものを「置けた」と言わない)。しかしこの順序を崩しても
   (条件を `ReleasedEarly && sent < total` に狭めても) **全テストが緑のまま** = m8 が生存。
   現実の流れでは**必ずこの入力が来る**: ユーザーが 50% で離す → Mac は送信を続ける → `sent >= total` になる。
   ここで順序が崩れると **F5 の嘘(保存のみを「置けます」)が無言で再発**する。
   再現: `ForecastInput { phase: ReleasedEarly, peer_lazy: false, sent: 10_000, total: 10_000, .. }` を
   テストに足す(期待は `WillSaveOnly`)。現状このケースが無い。
2. **[テスト欠落・軽微] `WillSaveOnly{dir: None}` が未固定**(重点2)。`save_dir: None` のとき
   表示側が「保存先が分からない」と言えることを保証するテストが無い。
3. **[doc/型・軽微] `deadline_ms` のコメントが表現と矛盾。** `forecast.rs:49` は
   `/// 相手が待つのをやめるまでの残り(0 = 期限が不明)` だが、型は `Option<u64>` で
   **不明は `None`**、`Some(0)` は「期限ちょうど」を意味する(実装 `:135-136` も不明に `None` を返す)。
   同じ「0 = 不明」という言い回しが `ForecastInput::peer_deadline_ms`(`:69`、u64 の番兵)にも使われており、
   I4 の実装者が `unwrap_or(0)` で両者を混同し得る。→「不明は None(0 ではない)」と書き分けるべき。
4. **[観察] `WaitingForPeer{deadline_ms: None}`(`peer_deadline_ms == 0`)の経路が未テスト。**
5. **[観察・I4 への申し送り] ETA と残り期限の突き合わせが値に無い。** 残り期限 5 秒・ETA 9 秒のとき、
   値としては「あと9秒」と「5秒で相手が諦める」が並ぶだけで、**HUD が「あと9秒」とだけ言えば嘘になる**。
   材料は揃っているので表示層で比較できるが、値の層に「間に合わない」という表現が無い。
6. **[観察] `speed_samples` は呼び出し側の自己申告**で、値の層では検証できない(I4 の配線時に確認する)。
7. **[観察] `eta_ms` の `saturating_mul` は桁溢れで黙って巨大な値を返す**(panic はしない)。
   `percent` の `.min(100)` と同様、実入力では到達しない防御。新規コードに `unwrap`/`panic!` 経路は無い。

**未検証の点**

- mac 側の配線が無いので、**実入力での予報の正しさは検証できない**(I4 の検証項目に送る)。
- `crates/win` は対象外。したがって「完了後も 30 秒取消が成立し得るか」は未検証の前提のまま。
- 目視・実機は未実施。

### 53c6291: I2 の m8 穴の修正 + 期限不明の固定 / 判定: **合格**

検証日: 2026-10-18 / rev **`53c6291`**

- 差分: `crates/common/src/drag/forecast.rs`(+62: テスト2件と doc)と doc(+142 = 私の I2 記録のコミット分)。
- テスト: `cargo test` = **558 passed / 0 failed / 4 ignored**(556+2、申告どおり)。
  HEAD `85fdecb` では 563 = 558 + `notice.rs` の5件 ✓ 連続性も一致。
- 文言: `diff multiset_da6129c multiset_53c6291` = **追加5件のみ**、すべて `forecast.rs` のテスト内の
  assert/panic メッセージ(`% は総量から出せる` / `完了前に離したら、後から届き終えても置けてはいない` /
  `期限が不明なら数字を出さない` / `未完の押下中は待ち`(2箇所目) / `残り時間も総量と速度から出せる`)。
  **production の文言は不変** ✓
- **m8 の塞ぎを私の手で再確認(lead の依頼)**: `if i.phase == CarryPhase::ReleasedEarly {` を
  `... && i.sent < i.total {` に狭めると
  `drag::forecast::tests::releasing_early_stays_save_only_even_after_the_bytes_arrive` が赤
  (exit 101、228 passed / 1 failed)。復元後は緑(**260 passed** = 265 − notice.rs の5件)✓ **穴は塞がった**。
- 新テストの品質: 期待値はリテラル。`ReleasedAfterComplete` 側は `..finished_after_release.clone()` で
  **`phase` の1点だけを変えて**比較しており、「差が離した順序で決まる」ことを正しく固定している(写しではない)✓
- doc 修正の確認: `deadline_ms` は「None = 期限が不明。Some(0) は期限ちょうど」に訂正され ✓、
  申し送り3件(`peer_lazy` は能力 / ETA と期限の比較は表示側 / `speed_samples` は自己申告)が
  module doc に入った ✓ = **私の I2 指摘 5・6 と `peer_lazy` の穴が文書化で吸収された**。
- 残る観察: m1(`total <= sent` → `total < sent`)は**引き続き等価変異**(条件自体は変えていないので
  I2 検証時の結論がそのまま成り立つ)。到達不能である旨がコメント化され、意図が読めるようになった ✓。
  `ReleasedAfterComplete && sent < total` という矛盾入力は依然未固定(到達しない前提の入力なので欠陥としない)。

### I5: 通知コンポーザ / 判定: **条件付き**(構造と SavedOnly は良い。`Placed` の言い切りが観測を超えている)

検証日: 2026-10-18 / rev **`85fdecb`**

- 差分: `common/src/notice.rs`(新規 231)・`common/src/lib.rs`(+1)・`mac/src/files.rs`(+61)・`mac/src/tap.rs`(+3)。
- `cargo build` exit 0(警告行 10 = 基準同数)/ `cargo test` = **563 passed / 0 failed / 4 ignored**(558+5、申告どおり)。
- 設計 §4 I5 の検証方法 (a)(b)(c) は満たす: (a) `SavedOnly` の本文に成功語なし(否定 assert)✓
  (b) 維持すべき要素(保存先・理由・次の一手)✓ (c) `Failed{retryable:false}` は再試行を促さない ✓。
  `NoticeKind::{Always, Suppressible}` で「注意があるときは必ず出す」を型化 ✓(§6-6 維持)。

**重点5: 文言の機械照合(私の golden list で独立に)**

| 旧(〜`53c6291`) | 新(`85fdecb`) | 判定 |
|---|---|---|
| `{peer} へファイルを渡せませんでした({}。接続を確認してもう一度掴んでください)`(`files.rs`、引数は `bulk::send_error_label(e)`) | `{peer} へファイルを渡せませんでした({reason}。接続を確認してもう一度掴んでください){saved}`(`notice.rs:117`、`reason` は同じ値。掴み経路は `saved_count: 0` なので `{saved}` は空文字) | **描画はバイト同一** ✓(`{}`→`{reason}` は名前付き引数への変更で、値も位置も同じ。§1.4 の「例外」に該当) |
| — | `{peer} へファイルを渡せませんでした({reason}){saved}` | **宣言済みの意図的変更**(retryable=false で「接続を確認してもう一度」を落とす)✓ |
| — | `(保存済み {saved_count} 件)` | 新情報。**掴み経路は `saved_count: 0` 固定なので production では未到達**(観察5) |
| — | `{count} 件を {peer} へ渡しました(相手の画面に置けました)` / `{count} 件は {peer} の受信フォルダに保存されています({why})。…` | **新規**(3状態化。旧は成功時に無言 = C1) |
| `ファイル転送を中止しました` | 同じ(composer を通さない `files.rs:546`) | 不変 ✓ |

golden list 上は「置換1・追加多数」として現れ、**すべて説明可能**(宣言済み or 新規)。
`H_multiset`: `53c6291` = `88ab6ef5…4b837` → `85fdecb` = `85b0dddc…892b3`。

**重点1: `DRAG_RELEASED_EARLY` の漏れ・誤リセット** — **問題なし**(ただし別の穴=欠陥2)

- フラグを読むのは掴み経路の完了時だけ(`files.rs:522`)。通常送信(メニュー/⌘C)と Win→Mac は**読まない** ✓
  → 「フラグが立ったまま次の通常送信で保存のみと言う」経路は無い ✓
- クリアは `offer_drag_to_win` の1箇所(`files.rs:434`)で、`send_drag_files_to_win` の唯一の呼び出し元
  (`tap.rs:2218`、offer の直後)より前 ✓
- **誤リセットの危険**(転送Aの進行中に新しい掴みBが offer すると A の記録が消える)→ **起きない**:
  B は `FILE_TX_BUSY.swap`(`files.rs:427`)で Busy 拒否され、**クリア(`:434`)より前に return** する
  (順序: swap `:427` → refuse `:428` → id `:432` → clear `:434`)✓
- 通常送信の進行中に離しても `x.id & DRAG_ID_BIT != 0` で弾かれる ✓

**重点2: `Placed` と言ってよい条件** — **言い切れない(条件付きの主因)**

観測できる事実は「**自分の送信が終わった時点で、自分のマウスボタンがまだ押されていた**」だけ
(`files.rs:513-538` と `:758-767`)。ここから「相手の画面に置けました」は2段の推測になる:
(a) 相手が OLE を始めたかは `crates/win` が自分の claim 状態から決める(F3/C4)ので Mac には見えない、
(b) 始まったとしても、そのあと**置ける場所で離したか**は Mac には分からない(デスクトップで離す・Esc・§5-W2 の30秒取消)。
しかも通知は `Ok` 分岐で**即座**に出る。この経路の前提は「ユーザーが押したまま転送が進む」(設計 §0 前提1)なので、
**離す前に「置けました」と言う**ことになる。設計自身が §9-7 で「F5 は Mac 単独では消えない。Mac が結果を
知るには W4 が要る」と書いており、§4 I5 の `Placed{dir}` は**ドロップ先**を知っている前提の型である
(実装が `dir` を持たないのは正直だが、文言だけが残った)。
→ **条件**: `Placed` の本文を観測事実に合わせる(例「◯件を Windows PC へ渡しました」)か、
W4 の `DragDone{copied}` を受けてから「置けました」と言う。**この1点で合格に上げられる。**

**重点3: `SUCCESS_WORDS` を production に置いた是非** — 置き場所より**網羅性**が問題

- 変異 a3(`SavedOnly` の本文に「(置けました)」を追記)が**生存**(265 passed)。
  `SUCCESS_WORDS` = `["置きました","送信しました","渡しました","完了しました"]` は、
  **`Placed` が実際に使う「置けました」を含んでいない** = 塞ぎたい嘘の語を1つすり抜ける。
- 提案(再設計しない範囲): (1) `"置けました"`/`"置けた"`/`"成功"` を足す。
  (2) より良い形: 黒名单をやめ、**成功側の語から導出する** — `Placed` の本文を組み立てる語を
  `const PLACED_PHRASES: &[&str]` として1箇所に置き(`Placed` もそれを使う)、テストは
  「`SavedOnly` の本文が `PLACED_PHRASES` のどれも含まない」を assert する。語彙が実装と自動で一致する。
- `pub const` を production に置くこと自体は「この語は禁止」という契約の表明なので許容範囲。
  隠すなら `#[cfg(test)]`(同一 module 内テストなので可能)だが、統合テストからは見えなくなる。

**重点4: 変異**(`/tmp` の detached worktree。本体の作業ツリーは汚していない)

| 変異 | 内容 | 結果 |
|---|---|---|
| base | `knit-common` 265 passed / `knit-mac` 298 passed | 緑 |
| a1 | `SavedOnly` の本文を `Placed` の本文に差し替え | **赤**(`saved_only_never_uses_success_words`)✓ |
| a2 | 「(保存済み N 件)」を落とす | **赤**(`failed_asks_for_a_retry_only_when_it_can_help`)✓ |
| **a3** | `SavedOnly` の本文に「(置けました)」を追記 | **生存**(265 passed)← **欠陥4** |
| b1 | `retryable_error` の判定を反転(`!matches!`→`matches!`) | **赤**(`retryable_follows_the_error_kind`)✓ |
| **c1** | 掴み経路の判定を `if early` → `if false`(**常に Placed**) | **生存**(298 passed)← **欠陥3** |
| **c2** | 呼び出し側で `retryable` を反転 | **生存**(298 passed)← **欠陥3** |
| **c3** | 新しい掴みでのフラグのリセット(`files.rs:434`)を削除 | **生存**(298 passed)← **欠陥3** |

**X2(tap スレッドで XFER の Mutex を取ること)の評価**

- `note_press_release`(`files.rs:760-767`)は `XFER.lock()` を取る。XFER を保持するのは
  `begin_xfer:801` / `update_xfer:811` / `end_xfer:821` / `cancel_active_xfer:830` /
  `xfer_title:841` / `xfer_line:849` で、**いずれもロック内で I/O や sleep をしない**
  (最長でも `format!`・`to_string()` の確保、`cancel_active_xfer` の `xfer::request` は O(1) の登録)。
  → **現状で入力経路を待たせる退行は無い** ✓
- 残るリスク: tap スレッドが**ブロッキングな `lock()`** を取るため、将来どれかの保持区間が長くなると
  そのまま入力が止まる(§6-2/§6-3)。必要な情報は「掴み転送が進行中か」「未完か」の2つなので、
  `AtomicBool`+`AtomicU64` にすれば **tap 経路から Mutex を消せる**(提案)。

**見つけた欠陥**

1. **[重要・条件付きの主因] `Placed` の言い切りが観測を超えている**(重点2)。通知は転送完了時に出るので
   **ユーザーが離す前に「置けました」と言う**。相手が OLE を始めたか・置けたかは Mac から見えない
   (設計 §9-7 / §5-W4 待ち)。
2. **[重要] 「越境直後・`begin_xfer` 前」に離した場合、記録されない。**
   `note_press_release` は `XFER` が `Some` のときだけ記録する(`files.rs:761-766`)が、`XFER` を立てるのは
   転送スレッドの `begin_xfer`(`:801`)。その前に走るのは `tap.rs:2218` の spawn →
   `mac_files_key()`(ファイルごとの `metadata` 走査)→ `entries_total()` なので、
   **境界を越えてすぐ離す「フリックして離す」操作がこの窓に入り得る**。その場合 `early=false` →
   **相手は保存のみなのに「置けました」と言う**(I5 が潰すはずだった F5 の嘘が残る)。
   修正案: `offer_drag_to_win` で `DRAG_IN_FLIGHT`(AtomicBool)を立て、`note_press_release` は
   「in-flight かつ(XFER が None または未完)」なら記録する。TxGuard の Drop で下ろす。
3. **[重要・テスト構造] I5 の mac 配線に自動テストが1つも無い**(変異 c1/c2/c3 がすべて生存)。
   判定(early → SavedOnly)もリセットも呼び出し側の `retryable` 反転も **CI では検出できない**。
   設計の I5 検証方法は common の表テストしか要求していないので「設計どおり」ではあるが、
   I5 の要である分岐が無防備なのは事実。修正案: 判定を common の純関数へ出す
   (例 `pub fn delivery_result(released_early: bool) -> DeliveryResult`)。配線は1行になり表テストで固定できる。
4. **[中] `SUCCESS_WORDS` が `Placed` の語「置けました」を見落としている**(変異 a3 が生存)。重点3を参照。
5. **[観察・スコープ] 設計 I5 の「Mac 側の各経路は outcome を埋めるだけ」は未完。** 配線されたのは
   掴みドラッグ経路だけで、通常送信(⌘C/メニュー)と受信経路(`files.rs:145-190` / `:557-670`)は今も
   自前の文言を組み立てている。`Failed{saved_count}` も掴み経路では **0 固定**で、§6-7 の
   「保存済み N 件」は受信経路のまま(無変更)。F5/C1 の範囲では正しい判断だが、
   「文言が1箇所でしか作られない」という I5 の狙いは未達。
6. **[観察・UX] 成功通知が「押したまま」の最中に出る。** 手がふさがっている間にバナーが出るため、
   I3 の HUD までの暫定として許容しつつ、I4 で扱う論点として残す。
7. **[観察]** `retryable_error` は `Interrupted` を false にするが、中止は composer を通さない(`:544-546`)ので
   この分岐は到達しない(防御としては妥当)。失敗本文が二重の案内になる癖
   (`send_error_label(NotConnected)` の「接続の回復を待ってから再送」+ retryable の「接続を確認してもう一度」)は
   旧実装と同じで退行ではない。

**未検証の点**

- `crates/win` は対象外。「相手が OLE を始めたか」「30秒取消が完了後に効くか」は未検証の前提。
- 目視・実機は未実施(通知の実表示、掴んだまま離す操作の体感)。
- mac 配線の実行時挙動は**テストでも目視でも確認していない**(欠陥3のとおりテストが存在しない)。

## 6.5 第3ラウンド: `85fdecb` 以降の未検証コミット(まとめて敵対的検証)

検証日: 2026-10-18 / 対象: `c7aae32`〜`6bc0791`(11 コミット)

**検証環境**: HEAD = `6bc0791` / `git status --porcelain` 空 /
`cargo build` exit 0(**警告行 10 = 基準と同数**)/
`cargo test` = **590 passed / 0 failed / 4 ignored, 11 binaries**(前回検証の 563 から +27)。

**文言(golden list、`85fdecb` → `6bc0791`)**: 差分はすべて説明可能だった。
- 意図的変更1件: `{count} 件を {peer} へ渡しました(相手の画面に置けました)` →
  `{count} 件を {peer} へ渡しました(転送完了。相手側で受け取れます)`(= c7aae32 が私の I5 指摘1を修正)
- 新規(I3): `hud_line()` の8文言 + `このまま離すと Mac 内の操作になります` + `[hud] NSPanel を作れません`
- 新規(I6): `直前の受け渡し` / `直前: {line}` / `もう一度送る` / `受信フォルダを開く` /
  `受信フォルダはまだありません(ファイルを受け取ると作られます)` / `受信フォルダを開けませんでした` /
  `もう一度送れるファイルが残っていません` / `前のファイルを転送中です。完了後にもう一度お試しください`
- **`Refusal::EmptyGrab` の追加でも通知文言は不変**(`notice_body()` が `TooMany` に委譲 = 同じ文字列)✓
- 残りはすべてテスト内の assert/panic メッセージ ✓ → **意図しないユーザー可視文言の変更は検出されなかった**

| コミット | 内容 | 判定 |
|---|---|---|
| `c7aae32` | I5 の指摘3件の修正(私の条件付きへの回答) | **合格** |
| `c49f95d` | 粘着バグ修正(`Carried::released_once`) | **条件付き** |
| `1047f61`+`ecf68a7` | Windows の mtime 回帰 | **合格** |
| `2fbf21d` | pairing テストのポート再試行 | **合格** |
| `5021c59` | I6(メニューの回復動線) | **条件付き** |
| `5cd0a42` | I7(版 17・lazy ゲート) | **合格** |
| `2f3e61e`+`54cf983` | I8/I8b(遅延提供の状態機械・目録) | **条件付き** |
| `90bd8eb` | I3(カーソル近傍 HUD) | **不合格** |
| `6bc0791` | W1-core(FILEGROUPDESCRIPTORW) | **合格**(実行検証は不可) |

---

### c7aae32: I5 の指摘3件の修正 / 判定: **合格**

- 私の条件付き3点がすべて解消されたことを確認:
  1. **`Placed` の言い切り** → 本文を「(転送完了。相手側で受け取れます)」に変更し、
     `PLACEMENT_CLAIMS = ["置けました","置きました","置かれました"]` を **`Placed` 自身にも**
     否定 assert(`notice.rs:172-190`)→ 送信側だけで言い切らない ✓
  2. **越境直後の取りこぼし** → `DRAG_IN_FLIGHT` + `DRAG_UNFINISHED` の2つの Atomic を
     offer 時(`files.rs:445-446`)に立て、完了時(`:535`)と `TxGuard` の Drop(`:760-761`)で下ろす ✓
     `note_press_release`(`:894-897`)は **Mutex を取らない**(Atomic 2つの読み出しのみ)=
     私が提案した tap 経路の Atomic 化がそのまま入った ✓
  3. **配線が無テスト** → `placed_or_saved(released_early)` / `failed_result(e, saved_count)` を
     common の純関数にし、テストで固定(`notice.rs:302-317`)✓
     → 判定そのものは表テストで守られる(**`placed_or_saved` の反転は赤になる** = 変異 c1 相当)✓。
     ただし **mac 側の呼び出し1行(どちらの値を渡すか)は依然として無テスト**である点は変わらない
     (配線が1行になったぶん危険は小さい)✓
- 4点目(SUCCESS_WORDS の網羅性)→ `PLACEMENT_CLAIMS` に「置けました」が入り、
  かつ Placed 本文からも禁止語を排除 ✓。「送信しました/渡しました/完了しました」は対象外になったが、
  SavedOnly の本文は「送信は実際に成功している」ので、**置けた主張の語に絞ったのは妥当** ✓
- 観察: `PLACEMENT_CLAIMS` に未来形「置けます」は入らない(SavedOnly が次の一手として使う)✓ 妥当。

### c49f95d: 離しの記録の粘着バグ修正 / 判定: **条件付き**

- 差分: `common/src/drag.rs` +38 / `win/src/dragdrop.rs` +7 / `win/src/input.rs` +10/-2 /
  `win/src/tray/preferences.rs` +8/-1。
- **報告されたバグの修正は正しい** ✓: `press()` と `claim()`(一致 id の消費)で `released_once` が消える
  (`drag.rs:275-288`)。追加テスト `a_new_drag_after_a_release_is_carried_again` が
  「押下→予告→Carried」「押下なしの遅着予告→Released」の両方を固定 ✓。
- **(b) `press()` の呼び忘れ経路は見つからない** ✓: `release_expected()` の呼び出し元は
  `host.rs:47 relay_up` と `host.rs:55 relay_cancel` のみ。`relay_up` の production 呼び出し元は
  `input.rs:373`(Mac 由来の左ボタン解放)で、**必ず `input.rs:371` の `note_press()` と対**になっている ✓
  (`remote_mouse_button` の production 呼び出し元は `session.rs:542` の1箇所 ✓)。
  `relay_cancel`(モード離脱・切断)は押下を伴わないが、次に Mac から押下が来た時点で消える ✓。
- **(c) 既存テストの弱体化なし** ✓: 差分はテストの**追加のみ**(既存の assert を1行も変えていない)。
- **欠陥(反例を実行して確認)**: **`press()` が保留中の `expected` の `released` まで消す**(`drag.rs:277-279`)。
  これが余分で、既存の保護「離した後に届いた予告は Released」を新しい押下が上書きする。
  実行した反例(`/tmp` の detached worktree に検証用テストを追加して実行):
  ```
  c.release(); c.announce(9, 2); c.press(); c.claim(9, 2)
    -> left: Carried / right: Released   (assertion failed, exit 101)
  ```
  到達経路(**F3 の典型的な失敗操作**): 予告が届く → 転送中に離す(`release()` が `expected.released` を立てる)
  → データ到着前に新しい押下が来る(`press()` がそれを消す)→ 旧データが `Carried` →
  **Windows は古いファイルで OLE ドラッグを始める**(ユーザーは別の操作をしているのに、古いファイルが
  カーソルに付く/意図しない場所に落ちる)。`expected` の `released` は「announce 時点で既に離していた」=
  **前の操作に属する**という事実なので、押下で消してはいけない。
  - 修正案: `press()` は `released_once = false` **だけ**行う(`expected` には触らない)。
    粘着バグの修正はそれだけで成立する(`announce` は `released_once` を読んで記録するため ✓)。
    併せて今回の反例を回帰テストにする(期待 `Released`)。
- **入力検証の修正も確認** ✓: `b64_decode` はパディングを末尾のみ・最大2つに制限(`preferences.rs:214-219`)。
  `body.contains('=')` で中間パディングも弾く ✓。
- 未検証: Windows 実機なし(`knit-win` は macOS ではビルド対象外)。CI の windows-latest 依存 ✓。

### 1047f61 + ecf68a7: Windows の mtime 復元 / 判定: **合格**(最終形 `ecf68a7` を評価)

- 最終形(`bulk.rs:917-947`): `#[cfg(not(windows))]` は**閉じる前の fd**へ `set_mtime_file(&f, mtime)`
  (従来の速い経路を維持 ✓ 質問(b) ✓)、`#[cfg(windows)]` は `publish()` が返した**最終パス**へ
  `set_mtime(&actual, mtime)`。
- 質問(a) 順序 ✓: `publish` の中で ADS(`Zone.Identifier`)の書き込みと rename が起きるため、
  **その後**に設定するのが正しい ✓。以降に実体へ書き込む処理は無い(履歴・指紋は別ファイル)✓。
- `hash_ok` が真の枝の中でのみ実行される ✓(検証に失敗したファイルへは触らない)。
- テスト側: `\`→`/` の正規化は**構造比較を維持したまま**区切りだけを吸収 ✓(期待値側は `/` のまま ✓)。
- 実機 Windows なし → 実行検証は不可(macOS 側の `bulk_receive` は緑 ✓)。

### 2fbf21d: pairing テストのポート確保 / 判定: **合格**

- 変更は `crates/common/src/pairing.rs` の **`mod tests` 内のみ**(`@@ ... @@ mod tests {` を確認)✓
  製品コードに触れていない ✓。
- 15回試行し、毎回 `bind("127.0.0.1:0")` で**新しいポート**を得る ✓(同じポートを再試行しない)。
  失敗時は最後のエラーを添えて panic ✓ → 失敗の診断性は上がった ✓。弱点化ではない ✓。

### 5021c59: I6(失敗からの回復動線) / 判定: **条件付き**

- 差分: `notice.rs` +69(`Recovery` / `needs_attention` / `recovery_for` = 純関数+テスト ✓)、
  `files.rs` +189、`gui.rs` +70。
- **(b) `FILE_TX_BUSY` との競合なし** ✓: `retry_last()` は `send_files_to_win` を呼ぶだけで、
  枠の予約は既存の `swap`(`files.rs:35`)に乗る ✓。`Busy` はその場で通知 ✓。
- **(c) 既存項目の位置・文言** ✓: 新項目は「ファイルを送る…」の直後に3つ挿入され、
  既存項目の**相対順と文言は不変**(golden list でも `転送を中止する({line}・Esc でも可)` 等は無変更 ✓)。
  絶対位置は +3 ずれる(項目を増やせば不可避)✓。中止項目は従来どおり後段 ✓。
- **(d) 未受信時に無言で失敗しない** ✓: `!dir.exists()` で
  「受信フォルダはまだありません(ファイルを受け取ると作られます)」を通知 ✓。
  観察: `/usr/bin/open` は `spawn` の成否しか見ないので、`open` 自体の失敗は無言(軽微)。
- **欠陥(リセット漏れ・中)**: `note_outcome` の production 呼び出しは
  `files.rs:191`(通常送信の**失敗**のみ)・`:544`(掴み成功)・`:564`(掴み失敗)の3箇所で、
  **通常送信の成功経路(`:95-145` の `Ok` 側)に呼び出しが無い**。したがって
  1. ⌘C/メニュー送信が**成功しても記録が消えない** → メニューに古い
     「直前: 渡せませんでした(…)」と「もう一度送る」が残る(「直前」が事実と食い違う)。
  2. `retry_last()` → `send_files_to_win` が**成功しても記録が消えない** →
     「もう一度送る」が残り続け、もう一度押すと**同じファイルを再送**する
     (相手側は自動リネームで「名前 (n)」が増える)。
  再現手順: 掴みドラッグを失敗させる → メニューに「直前: …」が出る → 同じファイルを ⌘C で正常送信 →
  メニューを開くと**前回の失敗が残っている**(期待: 消える)。
  テスト側は `note_outcome` を直接叩くもの(`files.rs:1166-1190`)だけで、
  **呼び出し側の配線は固定されていない**(I5 の c1〜c3 と同じ穴)✓。
  修正案: 通常送信の `Ok` 腕で Placed の `note_outcome` を呼ぶ(1行)+ 受信経路も同様に。
- 観察: `menu_line` は短く、SavedOnly では「置けず、相手の受信フォルダに保存されました」✓
  (メニューに入る長さ <30 文字のテストあり ✓)。

### 5cd0a42: I7(版 17) / 判定: **合格**

- `VERSION = 17`(`proto.rs:21`)・`lazy: ver >= 17`(`:53`)・`MIN_VERSION = 11` 据え置き ✓。
  テストが 16/17/`u32::MAX`/他能力との独立性/自分の版を固定(`:670-680`)✓。
- **副作用の確認** ✓: 接続可否は `peer >= MIN_VERSION` のみ(`proto.rs:29`)→ 旧版(11〜16)は接続できる ✓。
  `PeerFeatures` は相手版から**ローカルに構成**され、wire には乗らない(serialize されていない)✓
  → フィールド追加でプロトコル互換は壊れない ✓。
  自分の版は hello の `ver` に載る(`android.rs:741` 等)✓ → 旧版は ver=17 を見ても自分の古い閾値で
  能力を判定する(= `lazy` を知らないので false)✓ 期待どおり。
- 未検証: 旧版バイナリとの実接続(版ゲートは純関数としてテスト済み ✓)。

### 2f3e61e + 54cf983: I8(状態機械)+ I8b(目録) / 判定: **条件付き**

- 差分: `common/src/drag/lazy.rs` 310 新規 + `drag.rs` +4 / `proto.rs` +76(`ManifestEntry` + `Msg::Manifest` + テスト3)。
- 設計適合 ✓: `offer` の「非対応・0 件・二重開始」ガード、**受理がデータ到着を待たない**
  (`manifest_sent` → `StartNativeDrag`)、受理前の離し = abort、件数不一致はドロップにしない、
  異常終端は同一、片付けは一度だけ、不正遷移は無視。
- **変異テスト(私の手で。base = `knit-common` 286 passed)**:

| 変異 | 内容 | 結果 |
|---|---|---|
| L1 | `offer` から `!peer_lazy` ガードを外す(旧版にも目録を送る) | **赤**(`a_peer_without_lazy_falls_back_to_the_current_path`)✓ AC7 が守られている |
| **L2** | 件数照合 `received != expected` → `received < expected`(過剰受信をドロップ扱い) | **生存** ← 欠陥1 |
| **L3** | `accepted()` を `Streaming` からも受け付ける(二重受理) | **生存** ← 欠陥2 |
| L4 | 二度目の `abort` を許す(片付けが二度出る) | **赤**(`releasing_before_acceptance_aborts_and_cleans_up_once`)✓ |
| **L5** | 死んでいる `Accepted` を matches から外す | **生存** ← 欠陥3(等価変異) |

- **欠陥1(中): 過剰受信が未固定。** コードは `!=` で正しく断るが、テストは不足側(`completed(4)` vs
  予告5)しか無い。`received > expected`(重複・別バッチ混入)を「一致」とみなす変更を検出できない。
  設計 §4 I8 (d)「目録の件数と実受信が一致しなければドロップにしない」の**片側が無防備**。
- **欠陥2(中): 二重受理が未固定。** 受理後に再度 `accepted()` が来ると `Stream` が二度出る
  (状態機械としては不正遷移として無視すべき)。テストが無い。
- **欠陥3(軽微・実証済み): `LazyState::Accepted` は到達不能な死んだ状態。**
  代入箇所は `Offered`(`:108`)・`ManifestSent`(`:118`)・`Streaming`(`:127`)・`Dropped`(`:151`)・
  `Aborted`(`:162`)のみで、`Accepted` は **matches でしか現れない**(`:134`,`:142`)。
  設計 §4 I8 の遷移 `ManifestSent → Accepted → Streaming` と実装(`ManifestSent → Streaming`)が食い違い、
  `matches!(Accepted | Streaming)` が「受理済みだが未ストリーム」があり得るように読める。
  `Accepted` を実際に代入するか、状態を削って設計を直すか、どちらかに寄せるべき。
- 観察: `Manifest` の `id/count/total` は既存 `DragOffer` と重複し、受理は既存 `DragAccept` のまま ✓
  → 配線時(後続増分)に「同じ掴みでどちらが権威か」を1箇所で決める必要がある(両方を送ると二重開始の余地)✓
- 観察: `an_older_peer_cannot_decode_the_manifest` は `"manifest_v99"` で代用しており、
  旧版の decode を実際に試してはいない(「未知タグは None」で論理的には足りる ✓)。
- 観察: `ManifestEntry.rel` は `/` 区切り(Mac 由来)。Windows の `cFileName` へ渡す際の区切り変換は
  配線時の課題(W1)✓。

### 90bd8eb: I3(カーソル近傍 HUD) / 判定: **不合格**

**理由1(致命的): `setWraps:` は NSTextField に存在しない。初回表示で例外になる。**

- `crates/mac/src/hud.rs:132` は `call1_u8(label, c"setWraps:", 1)` を **NSTextField に対して**呼ぶ。
- 実機プローブ(`clang -framework AppKit` で `respondsToSelector` を確認):
  ```
  setWraps:  *** MISSING ***      (NSTextField instancesRespondToSelector = NO)
  実行時の結果:
  *** setWraps: で例外: NSInvalidArgumentException /
      -[NSTextField setWraps:]: unrecognized selector sent to instance 0x1033a10d0
  cell setWraps: 例外なし / cell responds=1     ← 正しい呼び先は NSCell
  ```
- 影響: ラベル生成時に必ず例外になる。`PANEL` はラベル生成の**後**にしか保存されない(`hud.rs:146-148`)ため、
  拒否のたびに最初からやり直して毎回例外になる。しかも例外はメインスレッドのタイマー
  コールバック(`imp_show_hud`)から、Rust の `extern "C"` フレームと `call1_u8` を貫いて発生する
  → 最悪はプロセス異常終了、最良でも HUD は出ない(いずれにせよ「F1 が消える」増分が成立しない)。
- 検証済みの修正(プローブで例外が出ないことを確認):
  `msg0(label, c"cell")` でセルを取り、`call1_u8(cell, c"setWraps:", 1)`。
  併用可: `setUsesSingleLineMode:`(NSTextField ✓)と `setMaximumNumberOfLines:`(NSTextField ✓)。
- 他のセレクタはすべて実在を確認済み ✓(下の「良い点」参照)。**この1箇所だけが誤り**。

**理由2(重要): マルチディスプレイで、カーソルの画面ではなく主画面に clamp される。**

- `position()`(`hud.rs:158-182`)は `[NSScreen mainScreen]` の frame を `place()` に渡す。
  `mainScreen` は「キーウィンドウのある画面」(キーウィンドウが無ければメニューバーのある主画面)であり、
  **カーソルのある画面ではない**。`NSEvent.mouseLocation` はカーソル画面の座標を返すため、
  カーソルが2台目のディスプレイにあると `place()` は主画面の範囲へ clamp する →
  **HUD が別モニタに出る/カーソルから遠い端に出る**(設計 §4 I3 の検証項目に「マルチディスプレイ」が明記 ✓)。
- 修正案: `[NSScreen screens]`(実在確認済み ✓)からカーソル座標を含む画面を選び、その frame を渡す。

**その他の指摘**

3. **[軽微] `orderOut:` を引数なしトランポリンで呼んでいる**(`hud.rs:65` の `call0`)→ 第3引数が不定。
   `orderOut:` は sender を使わないので実害は出ないが、`msg1_id(panel, sel("orderOut:"), null)` にすべき。
4. **[軽微] 表示時間 1.6 秒**(`gui.rs` の timer 1.6 秒 / `HIDE_GRACE_MS = 1400`)は設計の「1.2 秒」と食い違う
   (コミットメッセージで宣言済み ✓)。連続拒否で古い予約が新しい表示を消さない仕組みは
   `SHOWN_AT_MS` + grace で成立 ✓(コードコメントの「世代を渡す」は実装と合っていない=コメントの問題 ✓)。

**確認できた良い点(セレクタは私が実機プローブで実在を確認)**

- 判断は common の純関数 `place()` ✓(4隅・負座標・画面より大きいパネルの表テスト ✓)。
- `Refusal::EmptyGrab` を足して **0 件と 64 件超の案内を分けた** ✓(`hud_line` は別文言、
  `notice_body` は同一文言)= 私の §7-5 持ち越しを解消 ✓。
- **クリック透過** `setIgnoresMouseEvents:1`(実在 ✓)、非アクティブ化 `styleMask = 128`
  (`NSWindowStyleMaskNonactivatingPanel` = 128 ✓ 実測)、`NSBackingStoreBuffered = 2` ✓、
  `setLevel:` 3(`NSFloatingWindowLevel` = 3 ✓ 実測)。
- メインスレッド規約 ✓: `performSelectorOnMainThread:withObject:waitUntilDone:` を `waitUntilDone:0` で使用
  (`gui.rs:790-806`)→ tap スレッドはブロックしない ✓。`sdShowHud:`/`sdHideHud:` は
  `class_addMethod(..., "v@:@")` で登録され(`gui.rs:2799-2800`, `:2821`)、IMP の型
  `fn(ID, SEL, ID)` と一致 ✓。`GUI_TARGET` はそのクラスのインスタンス(`:2837` の `new` → `:2876` で保存)✓
  → **呼び先はメインスレッドで正しく実行される** ✓(質問(b) ✓)。
- パネルは作り直さない ✓、2回目以降は `setStringValue:` で文言を更新(`hud.rs:71-78`)✓(質問(d) ✓)。
- 通知は併用で残っている ✓(`files.rs:312-314`: ログ → HUD → 通知)✓ 設計 I3 の注意点どおり ✓。
- `UI_PREVIEW` 中は出さない ✓(`gui.rs:784-786`)。
- セレクタ実在: `initWithContentRect:styleMask:backing:defer:` / `setReleasedWhenClosed:` / `setOpaque:` /
  `setIgnoresMouseEvents:` / `setHasShadow:` / `setBackgroundColor:` / `setLevel:` / `contentView` /
  `orderFrontRegardless` / `orderOut:` / `setFrame:display:` / `NSTextField initWithFrame:` /
  `setStringValue:` / `setBezeled:` / `setEditable:` / `setSelectable:` / `setDrawsBackground:` /
  `setTextColor:` / `setFont:` / `addSubview:` / `NSColor colorWithAlphaComponent:` / `NSColor blackColor` /
  `whiteColor` / `NSFont systemFontOfSize:` / `NSEvent mouseLocation` / `NSScreen mainScreen` /
  `screens` / `NSTimer scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:` → **すべて OK** ✓
  (`setWraps:` のみ MISSING)。

### 6bc0791: W1-core(FILEGROUPDESCRIPTORW の組み立て) / 判定: **合格**(実行検証は不可)

**独立に桁を数え直した(一次資料: MSDN `FILEDESCRIPTORW (shlobj_core.h)` を取得して照合)**

| メンバ | 型/サイズ | オフセット | 実装 |
|---|---|---|---|
| dwFlags | DWORD 4 | 0 | `flags` ✓ |
| clsid | CLSID 16 | 4 | ゼロ16 ✓ |
| sizel | SIZEL 8 | 20 | ゼロ8 ✓ |
| pointl | POINTL 8 | 28 | ゼロ8 ✓ |
| dwFileAttributes | DWORD 4 | 36 | `attrs` ✓ |
| ftCreation/ftLastAccess/ftLastWrite | FILETIME 8×3 = 24 | 40/48/56 | ゼロ24 ✓(FD_*TIME を立てないので無視される ✓) |
| nFileSizeHigh / nFileSizeLow | DWORD 4+4 | 64 / 68 | `size>>32` → `size & 0xFFFFFFFF` ✓ 順序も正しい ✓ |
| cFileName[MAX_PATH] | WCHAR 260 = 520 | 72 | UTF-16LE + NUL 詰め ✓ |

- **合計 592 バイト** = 実装の `FILEDESCRIPTORW_SIZE = 592` ✓ **一致**。
  (私は当初 `dwReserved0/dwReserved1` の 8 バイトを疑ったが、MSDN の現行定義には存在しないことを
  一次資料で確認した。**実装が正しい**)
- フラグ ✓(すべて MSDN 記載値): `FD_ATTRIBUTES 0x4` / `FD_FILESIZE 0x40` / `FD_PROGRESSUI 0x4000` /
  `FD_UNICODE 0x80000000`。フォルダは `FD_FILESIZE` を立てず `FILE_ATTRIBUTE_DIRECTORY 0x10` ✓、
  ファイルは `FILE_ATTRIBUTE_NORMAL 0x80` ✓。
- `FILEGROUPDESCRIPTORW` のヘッダ = `cItems`(UINT 4)✓ リトルエンディアン ✓。
- `descriptor_count` の検証(質問(b)): **完全長一致** `4 + n*592 == len` で弾く ✓、
  件数だけ大きく書いた偽物も弾く ✓。内容(名前の NUL 終端やフラグ)までは見ないが、
  純粋な組み立て部品としては妥当 ✓。
- テストの目視確認: `name_off = 先頭 + 592 - 520 = +72` ✓ 正しい。フォルダの属性オフセット
  `4+16+8+8 = 36` ✓ 正しい。**テストは自分の思い込みの写しではなく、MSDN のレイアウトと一致している** ✓。
- 観察(次の増分への申し送り):
  1. 259 WCHAR での切り詰めは**サロゲートペアを割り得る**(絵文字入りファイル名)→ 不正な UTF-16 が
     `cFileName` に入る。境界をコードポイント単位にするのが安全 ✓。
  2. `rel` は `/` 区切り(Mac 由来)。`cFileName` へ渡す際の区切り変換は配線時の課題 ✓。
  3. `FD_PROGRESSUI` は常に立つ(小さい転送でも進行 UI の対象)✓ 意図と思われるが明示があると良い ✓。
  4. `lazy_enabled()` のテストが `std::env::set_var` でプロセス環境を書き換える(同一バイナリ内の
     他テストと競合し得る)✓ 軽微。
- **未検証(重要)**: `#![cfg(windows)]` のため **macOS ではコンパイルすらされない**(`cargo test` の対象外)。
  Windows 実機/CI での実行確認は私にはできない。したがって本判定は
  **「読解 + 一次資料(MSDN)との突き合わせ」のみ**であり、実行による裏取りは無い ✓。

### e5fb2f1: 指摘の修正版(再検証)/ 判定: **合格**

検証日: 2026-10-18 / rev **`e5fb2f1`**(HEAD)/ `git status --porcelain` 空

- `cargo build` exit 0(**警告行 10 = 基準同数**)/ `cargo test` = **593 passed / 0 failed / 4 ignored**
  (前回 590 から **+3 = 新規テスト3件** ✓ 申告どおり)。
- 文言: `diff multiset_6bc0791 multiset_e5fb2f1` = **追加8件のみ、すべてテスト内メッセージ**
  (`多くても置かせない` / `二度目は無視` / `受理だけでは流さない` 他)✓ **production の文言変更ゼロ** ✓。
- 差分: `drag.rs` +26 / `lazy.rs` +52 / `files.rs` +10 / `hud.rs` +54(ほか私の doc 296)。

**1. AppKit: 実機プローブで再確認(私が唯一の防波堤なので独立に実施)**

| 呼び出し | 結果 |
|---|---|
| `[NSTextField cell]` | **OK** ✓ |
| `[cell setWraps:]`(NSCell へ) | **OK ✓ 実行して例外なし**(新しい呼び方で文字列を設定しても例外なし) |
| `[NSTextField setUsesSingleLineMode:]` | **OK** ✓ |
| `[NSTextField setWraps:]` | **MISSING**(= もう呼んでいないことを確認 ✓ 不合格の原因は解消) |
| `[NSArray count]` / `[NSArray objectAtIndex:]` | **OK** ✓(`[NSScreen screens]` は実行時に `count=1 / frame 0,0 2056x1329` を返した ✓) |
| `[NSWindow orderOut:]`(nil を渡す) | **OK ✓ 実行して例外なし** |
| `[NSScreen screens]` / `mainScreen` / `frame` / `setFrame:display:` | OK ✓ |

- `cursor_screen`(`hud.rs:164-198`)の論理 ✓: `[NSScreen screens]` を走査し、**半開区間**
  (`>= origin && < origin+size`)でカーソルを含む画面を選ぶ(隣接画面の二重マッチなし ✓)、
  `screens` が nil または該当なしのときだけ `mainScreen` へ落とす ✓。`place()` にはその画面の frame が
  渡る ✓ → **マルチディスプレイの指摘は解消** ✓。
- `orderOut:` は `call1_id(panel, "orderOut:", nil)`(`hud.rs:65`)✓ = ABI の指摘も解消 ✓。
- **未検証(正直に)**: **2行で実際に描画されるかは私には確認できない**(プローブで見られるのは API の実在と
  例外の有無だけ)。`usesSingleLineMode:NO` + cell の `wraps:YES` は AppKit で複数行ラベルを作る標準的な
  組み合わせ ✓ だが、**画面の目視は実機を持つ人にしかできない** ✓。

**2. 反例 → 回帰テスト(変異で実効性を確認)** ✓

- `press()` は `released_once` **だけ**を消す(`drag.rs:275-284`)✓。私の反例
  `release → announce(9,2) → press → claim(9,2)` がそのままテストになり(`drag.rs:364-371`、期待 Released)、
  **変異で元に戻すと赤**(`a_new_press_does_not_resurrect_a_transfer_that_arrived_after_release`)✓。
  粘着バグの修正も維持されている(既存テスト緑 ✓ 変異テスト ml 系も緑 ✓)。

**3. I6 のリセット漏れ** ✓ 解消

- 通常送信の `Ok` 腕で `note_outcome(Placed)` を呼ぶ(`files.rs:136-146`)→ 成功で記録が消える ✓。
  `retry_last()` は `send_files_to_win` を通るので**再送成功でも消える** ✓ = 「もう一度送る」が残り続けて
  同じファイルを二重送信する経路は塞がれた ✓。
- 観察: 記録のクリアはスキップ処理より前にあるため、一部スキップがあっても「もう一度送る」は消える
  (スキップ自体は通知で残る ✓ 妥当)。

**4. I8 の穴(変異で確認。base = `knit-common` 289 passed)**

| 変異 | 期待 | 結果 |
|---|---|---|
| m_press: `press()` を元に戻す(expected.released も消す) | 赤 | **赤**(`a_new_press_does_not_resurrect…`)✓ |
| ml2: 件数照合 `!=` → `<`(過剰受信を一致扱い) | 赤 | **赤**(`over_receipt_is_not_a_drop_either`)✓ **穴(a) 解消** |
| ml6: `streaming()` の受理前ガードを外す | 赤 | **赤**(2件: `a_second_accept_is_ignored` / `invalid_transitions_are_ignored`)✓ = **`Accepted` が生きた状態になった**(設計の遷移 `ManifestSent → Accepted → Streaming` と一致 ✓)|
| **ml3: `accepted()` を `Streaming` からも受け付ける(元の L3 の形)** | 赤 | **生存** ← 残る穴(a) |
| **ml5a: `completed()` のガードから `Accepted` を外す** | 赤 | **生存** ← 残る穴(b) |

- **残る穴(a)(軽微)**: 新テスト `a_second_accept_is_ignored` は「**受理直後**の二度目の受理」しか見ていない。
  `Streaming` 中の受理を許す変更は緑のまま(状態が `Accepted` に戻る=不正遷移)。
  1行(`h.streaming()` の後に `assert!(h.accepted().is_empty())` → 状態が `Streaming` のまま)で塞げる ✓。
- **残る穴(b)(軽微)**: `completed()` を `Accepted` から直接呼ぶ経路が未テスト(全テストが `streaming()` を挟む)。
  ガードは `Accepted` を意図的に許している(受理直後にデータが揃う場合)ので**到達し得る** ✓。
  `accepted()` → `completed(n)` のテストを1件足すのが安全 ✓。
- 元の L5(`released()` の matches から `Accepted` を外す)は**今も生存するが等価変異**:
  `_` 腕が同じ `Vec::new()` を返すため挙動が変わらない ✓ = 指摘の性質が
  「到達不能な死んだ状態」から「冗長な腕」に変わった ✓(実害なし)。

**5. 判定の更新**

- **I3: 不合格 → 合格**(致命的だった `setWraps:` とマルチディスプレイ・`orderOut:` が解消 ✓。
  ただし**目視は依然として未実施** ✓ 私には画面を確認できない)。
- **c49f95d / I6 / I8/I8b: 条件付き → 合格**(指摘はすべて解消 ✓。I8 には上記の軽微な残り2点 ✓)。
- この修正版で、私が第3ラウンドで出した指摘は**すべて解消**した ✓。

## 6.6 第4ラウンド: W1系とW4(未検証6件+α)/ 検証中に HEAD が動いた

**検証対象(マージ除く)**: `58240f1`(W1-glue COM)→ `3d820f5`(allow)→ `d07164a`(mac 目録)→
`05a5cd7`(win 配線)→ `5b84b02`(能力と受理の分離)→ `ef9c43b`(W4)→ `e6b4035`(3つの防御)→
`652afc4`(I8 の残り2点)→ `dbccefc`(ABI 期待値の修正)。

**アンカーと環境**:
- C1 時点は `defa1a7`(クリーン)。**検証中に `e6b4035` と `dbccefc` が着地した**ため、
  途中で読んだ `lazy_com.rs` は `e6b4035` の状態(ABI テストが **24** を assert)だった。
  これは後述の「CI が検出した誤り」と一致する ✓。**教訓: 検証は常にコミットで固定する**
  (§1.4 のコミット指定と同じ理由。作業ツリーを読むと、実装中の状態を「その増分」と誤認する)。
- 最終確認は **`dbccefc`**: `cargo build` exit 0(**警告行 10 = 基準同数**)/
  `cargo test` = **598 passed / 0 failed / 4 ignored, 11 binaries**。
- 文言: `dbccefc` の `H_multiset` = `5343a6a4…96a850`(= ラウンド開始時と同一)。
  新機能の文言(`[drag] mac->win manifest …` / `{count} 件を {peer} の画面に置きました(ドロップできました)` /
  `相手が受け取らなかったため(置き場所が無い・取消など)、相手の受信フォルダに入りました` 等)以外に
  **意図しない変更は無い** ✓。

### フレーク2件の切り分け(製品コードの問題ではない)

私の最初の `cargo test` は **exit 101 / failed=2** だった:
`android::tests::clipboard_image_returns_fast_when_device_is_offline`(adb_calls=0)と
`updater::tests::apply_rolls_back_when_the_new_version_does_not_stay_running`(`assertion failed: !ok`)。

- **単体で各3回 + 2件同時 = すべて緑** ✓ / **干渉なしの全体 = 598 passed / 0 failed** ✓
- `android.rs`・`updater.rs` は今ラウンドで**未変更**(`git diff --stat e5fb2f1..HEAD --` が空)✓
  → **製品コードの問題ではない** ✓(リードの同時実行との衝突 ✓)
- adb 側の原因はコードで確認: `android.rs:1419` の固定ディレクトリ(`temp_dir()/knit-android-stub`)と
  固定 serial(`stub-serial`、`:1430`)を2つの `cargo test` が共有するため ✓(リードの説明と一致)
- **updater 側は別の原因**(adb の説明では説明できない): 一時 root は PID を含むので衝突しない(`updater.rs:495`)✓
  → 「新しい版が動き続けているか」の判定が負荷・タイミング依存か、`/usr/bin/pkill -f -- <root>`(:521-524)の
  プロセス一致に依存しているためと推測される(**推測であることを明記**)✓
- 推奨: この2件を直列化する(`--test-threads=1`)か、プロセス名一致に依存しない判定にする ✓

### 58240f1: W1-glue(COM 面)/ 判定: **合格**(読解+MSDN 照合のみ。実機なし)

**vtbl と構造体(一次資料・実行の両方で確認)**

| 対象 | 検証 |
|---|---|
| `IStream` の並び(14) | qi, add_ref, release, read, write, seek, set_size, copy_to, commit, revert, lock_region, unlock_region, stat, clone = **MSDN の宣言順と一致** ✓ 各署名も `Read(void*,void*,ULONG,ULONG*)` / `Seek(LARGE_INTEGER,DWORD,ULARGE_INTEGER*)` 等と一致 ✓ |
| `IDataObject`(12) | qi, add_ref, release, get_data, get_data_here, query_get_data, get_canonical_format_etc, set_data, enum_format_etc, d_advise, d_unadvise, enum_d_advise ✓(既存実装と同じ並び ✓) |
| `IEnumFORMATETC`(7) | qi, add_ref, release, next, skip, reset, clone ✓ |
| `STATSTG` | **MSDN(`objidl.h`)と完全一致** ✓ 桁を自力で数え直して **80 バイト** ✓(name@0, type@8, cbSize@16, mtime/ctime/atime@24/32/40, grfMode@48, grfLocks@52, clsid@56, grfStateBits@72, reserved@76)|
| `STGMEDIUM` | tymed@0, h_global@8, p_unk_for_release@16 = **24 バイト** ✓ union へは `h_global` 経由でポインタを書いており、union は全てポインタサイズなので**レイアウト等価** ✓ |
| `FormatEtc` | **実測 32 バイト**(下記)。offsets は cf_format@0, ptd@8, dw_aspect@16, lindex@20, tymed@24 = **C の FORMATETC と一致** ✓ |
| `Guid` | 16 バイト ✓ |
| クリップボード形式 | `RegisterClipboardFormatW("FileGroupDescriptorW"/"FileContents")` ✓(値は実行時に決まる ✓)|
| フラグ | `TYMED_ISTREAM=4` / `STGTY_STREAM=2` / `STREAM_SEEK_*` / `GMEM_MOVEABLE=0x2` ✓ |

**ABI の実測(私の手で実行)**: 同一の `#[repr(C)]` 定義を rustc でコンパイルして `size_of`/オフセットを出力:
```
size_of FormatEtc = 32     offsets: cf_format=0 ptd=8 dw_aspect=16 lindex=20 tymed=24
size_of StgMedium = 24     offsets: tymed=0 h_global=8 p_unk=16
size_of StatStg   = 80
size_of Guid      = 16
```
- `e6b4035` は `FormatEtc == 24` を assert していた → **誤り**(実測 32。ポインタ由来の整列8で
  28→32 に切り上げ)✓。**`dbccefc` が 32 に修正済み**で、私の実測と一致する ✓
  (コミットメッセージに「CI が私の誤りを検出」とあり、CI が実際に機能した例 ✓)。
- **提案**: サイズだけでなく **オフセット**を `std::mem::offset_of!` で固定する ✓
  (今回の誤りはサイズ側だけだったが、OLE が読むのはオフセット ✓。実測では offsets は正しい ✓)。

**参照カウント(QI で AddRef、0 で Box::from_raw)** ✓
- `LazySource`: `Box::into_raw`(refs=1)→ DoDragDrop に渡す → OLE が AddRef/Release → 終了後に
  `lazy_com::release(ds)` で作成側の参照を落とす(`dragdrop.rs:637`)✓ **漏れも二重解放も無い** ✓
- `Stream`: `GetData` ごとに `refs=1` で生成し `pUnkForRelease=NULL` で渡す ✓
  = **呼び手が Release する**という TYMED_ISTREAM の規約どおり ✓(Refs=1 をそのまま渡している ✓)
- `LazyEnum`: `refs=1` で生成して返す ✓ / `fetch_sub` の underflow は u32 に飽和(二重解放にはならない)✓
- `GetData` は `idx >= files.len()` で `DV_E_LINDEX` ✓ 境界チェックあり ✓

**HGLOBAL** ✓: `GlobalAlloc(GMEM_MOVEABLE)` → `GlobalLock` → `copy` → `GlobalUnlock` ✓
(受け手の `ReleaseStgMedium` が解放する規約に一致 ✓)。軽微: `GlobalLock` が失敗したとき
`GlobalFree` していない(確保失敗/OOM 経路のみの小さな漏れ)✓

**st_read の待ちと pos** ✓: 宣言サイズに達するまで `wait_ready`(10ms 刻み、上限 30 秒)で待ち、
`File::open` + `seek(pos)` で読めた分だけ返す ✓ pos は読めた分だけ進む ✓ 終端は S_OK/0 バイト ✓
期限切れは E_FAIL(無限に待たない ✓)。軽微: `resolve_received` を Read ごとに呼ぶ
(通常は metadata 1 回 ✓、同名衝突時は最大 100 回 ✓ 有界 ✓)。

**その他の軽微な観察**: `st_stat` は `pwcsName` を常に NULL、`grfMode` を 0 で返す
(`STATFLAG_NONAME` 以外の呼び手には名前を返すのが仕様だが、Explorer は NONAME ✓ 実害は薄い)/
`le_skip` は末尾を超えても S_OK(S_FALSE が仕様)✓ / `ls_get_canonical` は ptd=NULL + E_NOTIMPL ✓ 仕様どおり。

### d07164a: Mac が目録を送る / 判定: **合格**

- `bulk::manifest_of(&entries)` を **`entries` を `DRAG_TX` へ move する前**に作っている ✓
  (`files.rs`: `let manifest = peer_features_lazy().then(|| bulk::manifest_of(&entries));` の直後に move ✓)。
- 送信は `if let Some(files) = manifest` の中だけ = **旧版(版17未満)には送らない** ✓
  (`peer_features_lazy()` が版ゲートに一本化 ✓ 閾値リテラルを呼び出し側に書かない ✓)。
- `manifest_of` は `OutFile` の名前・サイズ・`is_dir` だけを見る(**中身を読まない** ✓)。
  テスト `manifest_carries_names_sizes_and_kinds_without_reading_contents` が名前・サイズ・種別と
  空入力の両方を固定 ✓(名前・種別の対応関係も ✓)。
- 観察: `rel` は Mac 由来の `/` 区切り ✓(Windows 側の `staged_path` が `\`→`/` に正規化して
  `/` で分割するので整合 ✓)。

### 05a5cd7: Windows が目録で即開始 / 判定: **条件付き**

- **二重開始の防止は効いている** ✓: `start_lazy` はセッションスレッドで同期的に
  `LAZY_STARTED.store(id)`(`dragdrop.rs:580`)してからスレッドを起こし、claim 経路
  (`xfer.rs:486` 付近)が `Claim::Carried if dragdrop::lazy_started(id)` で**通常の start を省略** ✓。
  早期 return の3経路で `LAZY_STARTED` を消す(`:586-610`)✓ = e6b4035 の防御 ✓。
- **`resolve_received` は無限ループしない** ✓: `1..100` の有界探索 ✓、見つからなければ宣言パスを返して
  呼び出し側の待ちが期限切れになる ✓。`staged_path` が `..`・絶対パス・`:`(ADS)・空要素を拒否 ✓
  = 受信先の外へは書かない/読まない ✓。誤マッチはサイズ一致ヒューリスティックなので理論上あり得るが、
  1つの目録内で rel は一意 ✓ 受信側はハッシュ検証済み ✓ 実害は薄い ✓。
- **欠陥1(中)**: **`DragAccept` が「実際にドラッグを開始できたか」より先に送られる**。
  `session.rs` の Manifest 腕は `dragdrop::start_lazy(...)` の直後に無条件で accept を返すが、
  `start_lazy` は**スレッドを起こすだけ**で、その中で (a) `OleInitialize` 失敗、(b) `Host::create` 失敗、
  (c) **押下が続いていない**のいずれかなら開始せずに return する(`dragdrop.rs:582-615`)✓。
  とくに (c) は「フリックして離す」= **F3 の典型操作**そのもの ✓。
  この場合 Mac の `lazy_accepted(id)` は true のまま ✓ → `early = DRAG_RELEASED_EARLY && !accepted` が
  **false に固定**され(`files.rs`)、結果も来ない(相手は DragDone を送らない)ので 1.2 秒待って
  フォールバックの `Placed`(「転送完了。相手側で受け取れます」)になる ✓
  = **遅延提供を使わない経路なら `SavedOnly{ReleasedEarly}`(「保存のみ」)と言えていた場面で、
  より曖昧な(実態は保存のみなのに受け取れると言う)通知になる** ✓ + 予報(HUD)も「置けます」に反転する ✓。
  **修正案**: accept は「押下確認を通って実際に `DoDragDrop` を始める直前」に送る
  (または開始できなかったときに Mac の受理状態を消す/「開始できず」を返す)✓。
- **欠陥2(軽)**: **`receive_dir()` の一本化漏れ** — トレイの「受信フォルダを開く」は今も
  `tray.rs:913-916` で `USERPROFILE/Downloads/Knit` を**自前で組み立てている** ✓
  (`main.rs:241` の `receive_dir()` を使っていない)。1行で揃う ✓。
- 観察(競合・低確率): Manifest はセッションスレッド、claim は bulk スレッドで処理されるため、
  理論上は claim が先に処理されると `lazy_started(id)` がまだ false → 通常 start が走り、
  その後に Manifest で `start_lazy` が走って **OLE ドラッグが二重**になり得る ✓
  (Mac は目録を先に送り、bulk は転送スレッドの起動を挟むので実際は Manifest が先 ✓ 低確率)。
  対策案: `lazy_enabled() && 相手が lazy` のときは通常開始を抑止する(または DragOffer 時点で
  「目録が来る」ことを記録する)✓。

### 5b84b02: 能力と受理の分離 / 判定: **合格**

- `ForecastInput.accepted` を追加し、`WillPlace` の条件を `peer_lazy` から **`accepted`** に変更 ✓
  = 私が I2 の記録で申し送った「能力≠受理」が**構造で解消** ✓。
- テスト `capability_without_acceptance_still_waits` が「能力だけでは反転しない/受理で反転する」を固定 ✓
  → **変異 mf1(`accepted`→`peer_lazy`)が赤** ✓、mf2(`accepted` を無視)も赤 ✓。
- **(a) `id >> 63 == 1` ガードは Win→Mac の受理を食っていない** ✓: Win 側のコードに最上位ビットを
  立てる箇所は無い(`grep "1 << 63|DRAG_ID_BIT|0x8000…" crates/win/src` = 0 件 ✓)。
  Mac の `DragAccept`/`DragDone` の腕はこのガード付きの1つだけで、他は既存の catch-all ✓。
  逆向き(Windows 側が Mac 発の `DragDone` を受ける)も `edge::cancel(id)` が
  **id 一致時だけ**状態を消す(`edge.rs:42-47`)✓ + RX_DRAG も id 一致時のみ ✓ = **無害** ✓。
- **(b) 受理のリセット** ✓: `offer_drag_to_win` ごとに `LAZY_ACCEPTED_ID=0`(`files.rs:455`)✓、
  `DRAG_DONE=None` も同所で消す ✓ → 前の掴みの受理/結果を引き継がない ✓。
- **(c) 予報の反転** ✓: 上のとおり「受理の事実」でのみ反転 ✓。

### ef9c43b: W4(結果を事実で言い切る)/ 判定: **条件付き**

- **(a) 待ちの間のロック** ✓ **問題なし**:
  `wait_drag_done` の `if let Some(...) = *DRAG_DONE.lock()…` は**一時ガードが if-let 文の末尾で落ちる**ため、
  `sleep(25ms)` はロックの外 ✓。これは重要で、もし保持したままなら `note_drag_done`(セッションスレッド)が
  書けず、**結果が永久に来ない→毎回タイムアウト**になるところだった ✓(コードを読んで確認 ✓)。
- **(b) 推測で言い切らない** ✓: 結果が来なければ `None` → `placed_or_saved(early && !accepted)` に落ち、
  `Placed` の慎重な本文(「転送完了。相手側で受け取れます」)になる ✓。「置けました」と言うのは
  `DragDone{copied:true}` が届いた `PlacedOnScreen` だけ ✓。
- **(d) 文言規則** ✓: `PeerDidNotPlace` でも成功語は入らない ✓(変異 mn2 が赤 ✓)。
- **欠陥3(中): `PeerDidNotPlace` の文面が冗長で、次の一手が誤っている。**
  共通テンプレートが既に「…の受信フォルダに保存されています({why})。もう一度掴んで、**渡し終わるまで
  待ってから離すと相手の画面に置けます**」なので、`why` に「相手の受信フォルダに入りました」を足すと
  **受信フォルダの記述が二重**になり ✓、さらに「待ってから離すと置けます」は**この理由では誤った案内** ✓
  (相手が受け取らなかった = 置き場所が無い/取消であり、待っても直らない)。
  → 理由ごとに「次の一手」を分ける(例: 置ける場所(フォルダ等)へドロップしてください)✓。
- **欠陥4(軽): `needs_attention`/`recovery_for` の `PlacedOnScreen` が未テスト** ✓
  (変異 mn3「PlacedOnScreen を要注意にする」が**生存**)。成功を「要注意」にするとメニューに
  「直前: …置きました」と再送項目が出る ✓ → テスト1件で塞げる ✓。
- 観察(軽): 1.2 秒待つ間も `TxGuard` が生きているため `FILE_TX_BUSY` と転送中表示が最大 1.2 秒残り、
  直後の掴みが「前のファイルを転送中です」で断られ得る ✓。
- 観察: `DRAG_DONE` は「最後に届いた結果」1件のみ保持するため、古い結果が新しい結果を上書きすると
  新しい方はタイムアウトして慎重側に落ちる ✓(誤って言い切ることはない ✓)。

### e6b4035 + dbccefc: 3つの防御と ABI 期待値の修正 / 判定: **合格**

- **押下確認** ✓: Manifest 到着後 `CLAIM_WAIT_MS` のあいだ押下を待ち、離れていれば開始しない ✓
  (ただし accept は先に送られている = 上記 05a5cd7 の欠陥1 ✓)。
- **早期 return のフラグ漏れ** ✓: 3経路すべてで `LAZY_STARTED` を消す ✓ + DoDragDrop 後に必ず消す ✓。
- **ABI 固定テスト** ✓(stat 80 / FormatEtc 32 / StgMedium 24 / Guid 16)。
  `e6b4035` の `FormatEtc == 24` は**誤り**で、私の実測(32)と `dbccefc` の修正が一致 ✓
  (**CI が検出した**とコミットメッセージにあり、CI が機能した実例 ✓)。

### 652afc4: I8 の残り2点 / 判定: **合格**

- 私が前ラウンドで残した2点(`Streaming` 中の受理の無視 / `Accepted` 直後の `completed`)に
  テストが入った ✓ → **変異 ml3'・ml5a' がどちらも赤** ✓ = **残指摘は解消** ✓。

### 変異テスト(私の手で実施。worktree は撤去済み)

base = `knit-common` **294 passed** / すべて `defa1a7` で実施(HEAD の移動前に固定)。

| 変異 | 結果 |
|---|---|
| mf1 `accepted` → `peer_lazy`(能力で反転) | **赤**(`capability_without_acceptance_still_waits`)✓ |
| mf2 `accepted` を無視(`if false`) | **赤**(2件)✓ |
| mn1 `PlacedOnScreen` の断定を外す | **赤**(`only_a_peer_confirmed_drop_claims_the_place`)✓ |
| mn2 `PeerDidNotPlace` の理由に成功語を混ぜる | **赤**(2件)✓ |
| **mn3 `PlacedOnScreen` を「要注意」にする** | **生存** ← 欠陥4 |
| ml3' 流している最中の受理を受け付ける | **赤**(`an_accept_arriving_while_streaming_is_ignored`)✓ 解消 |
| ml5a' 受理直後の `completed` を拒否する | **赤**(`completion_can_arrive_right_after_acceptance`)✓ 解消 |

## 6.7 第5ラウンド: 指摘5件の修正版(`121b7b1` / マージ `5567a5f`)/ 判定: **合格**

検証日: 2026-10-18 / rev **`5567a5f`**(`121b7b1` のマージ)/ `git status --porcelain` 空 /
`cargo build` exit 0(**警告行 10 = 基準同数**)/ `cargo test` = **598 passed / 0 failed / 4 ignored, 11 binaries**。

文言: `diff multiset_dbccefc multiset_121b7b1` は**意図した修正のみ** ✓
(`…保存されています({reason})。{next}` への分割 / `相手が受け取らなかったため(…)、相手の受信フォルダに入りました`
→ `相手が受け取らなかったため` / 新しい次の一手 `置ける場所(フォルダやデスクトップ)の上で離すと置けます` /
テストメッセージ)。`H_multiset` = `19ce108b…3995d4`。

**1. 受理のタイミング(前回の欠陥1)= 塞がった** ✓
- `session.rs` の Manifest 腕から accept 送信を削除 ✓。`start_lazy` のスレッド内で
  (a) `OleInitialize` → (b) **押下確認** → (c) `Host::create` → (d) `DRAG_THREAD` 設定 →
  **(e) accept 送信** → `DoDragDrop` の順になり、「実際に開始できると確認できた後」だけ accept を返す ✓。
  3つの失敗経路(OLE 初期化・Host・押下なし)はすべて accept を送らずに return する ✓。
- 帰結を Mac 側の現物で確認: フリック離し(押下なし)では `lazy_accepted(id)` が **false のまま** ✓ →
  `early = DRAG_RELEASED_EARLY.load() && !accepted`(`files.rs:566-575`)が **true** →
  `placed_or_saved(true)` = **`SavedOnly{ReleasedEarly}`**(「保存のみ」)✓
  = **遅延提供なしと同じ正しい案内に戻った** ✓(accept 済みだが結果が来ない場合だけ慎重な `Placed` に落ちる ✓)。

**2. 案内文(前回の欠陥3)= 直った** ✓
- `(reason, next)` を理由ごとに分離 ✓。`PeerDidNotPlace` の次の一手は
  「置ける場所(フォルダやデスクトップ)の上で離すと置けます」= **「待てば直る」案内が消えた** ✓。
- 受信フォルダの記述は1回だけ ✓(`n.body.matches("受信フォルダ").count() == 1` をテストで固定 ✓)。

**3. `receive_dir()` の一本化** ✓: `tray.rs:911` が `crate::receive_dir()` を使用 ✓(自前組み立てを削除 ✓)。

**4. `needs_attention`/`recovery_for(PlacedOnScreen)`** ✓: 両テストに assert が入り、
**前回生存した変異 mn3 が赤になった**(下の表 ✓)+ 再送(Resend)を出す変異 mn4' も赤 ✓。

**5. 二重開始(前回の低確率競合)** ✓: `start_lazy` の先頭に `DRAG_THREAD != 0` ガード ✓。
`DRAG_THREAD` は通常の start(`dragdrop.rs:707`)と Win→Mac の帯(`edge.rs:690`)も立てるため、
**「claim が先に処理されて通常ドラッグが走り、後から Manifest が届く」順序でも二重にならない** ✓。
ガードは `LAZY_STARTED` を立てる**前**に return するので、通常経路の claim 判定も壊さない ✓。

**6. その他** ✓: `hglobal_from` は `GlobalLock` 失敗時に `GlobalFree`(`lazy_com.rs:715`)✓ /
`le_skip` は末尾超過で **S_FALSE**(`saturating_add` で桁溢れも回避 ✓)。

**変異テスト(私の手で再実行。base = knit-common 294 passed、worktree は撤去済み)**

| 変異 | 結果 |
|---|---|
| **mn3' `PlacedOnScreen` を「要注意」に戻す**(前回生存) | **赤**(`only_failures_ask_for_attention`)✓ = 解消 |
| mn4' `PlacedOnScreen` に再送(Resend)を出す | **赤**(`retryable_failure_offers_resend`)✓ |
| mo1 `PeerDidNotPlace` に「待ってから離す」を混ぜる | **赤**(`saved_only_never_uses_success_words`)✓ |
| mo2 `PeerDidNotPlace` の理由に「受信フォルダに入りました」を戻す | **赤**(同上)✓ |
| mo3 2つの理由の次の一手を入れ替える | **赤**(同上)✓ |

- **Windows 専用部分(受理のタイミング・`DRAG_THREAD` ガード)は読解のみ** ✓:
  `knit-win` は macOS ではビルドできず、この配線に自動テストも無いため、**CI(windows-latest)が唯一の
  実行検証**である ✓。「accept を Manifest 腕へ戻す」変異はローカルでは検出できない(テストが存在しない)✓
  = 前回の指摘どおり、この層は**読解が唯一の防御線**であり続ける ✓。

**残る観察(いずれも軽微)**

- 速い転送では、accept が届く前に Mac の送信が完了すると `accepted=false` となり、W4 の事実を待たずに
  慎重な `Placed` 本文(「転送完了。相手側で受け取れます」)になる ✓。嘘ではないが「置けました」は出ない
  (転送が accept の往復より遅い場合だけ事実で言い切れる)✓。
- 1.2 秒待つ間も `TxGuard` が生きるため `FILE_TX_BUSY` と転送中表示が最大 1.2 秒残る ✓(前回から継続)。
- 新しい assert(理由・次の一手・受信フォルダ1回)は `saved_only_never_uses_success_words` という名前の
  テストに入っており、名前と内容がずれてきている ✓(assert メッセージで特定はできる ✓ 軽微)。
- `DRAG_THREAD.store` の後に `DoDragDrop` までで panic すると `DRAG_THREAD` が残る
  (既存の通常 start と同じ構造 ✓ 軽微)。

## 6.8 第6ラウンド: Win→Mac の受け取り進捗 HUD(`53e60c3`・`ced4db4` ほか)/ 判定: **条件付き**

**検証対象(マージ除く。範囲 `121b7b1..ced4db4`)**: `1ed9c81`(notice テスト分離)→ `31352ce`(速い転送でも受理を待つ)→
`b8aec13`(docs)→ `78cd925`(ハンドシェイク統合テスト + updater の間欠テスト堅牢化)→ `c4453d4`(docs)→
**`53e60c3`**(受け取り進捗 HUD)→ `54428f2`(docs)→ **`ced4db4`**(進捗が断りを消さない)。

- HEAD `ced4db4` / `git status` 空 / `cargo build` exit 0(**警告行 10 = 基準同数**)/
  `cargo test` = **605 passed / 0 failed / 4 ignored**(再実行2回とも同じ ✓)。
- 文言: 差分は**意図した新規のみ** ✓(`受け取り中 {pct}%({} / {}){speed}` / `受け取り中 {}{speed}` /
  `受け取りの準備中です` / `・{}/s` / `転送が終わると Mac 側でドラッグを続けられます`)。
  notice テストの文字列が消えて見えたのは `1ed9c81` の**テスト分離**(アサーションは維持 ✓ 弱体化ではない ✓)。

### 重点7点への回答

**1. 配線の穴 = なし** ✓
- `on_rx_bytes` は `bulk.rs:1192` の定義どおり「**バッチ累計バイト**」なので `RX_BYTES.store(n)` は正しい ✓
  (差分ではなく累計 ✓)。`on_batch_begin` は非キャプチャのクロージャで `fn()` に型強制され ✓、
  `note_bulk_rx_device()` が**先頭**で呼ばれる = 履歴ラベルの固定は従来どおり ✓ 呼び出し順・呼ばれ方に変化なし ✓。
- 進捗 HUD は `WAITING`(掴み越境の待ち)があるときだけ出るので、通常の ⌘C 受信バッチでは表示されない ✓。

**2. 待ち状態の消し忘れ = 追った限り無し** ✓(唯一の穴は「無音のハング」= 欠陥3)
- 立てるのは `offer` が受理したときだけ(`incoming_drag.rs:93`)✓ 拒否時は立てない ✓。
- 消すのは `receive` の Ok(`:114`)✓ / `cancel`(`:130`)✓ / `reset`(`:143`)✓。
  **`receive` の Err 腕は `cancel(id)` を呼ぶので消える** ✓(`delete_received` → `cancel(id)` → `clear_waiting` ✓)。
  切断は `reset` ✓。`commit` は `receive` の後なので不要 ✓。`Msg::DragDone` はこの向きでは Mac が**送る**側 ✓ 無関係 ✓。

**3. 毎秒更新の副作用 = 問題なし** ✓
- タイマーは `updateStatus:` を **1.0 秒**で回している(`gui.rs:2891`・`:3105`)✓ = 申告どおり。
- `note_progress` → `request_hud` は `performSelectorOnMainThread:withObject:waitUntilDone:0` ✓(非ブロッキング ✓)
  → メインスレッドでは `take_pending` + `setStringValue` + `setFrame` + `orderFrontRegardless` + 1.6 秒タイマー
  = **1秒あたり 1 回の軽い更新** ✓。
- 「開示中は書き換えない」規約は**メニューバー題名**の話(`state_title_set_now`)✓ で、HUD は別パネル ✓
  規約違反ではない ✓。メニュー追跡中は既定モードのタイマーが発火しないため、HUD の更新も保留される ✓
  (追跡終了後に最新の1件だけが反映される ✓)。

**4. 1.6 秒の自動非表示との噛み合わせ = 両立している** ✓
- 毎秒の出し直しで `SHOWN_AT_MS` が更新され、各非表示タイマーは `now - shown < 1400ms` で拒否される ✓
  → 待っている間は消えない ✓。最後の出し直しから 1.6 秒後に来るタイマーだけが `1600 >= 1400` で隠す ✓
  → 待ちが終われば**自然に消える** ✓。

**5. 文言の正しさ = 正しい** ✓
- 断りは `HUD_HINT`(「このまま離すと Mac 内の操作になります」)✓、受け取り待ちは
  `PROGRESS_HINT`(「転送が終わると Mac 側でドラッグを続けられます」)✓ と**別の一言**を使い分け ✓、
  テストが「進捗の本文に HUD_HINT を入れない」を固定 ✓(`refusal_and_progress_use_different_reassurance` ✓)。
- この向き(Win→Mac)は**離すと保存のみ**なので「そのまま離してよい」と読める案内は誤り ✓
  使っていないことを確認 ✓。逆の案内にもなっていない ✓(転送完了後に Mac 側でドラッグが続く、が事実 ✓)。

**6. 純関数 `receive_progress_line` = 妥当** ✓
`received==0` → 「準備中」(0% と偽らない ✓)/ `total==0` → % を出さず実量と速度のみ ✓/
`pct` は `min(100)` で上限 ✓ かつ `saturating_mul` で桁溢れなし ✓/ `secs==0` は速度を出さない(0除算・∞/s を回避 ✓)✓。
テスト5件(割合・実量・速度 / 経過0 / 合計不明 / 100%上限 / 準備中)✓ 変異 m3・m4・m5 がすべて赤 ✓。

**7. 変異(私の手で再実行。base: hud 4 / progress 2 / accept 2 passed、すべて直列実行)**

| 変異 | 結果 |
|---|---|
| m1 進捗の優先度ガードを外す | **赤**(`progress_does_not_erase_a_fresh_refusal`)✓ |
| m2 進捗に断りの一言(HUD_HINT)を使う | **赤**(2件)✓ |
| m3 合計不明でも % を出す | **赤**(`an_unknown_total_never_fakes_a_percent`)✓ |
| m4 受信0バイトで 0% を出す | **赤**(同上)✓ |
| m5 100% 上限を外す | **赤**(同上)✓ |
| **m6 待ち状態を消す経路を1つ外す(`cancel` の `clear_waiting`)** | **生存**(306 passed)← **欠陥3** |
| m7 目録を送っていない相手でも1秒待つ | **赤**(`no_wait_when_no_manifest_was_sent`)✓ |
| m8 受理の到着を待たない | **赤**(`waiting_stops_as_soon_as_the_acceptance_arrives`)✓ |
| m9 自動非表示の grace(1400→100)を変える | **赤**(`a_stale_hide_request_does_not_close_a_newer_hud`)✓ |

### 見つけた欠陥

1. **[中・テスト] `hud::tests` が並列実行でフレークする。** `PENDING` はプロセス共有の static で、
   テスト間のロックが無い ✓。私の実行では**フィルタ実行時に実際に落ちた**:
   `progress_does_not_erase_a_fresh_refusal` が `受け取り中 42%\n転送が終わると…` を掴んで assert 失敗 ✓
   (他テストの `take_pending()` が断りを消費したため ✓)。`--test-threads=1` では 4 passed ✓、
   全体実行は3回とも 605 passed ✓ = **並列度しだいで落ちる** ✓。
   → `updater` に入れた `PROC_LOCK` と同じ手当て(テスト専用の static ロック)を推奨 ✓。
   (CI が緑でも再現し得る種類 ✓)
2. **[中・挙動] 優先度ガードは「2秒間の表示」を保証しない。** `PENDING` はメインスレッドの `take_pending()` で
   **即座に空になる**ため、ガードが効くのは「断りがまだ表示待ちの間」だけ ✓。表示後は次の毎秒更新(≤1秒)で
   断りの文言が進捗に置き換わる ✓(テスト自身もその挙動を固定している: 断りを消費した後に進捗が出る ✓)。
   実害は「古い待ちが残っている状態で Mac→Win の掴みを断る」等の限定的な重なりに限られる ✓(低頻度)。
   → 2秒を守るなら、`take` を跨いで残る `LAST_REFUSAL_MS`(AtomicU64)を `note_progress`/`imp_show_hud` が見る ✓。
3. **[軽] 待ち状態にタイムアウトが無い。** `WAITING` は offer 受理で立ち、データ到着(`receive`)/`cancel`/`reset`
   でしか消えない ✓。相手がハングして「データもキャンセルも切断も来ない」場合、進捗 HUD が**消えずに残る** ✓
   (速度表示は 0 B/s 側へ減衰する ✓)。変異 m6 が生存 = **この消去経路はテストされていない** ✓。
   → 数秒間 `RX_BYTES` が進まなければ消す(または「応答なし」に切り替える)✓。
4. **[軽] offer 受理〜最初のバッチ開始の間は、前のバッチの `RX_BYTES` を読む。** リセットは
   `on_batch_begin` のみ(`main.rs` のクロージャ ✓)なので、その窓で毎秒更新が走ると**前回の受信量**で
   割合を描く ✓(100% 上限のため見た目は破綻しないが内容は誤り ✓)。→ `offer` でも RX を 0 にする ✓。
5. **[軽] `TxGuard` の保持が最大 2.2 秒に伸びた**(受理待ち 1.0 秒 + 結果待ち 1.2 秒)。
   その間 `FILE_TX_BUSY` と転送中表示が残り、直後の掴みが「前のファイルを転送中です」で断られ得る ✓。
6. **[軽] `body()` が恒等関数になった**(2行の組み立ては呼び出し側 ✓ 残骸 ✓)。

### 範囲内の他のコミット(名前指定は無かったが同一範囲)

- **`31352ce`(速い転送でも受理を待つ)= 合格** ✓: `MANIFEST_SENT_ID` を掴みごとにリセットし、
  目録を送った相手にだけ最大1秒 `wait_lazy_accepted` で受理を待つ ✓ = 私が前ラウンドで
  「速い転送では事実を待たず慎重な本文になる」と観察した点の改善 ✓。目録を送っていない相手は即 false ✓
  (旧版に遅延を足さない ✓)。変異 m7・m8 が赤 ✓。
- **`1ed9c81`(notice テスト分離)= 合格** ✓: 私が指摘した「テスト名と内容のずれ」を解消 ✓
  アサーションは維持(受信フォルダ1回の検査も新テストへ移動 ✓ 弱体化なし ✓)。
- **`78cd925`(統合テスト + updater 堅牢化)= 合格** ✓: 実メッセージで両側の状態機械を駆動する
  端から端のテストを追加 ✓。updater の間欠失敗は**私が前ラウンドで報告したフレーク**で、
  `PROC_LOCK` による直列化 + 待ち秒数の余裕で堅牢化 ✓(テスト専用 ✓)。adb 側の固定ディレクトリ競合は
  未対応のまま ✓(前ラウンドの指摘どおり ✓)。

## 6.9 第7ラウンド: 指摘4件の修正版(`a59c191`)/ 判定: **条件付き**

- HEAD `a59c191` / `git status` 空 / `cargo build` exit 0(**警告行 10 = 基準同数**)/
  `cargo test` = **608 passed / 0 failed / 4 ignored**(申告どおり ✓。**3回連続で再現** ✓)。
- 文言: 新規は期限内ログ `[drag] 受け取り待ちが期限切れ({id})` のみ ✓(ユーザー可視の変更なし ✓)。

**1. 優先度が「表示後」も効くか = 効くようになった** ✓(私の指摘2の本体)
- `note_refusal` が `LAST_REFUSAL_MS` を立て(`hud.rs:48`)✓、`take_pending()` で `PENDING` が空になっても
  この時刻は残る ✓。`note_progress` は `PENDING` を触る前に `refusal_has_priority(LAST_REFUSAL_MS, now)` で
  早期 return する ✓ → **表示済みの断りは 2 秒間上書きされない** ✓。
- テストも意味が変わり、「断りを消費した直後の進捗は出ない(`take_pending().is_none()` ✓)」+
  純関数の境界(`1000→1500` true / `1000→3000` false / `0` false ✓)を固定 ✓。
- 変異: m1(純関数を `false` に固定)**赤** ✓ / m2(`note_progress` のチェック削除)**赤** ✓ = どちらも実効 ✓。

**2. 期限切れの副作用 = 長い転送の「表示」を途中で切ってしまう**(下の欠陥2)
- リードの理解は正しい ✓: 切れるのは**表示だけ**で、転送(`bulk` の受信・`incoming_drag` の状態機械)は
  触っていない ✓(`waiting_for_data` は HUD 用の `WAITING` を消すだけ ✓)。
- **ただし期限は「offer から30秒」であって「進捗が止まって30秒」ではない**: `WAITING` の時刻は
  `offer`(`incoming_drag.rs:111`)で一度だけ書き込まれ、`RX_BYTES` の進展では更新されない ✓
  (`WAITING` への書き込みは offer / `set_waiting_for_test` / clear / 期限切れのみ ✓)。
  → **1GB(34秒)や 10GB(341秒)の正当な転送でも 30 秒で進捗表示が消える** ✓
  = F2 が消そうとした「待っている間、手元に何も無い」を再び作る ✓(転送は無事 ✓)。

**3. `TxGuard` を先に落とした新しい競合 = 無し** ✓
- 待つ前に読む値: `released_early`(`files.rs:566`)✓ = 競合しない ✓。
- 待ちの中で読む値はすべて **id 検査付き** ✓: `lazy_accepted(id)`(`files.rs:1008-1010`)✓ /
  `wait_lazy_accepted` の `MANIFEST_SENT_ID != id` 早期 return(`:1024`)✓ /
  `wait_drag_done` の `got_id == id` ✓。
  → 新しい掴みが始まって静的値が書き換わっても、古い判定は**古い id の結果を拾わない** ✓
  (待ちはタイムアウトし、事前に読んだ `released_early` で判断する ✓ = 正しい)✓。

**4. 細かい点** ✓: `offer` 受理時に `note_rx_begin()`(受信量を 0 に戻す ✓ = 私の指摘4)✓ /
`body()` 削除 ✓(私の指摘6)✓。

**変異とフレーク・ストレス(私の手で実行。worktree は撤去済み)**

| 変異/実験 | 結果 |
|---|---|
| m1 断りの優先(純関数)を無効化 | **赤** ✓ |
| m2 `note_progress` の優先チェックを外す | **赤** ✓ |
| m3 期限判定(純関数)を無効化 | **赤**(`a_silent_wait_expires`)✓ |
| **m4 `waiting_for_data` の期限チェック(呼び出し側)を外す** | **生存** ← 純関数しかテストされていない(欠陥3) |
| **m5 `receive` の `clear_waiting` を外す** | **生存** ← 到着時の消去は未テスト(欠陥3) |
| m6 早期 `drop(guard)` を戻す | 生存(タイミングの意図はテスト対象外 ✓) |
| **m7 `offer` の受信量リセットを外す** | **生存** ← 未テスト(欠陥3) |
| s1 ロックを外して hud 4件を並列 20 回 | **2 回失敗** ✓ = ロックが効いている証拠 |
| s2 ロック有りで hud 4件を並列 30 回 | **0 回失敗** ✓ = フレークは直った ✓ |
| **s3 `incoming_drag` を並列 20 回** | **11 回失敗**(`cancelling_another_id_keeps_the_waiting_state` 8 / `cancelling_clears_the_waiting_state` 3)← **欠陥1** |

### 見つけた欠陥

1. **[中・テスト] 新設の `waiting_tests` が並列実行でフレークする。** 2つのテストが共有 static `WAITING` を
   ロック無しで触るため、片方の `set_waiting_for_test` が他方の `cancel` 対象を上書きする ✓
   (`incoming_drag` を並列実行して **11/20 失敗** ✓ 内訳は上記 ✓)。全体スイートは3回とも 608/0 だが、
   この実行条件では 55% で落ちる ✓ = **CI のスケジューリング次第で落ちる** ✓。
   → hud に入れた `HUD_TEST_LOCK` と同じテストロックを `waiting_tests` にも入れる ✓
   (前ラウンドで直したのと同じ種類のフレークを、同じコミットで新設している ✓)。
2. **[中] 期限が「進捗の停滞」ではなく「offer から30秒」で切れる** → 正当に長い転送(1GB=34秒、
   10GB=341秒)でも 30 秒で進捗が消える ✓。転送は継続するのでデータは無事 ✓ だが、F2 の目的
   (待っている間に手元で状況が分かる)を再び失う ✓。
   → `RX_BYTES` が進んだら時刻を更新する(停滞時間で切る)か、上限を大幅に伸ばす ✓。
3. **[軽] 配線が未テスト**(m4/m5/m7 が生存 ✓): 純関数(`waiting_alive`)は固定されているが、
   「呼び出し側が期限切れを消す」「到着(`receive`)で消す」「offer で受信量を戻す」はテストが無い ✓。
4. **[軽] `DRAG_UNFINISHED.store(false, …)` が `files.rs` に二重に書かれている**(`:561` と `:568`)✓。

## 6.10 第8ラウンド: 停滞ベースの期限(`6641762`)/ 判定: **合格**(軽微な残り2点)

- HEAD `6641762` / `git status` 空 / `cargo build` exit 0(**警告行 10 = 基準同数**)/
  `cargo test` = **610 passed / 0 failed / 4 ignored**(申告どおり ✓)。
- 文言: 新規リテラルは**ログ1行のみ** ✓(`[drag] 受け取り待ちが停滞したため表示を打ち切ります({id})`)。
  ユーザー可視の変更は無し ✓(差分を全文読んで確認 ✓ = 対象2ファイルともロジックとテストのみ ✓)。

**1. 停滞ベースの期限 = 正しい** ✓(私の欠陥2の本体)
- `WAITING_SEEN_MS`(最終進捗時刻)を `offer` で立て、`files::note_rx_bytes` → `incoming_drag::note_progress_seen()`
  で更新 ✓。切る条件は `waiting_alive(seen, started, now)` = **停滞15秒** または **開始から15分** ✓。
  `note_progress_seen` は `WAITING_SEEN_MS != 0` のときだけ書く = 「待ちが無いときは触らない」✓。
  `clear_waiting` / 期限切れ / `reset` のすべてで `WAITING_SEEN_MS` を 0 に戻す ✓(掃除も揃っている ✓)。
- **1GB=34秒の表示が消えない** ✓(テスト `a_silent_wait_expires_but_progress_keeps_it_alive` が
  「進捗が続く限り5分でも生きている」を固定 ✓)。

**2. 停滞の閾値(15秒)が極端に遅い回線で誤発火しないか = しない** ✓(コードで確認)
- 受信フックは **`bulk.rs:898-900` で「受信したブロックを書き込むたび」**に呼ばれる ✓
  (ファイル完了ごとではない ✓)。ブロックは送信側の `CHUNK`(=256KB)単位 ✓。
- したがってフック間隔 = 256KB ÷ 実効速度 ✓:
  30MB/s → 約 8.5ms / 1MB/s → 約 0.26秒 / 100KB/s → 約 2.5秒 / **17KB/s を下回ると 15 秒を超える** ✓。
  → **15秒の停滞は「実効 17KB/s 未満」または本当の停止を意味する** ✓ = 閾値として妥当 ✓
  (リードの見立てどおり ✓)。
- 例外として理論上残るのは「1ファイルのハッシュ検証(blake2)が15秒を超える」場合 ✓
  (最終チャンクの後・`receive` の前に無音になる ✓)。10GB 単一ファイルで約10秒程度 ✓ 通常は収まる ✓
  → 気になるなら**ファイル完了時にも `note_progress_seen`** を呼ぶのが安全 ✓(軽微)。
- **15分の保険** ✓: 10GB=341秒(5分41秒)なので余裕は約2.6倍 ✓ 妥当 ✓。
  ただし 10GB を 11MB/s 未満で流すと保険に当たる(表示だけ切れる ✓ 転送は無事 ✓)✓。

**3. `waiting_tests` の直列化 = 実効(11/20 → 1/20)** ✓ ただし**完全ではない**(下の残り1)
- 5テストすべてが `lock()` を取り ✓、私のストレスでも **並列12回で 0 失敗** ✓(hud 側も 0/12 ✓)。
- ただし `incoming_drag` 全体を並列 20 回で **1 回失敗** ✓(`cancelling_another_id_keeps_the_waiting_state`
  が :717「別IDでは消さない」で失敗 ✓)。原因は**モジュール外の `reset()`**:
  `mod tests` の `disconnect_keeps_native_resources_for_main_thread_cleanup`(`:601-607`)が
  `reset()` を呼ぶが、`WAIT_TEST_LOCK` は `waiting_tests` の中の private static なので**掛からない** ✓
  → その瞬間に `WAITING` と `WAITING_SEEN_MS` が消える ✓。

**4. 配線のテスト = 塞がった** ✓(私の m4/m5 が赤になった)
- `an_expired_wait_is_cleared_by_the_reader`(時刻を古くして `waiting_for_data` が消す ✓)
- `receiving_clears_the_waiting_state`(実際の `INCOMING.offer(99,1,0.0)` + `receive(99,[…])` ✓)
- `set_waiting_for_test` も「生きている待ち」を模すよう修正 ✓(前版の取り違えを解消 ✓)
- `DRAG_UNFINISHED` の二重 store も解消 ✓。

**変異とストレス(私の手で実行。worktree は撤去済み)**

| 変異/実験 | 結果 |
|---|---|
| base(`waiting_tests` 5 / `receive_progress` 2) | 緑 ✓ |
| m1 停滞の判定を外す | **赤**(`a_silent_wait_expires_but_progress_keeps_it_alive`)✓ |
| m2 全体の上限(15分)を外す | **赤**(同上)✓ |
| m3 `waiting_for_data` の期限チェックを外す | **赤**(`an_expired_wait_is_cleared_by_the_reader`)✓ = 前ラウンドの m4 が解消 |
| m4 `receive` の `clear_waiting` を外す | **赤**(`receiving_clears_the_waiting_state`)✓ = 前ラウンドの m5 が解消 |
| **m5 受信フックからの `note_progress_seen` を外す** | **生存** ← 残り2(フック→期限延長の配線が未テスト) |
| s1 `incoming_drag` を並列 20 回 | **1 回失敗**(上記。前ラウンドは 11/20) |
| s2 `hud::tests` を並列 12 回 | **0 回失敗** ✓ |

### 残り(いずれも軽微。判定は合格)

1. **[軽] ストレス 1/20 の残存フレーク**: `WAIT_TEST_LOCK` が `waiting_tests` の private static なので、
   `mod tests` の `reset()` を巻き込めない ✓。→ ロックをファイル共有(`#[cfg(test)]` の crate 内 static)に
   するか、`mod tests` の `disconnect_…` も同じロックを取る ✓。全スイート実行は3回とも 610/0 だが、
   負荷次第では同じ経路が踏める ✓。
2. **[軽] フック → 期限延長の配線が未テスト**(m5 生存): 純関数(`waiting_alive`)と読み手
   (`waiting_for_data` / `receive`)は固定されているが、「受信のたびに `WAITING_SEEN_MS` が延びる」ことは
   テストされていない ✓。→ `note_rx_bytes` を呼んで `waiting_for_data` が生き続けることを1件足す ✓。

## 6.11 第9ラウンド: Windows 側の W2-lite・W5-lite(+ 前ラウンドの残り2点)/ 判定: **合格**

**検証対象(マージ除く。範囲 `6641762..8b0dd03`)**: `682e9a3`(前ラウンドの残り2点)→
`1f5f6ba`(**W2-lite**: 30秒の無言取消をやめて理由を知らせる)→ `10d7c8b`(**W5-lite**:
「受信フォルダを開く」をトレイメニュー先頭へ)。

- HEAD `8b0dd03` / `git status` 空 / `cargo build` exit 0(**警告行 10 = 基準同数**)/
  `cargo test` = **611 passed / 0 failed / 4 ignored**(申告の 610 と一致。+1 は `682e9a3` の新テスト ✓)。
- **限界(重要)**: `cargo test -p knit-win --no-run` は macOS では **16 個のコンパイルエラー**で通らない ✓
  (`rustup` の Windows ターゲットも未導入 ✓)。したがって `1f5f6ba` の `send_wait_tests` 2件と、
  メニュー構築の実行は**私の手では動かせない** ✓ = Windows CI が唯一の実行検証 ✓(今回は CI pass の申告 ✓)。
  また **golden list は `crates/win` を走査していない**(§1.6 の限界)ため、Windows 側の文言は
  差分の読解で確認した ✓。

### `682e9a3`: 前ラウンドの残り2点 / **解消** ✓

- **共有ロックの範囲** ✓: `SHARED_STATE_LOCK` をファイル直下の `#[cfg(test)] pub(crate) static` にし、
  `waiting_tests` の `lock()` も `mod tests` の `disconnect_keeps_native_resources_…` も
  **同じ1本**を取る ✓ → 私が特定した「モジュール外の `reset()` が横から消す」経路が塞がった ✓。
  私のストレス再実行: `incoming_drag` 並列 **0/20 失敗**(前ラウンドは 1/20)✓ / `hud::tests` 並列 **0/12** ✓。
- **フック配線のテスト** ✓: `receiving_bytes_extends_the_wait` が実際の `files::note_rx_bytes(262_144)` を
  呼び、`WAITING_SEEN_MS` が前へ進むことと待ちが生き続けることを固定 ✓ →
  **前ラウンド生存した変異 m5(フックの `note_progress_seen` を外す)が赤** ✓
  (`incoming_drag::waiting_tests::receiving_bytes_extends_the_wait`)✓。
- 観察: `a_wait_without_any_progress_is_not_shown` の説明どおり、`now_ms()` は**プロセス起動からのミリ秒**
  なので「15秒前」を実時間で作ると 0(=未設定)に飽和する ✓ → 停滞そのものは純関数 `waiting_alive` 側で
  固定し、読み手側は `seen == 0` の場合を固定する、という切り分けは妥当 ✓
  (私の m4 相当=呼び出し側の削除変異は、この形でも赤になることを確認済み ✓)。
  軽微: テスト内で `WAITING_SEEN_MS` を2回続けて store しており、1回目は直前の2回目の store で
  上書きされる(意図のコメントはあるが実質不要)✓。

### `1f5f6ba`: W2-lite / **合格**(読解のみ。実行は Windows CI)

- **(a) 取り消す条件は変えていない** ✓: 変更前
  `(!enabled() || !at_target || !down || (!p.sending && p.began.elapsed() > 30s)) && !p.committed` →
  変更後 `… || timed_out) && !p.committed`(`timed_out = !sending && waited > SEND_WAIT`)✓
  **論理的に同一** ✓(`>` の境界も維持 ✓ テストが「ちょうど30秒はまだ待つ」を固定 ✓)。
  唯一の差は `p.began.elapsed()` が short-circuit されず**常に評価**されること ✓(µs のコスト ✓ 意味の変化なし ✓)。
- **(b) `timed_out` の立て方** ✓: タプルの3要素目を `cancel && timed_out` にしているため、
  **実際に取り消した AND 理由に時間切れが含まれる**ときだけ通知する ✓(`p.committed` が真なら
  `cancel=false` → 通知しない ✓ = 確定済みのドラッグに誤通知しない ✓)。
  軽微: 時間切れと他の理由(例: カーソルが外れた)が同時のときも時間切れとして通知する ✓
  (事実としては「30秒待った」は真 ✓ 誤りではない ✓)。
- **(c) 文言は実態と合っている** ✓: 「受け渡しが始まりませんでした(30秒待ちました)。ドラッグは取り消しました。
  もう一度掴んでください」= Win→Mac で Mac が受理しないまま時間切れ → Windows 側のドラッグが取り消される ✓
  という実態と一致 ✓ 設計 §5 W2 の「取消時は必ず通知し、理由(時間切れ)と次の一手を出す」を満たす ✓。
- **(d) `p.committed` の扱いは不変** ✓(`… && !p.committed` の位置・`finish` の式とも変更なし ✓)。
- テスト ✓: `a_wait_that_never_starts_expires`(29秒/ちょうど30秒/31秒 ✓)/
  `an_active_transfer_is_never_expired`(送信中は31秒でも6時間でも false ✓)
  = 設計の核「長い転送を打ち切らない」を固定 ✓(実行は Windows CI ✓)。

### `10d7c8b`: W5-lite / **合格**(読解のみ。実行は Windows CI)

- **(a) 機能は不変** ✓: 移動したのは `AppendMenuW` の**位置**だけで、ハンドラ
  `MENU_OPENFOLDER => { let dir = crate::receive_dir(); … }`(`tray.rs:910`)は無変更 ✓。
  `receive_dir()` の使用も維持 ✓(前ラウンドで一本化した経路 ✓)。
- **(b) 他の項目の相対順・区切りは不変** ✓: 旧位置(`MENU_BACKMAC` と「設定フォルダを開く」の間)から
  1件を抜き、第1セパレータの直後(= 最初に押せる位置)へ入れただけ ✓。セパレータの追加・削除は無し ✓
  (旧位置の前後にもセパレータは無い ✓)。設計 §5 W5 の「トレイメニューの先頭に出す」に一致 ✓。
- **(c) ID の二重使用なし** ✓: `MENU_OPENFOLDER = 1008` は1箇所で定義 ✓、`AppendMenuW` も
  **1箇所だけ**(`:1477`)✓ 旧 `:1535` の追加は削除済み ✓(grep で確認 ✓)。

### 実測まとめ

| 項目 | 結果 |
|---|---|
| HEAD / ツリー | `8b0dd03` / クリーン ✓ |
| `cargo build` | exit 0 / 警告行 10(基準同数)✓ |
| `cargo test`(既定メンバー) | **611 passed / 0 failed / 4 ignored** ✓ |
| `682e9a3` の残り2点 | 共有ロック ✓(0/20)/ フック配線 ✓(m5' が赤)|
| `knit-win` のローカル検証 | **不可**(16 コンパイルエラー / Windows ターゲット未導入)✓ = CI のみ |

### 実機なしで「実装だけ」進められる Windows 側の残り(lead の質問への回答)

優先順に、いずれも **純関数/テストを CI で回せる**形にできるものです ✓:

1. **`start_lazy` の開始条件を純関数化**(`should_start_lazy(lazy_enabled, drag_thread, files_len) -> bool` 相当)✓
   現在は `lazy_enabled` と `DRAG_THREAD != 0` と件数の判定が本体に混在 ✓ → 切り出せば CI で固定できる ✓
   (私が前ラウンドで指摘した二重開始ガードの回帰を CI で守れる ✓)。
2. **取り消し理由ごとの通知**(F10 の「無言」): `send_wait_expired` と同じ形で
   「モード離脱」「切断」「相手の拒否」を純関数化し、それぞれに通知を出す ✓ — `edge.rs` の
   `relay_cancel` 系は今も無言 ✓ 設計の「無言を作らない」文化に揃えられる ✓(CI で判定を固定 ✓)。
3. **`ManifestEntry.rel` の区切り変換**(`/` → `\`): `cFileName` に渡す直前で正規化する純関数 ✓
   CI でテストできる ✓(私が W1 検証で申し送った点 ✓)。
4. **`W2` 本体の 200ms 閾値 + ツールチップ進捗**: 「準備が 200ms を超えたら出す」を純関数にして、
   ツールチップ/状態行へ出す ✓ — 8MB 閾値の無言(F2)を消せる ✓ 表示の見た目だけが実機待ち ✓。
5. **`DRAG_THREAD` のリークガード**: `store` 〜 `DoDragDrop` の間に panic すると フラグが残り、
   以後の遅延提供が全部スキップされる ✓ → RAII ガードにする ✓(読解で直せる ✓)。
6. **`resolve_received` の `(n)` 探索上限**(1..100)を超えた衝突の扱い ✓ 純関数で CI テスト可 ✓。

**実機が必須で後回しが妥当**: W3 の層付きウィンドウ(HUD の見た目)、CFSTR_FILECONTENTS の
`IStream` 寿命(9/28 設計 段階3 の PoC)、CF_HDROP しか受けないアプリへのフォールバック挙動、
トーストのアクション(W5 本体)✓ — これらは「実装はできるが、確かめずに既定を反転させるのは危険」✓
(遅延提供が `KNIT_LAZY=1` の opt-in である限り安全 ✓)。

## 6.12 第10ラウンド: 実機なしで守れる Windows 側4件 + テスト土台 / 判定: **条件付き**

**検証対象(マージ除く。範囲 `6641762..61f2a34`)**: `fa84a97`(4件: 区切り変換・開始条件の純関数化・
リークガード・上限の文書化)→ `7222d8b`(CI 検出: `GetCurrentThreadId` の unsafe ブロック)→
`3bde9fd`・`237fb56`(テスト修正2件)→ `c256873`(スタブ adb の作業ディレクトリをプロセスごとに)。

- **コミットハッシュの訂正**: 指示の `59f4f8f` は存在せず、実際は **`fa84a97`** でした ✓(以降はこの rev で記載)。
- HEAD `61f2a34` / `git status` 空 / `cargo build` exit 0(**警告行 10 = 基準同数**)/
  `cargo test` = **611 passed / 0 failed / 4 ignored** ✓(macOS では `crates/win` のテストは走らない ✓)。
- **限界**: `crates/win` は macOS でコンパイルできない ✓ → Windows 側はすべて**読解+CI 申告** ✓。

### `fa84a97` + `7222d8b`: 4件のうち **3件は正しく、1件は未配線** ← 欠陥1

**(a) 区切り変換の位置 = 正しい** ✓(リードが警戒した混同は起きていない)
- 変換は `descriptor_bytes` の中の**ローカル変数** `let name = e.rel.replace('/', "\\")` だけ ✓
  (`lazy.rs:62`)→ `ManifestEntry.rel` 自体は `'/'` のまま ✓。
- 探査経路は元の `rel` をそのまま受ける ✓: `resolve_received(root, rel, …)` → `staged_path` ✓ は
  `rel.replace('\\', "/")` で**両方の区切りを受ける** ✓(`lazy.rs:89`、テスト `:252-258` が
  入れ子 `/`・`\` の両方を固定 ✓)→ **cFileName だけ `\`、実体探しは `/` でも `\` でも一致** ✓
  という要求どおりの分離 ✓。

**(b) 開始条件の純関数化 = 論理的に同一** ✓
- `lazy_start_allowed(enabled, drag_running, count) = enabled && !drag_running && count > 0` ✓ =
  従来の3つの早期 return の連言と同一 ✓(`drag_running` を先に読むが副作用は無い ✓)。
- テスト `lazy_start_needs_enabled_no_drag_and_a_non_empty_manifest` が4通りを固定 ✓ = CI で守れる ✓。
- 軽微: 従来「目録が空です」を出していた `eprintln!` が消え、空の目録では**無言**になった ✓
  (理由は `drag_running` のときだけ表示 ✓)。診断性が1段下がった ✓(1行で戻せる ✓)。

**(c) 【欠陥1】`DragThreadGuard` は定義されているが、どこからも使われていない**(未配線)✗
- `grep -rn "DragThreadGuard" crates/win/src/` = **定義4行のみ**(`:92` 構造体 / `:94-101` `arm()` /
  `:105-108` `Drop`)✓ で、`arm()` の**呼び出しはゼロ** ✓。
- `start_lazy` は今も生の `DRAG_THREAD.store(GetCurrentThreadId(), …)`(`:643`)と
  生の `DRAG_THREAD.store(0, …)`(DoDragDrop の後 `:672`)を使っている ✓
  → **意図した「panic してもフラグを残さない」保護は入っていない** ✓。
- 影響: `:643`〜`:672` の間(`LazySource::new` / `Box::into_raw` / `ensure_formats` / `DoDragDrop` 周辺)で
  panic すると `DRAG_THREAD` が非ゼロのまま残る ✓ → `lazy_start_allowed` が以後の目録を全部拒否し、
  **遅延提供が再起動まで黙って効かなくなる** ✓(以前直した `LAZY_STARTED` の漏れと同じ種類 ✓)。
- 修正: `start_lazy` のスレッド内で `let _g = DragThreadGuard::arm();` を使い、生の store と
  DoDragDrop 後の `store(0)` を置き換える ✓(下ろすタイミングは同じ ✓)。
  **半端に残すよりは削除した方が安全** ✓(「守られているように見える」状態が一番危険 ✓)。
- 補足: `7222d8b` は**この未使用の `arm()` のコンパイルエラー**を直したコミット ✓
  (ガードが呼ばれていないこと自体は CI でも警告止まり ✓)。

**(d) `resolve_received` の上限の文書化** ✓: 120 件の衝突を作り、上限内で見つからなければ
**宣言名を返す**(誤った実体を渡さない ✓)ことをテストで固定 ✓(`lazy.rs:322-332`)✓。

### `3bde9fd` + `237fb56`: テスト修正 / **合格**

- `3bde9fd`: cFileName の検証を添字ではなく**期待文字列 `"資料\\a.txt"` + NUL との比較**に変更 ✓
  = 区切り変換の意図をテスト自身が文書化し、終端まで検証する強い形 ✓(`\` を期待 ✓)。
- `237fb56`: 置換で消えていた `let second = …` を復元 ✓(CI が検出 ✓)。
- いずれも**テストのみ** ✓ 意図は保たれ、むしろ強化 ✓。

### `c256873`: スタブ adb の作業ディレクトリをプロセスごとに / **合格**(私の再現実験つき)

- (a) **テスト専用** ✓(`crates/mac/src/android.rs` の `mod tests` 内のみ ✓)。
- (b) **プロセス内の直列化は維持** ✓: 4つのスタブテストすべてが `STUB_TEST_LOCK` を取る ✓
  (`:1418` / `:1515` / `:1536` / `:1552`)✓。
- (c) **固定名に依存する他のテストは無い** ✓(`grep knit-android-stub` = 新しい PID 付き形式1箇所のみ ✓)。
  ディレクトリは `knit-android-stub-{pid}` / `{dir_name}-{pid}` ✓ になり、`remove_dir_all` を掛けるのも
  自分の PID の領域だけ ✓。
- **私の再現**: `cargo test -p knit-mac --bin knit-mac android::` を**2プロセスで同時実行** →
  **A: 39 passed / 0 failed / 2 ignored**、**B: 39 passed / 0 failed / 2 ignored** ✓ = リードの主張どおり ✓
  (修正前は 37/2 と 38/1 だったとの申告 ✓ と整合 ✓)。私が第3〜5ラウンドで報告した
  「2プロセスの同時実行で落ちる」フレークの根本原因がこれで塞がった ✓。

### 実測まとめ

| 項目 | 結果 |
|---|---|
| HEAD / ツリー | `61f2a34` / クリーン ✓ |
| `cargo build` | exit 0 / 警告行 10(基準同数)✓ |
| `cargo test`(既定メンバー) | **611 passed / 0 failed / 4 ignored** ✓ |
| adb 同時実行の再現 | 2プロセス × 39 passed / 0 failed ✓ |
| `DragThreadGuard` | **未使用 = 未配線** ✗(欠陥1)|

### まだ実機なしで「実装だけ」できるもの(更新版)

1. **【最優先】`DragThreadGuard` を実際に使う**(または削除する)✓ — 3行 ✓ 主張と実装を一致させる ✓
2. **空の目録のログを戻す** ✓(1行 ✓ 診断性 ✓)
3. **取り消し理由ごとの通知**(前回の提案2・未着手 ✓): モード離脱・切断・相手の拒否を純関数化して通知 ✓
   (`relay_cancel` 系は今も無言 = F10 ✓)CI で判定を固定できる ✓
4. **W2 本体の 200ms 閾値 + ツールチップ進捗**(前回の提案4・未着手 ✓)✓ 純関数+表示 ✓
5. **W3 を opt-in で入れる** ✓: 既定を変えないため `KNIT_HUD=1`(または既存の `KNIT_LAZY` と同様の
   スイッチ)で**暗いまま**出荷する ✓ — リードの判断(実機確認なしで既定を変えない ✓)を守りつつ、
   実機が来たらフラグを反転するだけで済む ✓ 判断材料は common の `ReleaseForecast` に既にある ✓
6. **`cFileName` の切り詰めがサロゲートペアを割る問題**(W1 検証の申し送り ✓): 259 WCHAR で切る際に
   非サロゲート境界へ丸める ✓ 純関数化して CI テスト可 ✓
7. 軽微: `st_stat` の `grfMode`/`pwcsName`(未設定のまま返す ✓)/ `resolve_received` の `(n)` 上限の扱いは
   文書化済み ✓
- **同意**: I9(PoC 前提 ✓)と W3 の既定反転(実機必須 ✓)はリードの判断どおり後回しで妥当 ✓。

## 6.13 第11ラウンド: ガード配線・サロゲート境界・拒否通知 / 判定: **合格**

**検証対象(範囲 `61f2a34..7ba2c25`)**: `745aa4c`(前回の指摘3件)→ `ad3715c`(受理前の拒否通知)→
`c256873`(スタブ adb のディレクトリをプロセスごとに。**前ラウンドで検証済み・内容も同一** ✓)→ docs。

- HEAD `7ba2c25` / `git status` 空 / `cargo build` exit 0(**警告行 10 = 基準同数**)/
  `cargo test` = **611 passed / 0 failed / 4 ignored** ✓。
- 限界は前ラウンドと同じ ✓(`crates/win` は macOS でコンパイル不可 ✓ = 読解 + CI 申告 ✓)。

### `745aa4c`: 指摘3件 / **すべて解消** ✓

- **(a) 今度こそ使われている** ✓: `crates/win/src/dragdrop.rs:646` に
  `let _drag_guard = DragThreadGuard::arm();` が **1箇所** ✓ あり、遅延提供の生の
  `DRAG_THREAD.store(GetCurrentThreadId(), …)` は**消えている** ✓。
  残る `DRAG_THREAD.store` は「ガード自身の `arm`(`:97`)」「ガードの `Drop`(`:107`)」
  「通常 start(`:728`/`:747` = 元から別経路 ✓)」のみ ✓ = 意図どおり ✓。
- **(b) DoDragDrop 後の明示 `store(0)`(`:672`)を残した判断は正しい** ✓:
  - これにより**下りるタイミングが従来と同一** ✓(DoDragDrop 直後に下ろし、
    後続の `lazy_com::release` / `src_release` / `drop(host)` の間だけ遅くする従来の挙動を維持 ✓)。
  - ガードは**panic 経路の保険**として二重に効く ✓(`:646`〜:672` の間で panic しても Drop が下ろす ✓)。
  - 二重 store(0) は冪等 ✓ 害なし ✓。
- **(c) `truncate_utf16` の境界は正しい** ✓:
  `out.truncate(max)` の後、**末尾が上位サロゲート(0xD800..0xDC00)のときだけ** pop する ✓
  → 下位サロゲートが末尾に残る場合は、その直前の上位サロゲートも同時に残っている
  = **完全なペア**なので pop 不要 ✓(`encode_utf16` はペアを連続して出すため ✓)。
- テスト `truncation_never_splits_a_surrogate_pair` が良い境界を作っている ✓:
  `"a"*258 + 🎉 + "tail"`(全264ユニット)を 259 に切ると 259 個目(= index 258)が**上位サロゲート** ✓
  → pop して **258 ユニット**になることを固定 ✓ + 「割れない場合は max まで」✓ + 「収まるときは触らない」✓。
- **空の目録のログ復元** ✓: `else if files.is_empty() { println!("[drag] 遅延提供: 目録が空です") }` ✓。
  `lazy_start_allowed` が false になる3理由のうち、**無効(既定)以外は必ず理由が出る** ✓
  (呼び出し元は `lazy_enabled()` のときだけ呼ぶので、空の目録での誤ログも出ない ✓)。
- 軽微: `for unit in truncate_utf16(…).into_iter()` の `.into_iter()` は冗長 ✓(挙動は同じ ✓)。

### `ad3715c`: 受理前の拒否通知 / **4点とも正しい** ✓

- **(a) 判定が「送信前のまま残っている」に限定** ✓:
  `refusal_notice_needed(pending_matches, sending, committed) = pending_matches && !sending && !committed` ✓
  と純関数化し、`refused_before_send(id)` は **同じ id の Pending が残っているときだけ** true ✓
  (`Some(p) if p.id == id` ✓ / 別IDは false ✓ = テストで固定 ✓)。
- **(b) `Msg::DragDone` では通知しない** ✓: 腕は `DragCancel | DragDone` の共通だが、
  条件に `matches!(msg, Msg::DragCancel { .. })` を入れて **DragCancel のときだけ**通知 ✓
  = Mac が置き終えた `DragDone` を「受け取れませんでした」と誤通知しない ✓。
- **(c) 他の取り消し経路で二重バナーにならない** ✓(経路を追って確認):
  - ローカルの取り消し(Esc・モード離脱・切断)は Windows 側が `cancel_current()` で
    Pending を **take してから** `DragCancel` を送る側 ✓ → この通知は「受信した DragCancel」でしか
    発火しないので**出ない** ✓。
  - Mac が受理後に失敗して送る `DragCancel` は、その時点で Windows 側の `p.sending == true` ✓
    → `!sending` が false で通知しない ✓(二重なし ✓)。
  - Mac の 30秒待ちの取り消し(W2-lite の通知 ✓)と競合する場合も、`edge::cancel(id)` が Pending を
    消した後は `refused_before_send` が false ✓ = 二重にならない ✓。
- **(d) 文言は実態と合っている** ✓: 「Macが受け取れませんでした(共有範囲外・容量超過・切断など)。
  もう一度掴んでください」= Mac 側 `offer` の拒否条件(`AVAILABLE` / `WIN_MODE` / `BTN_DOWN` /
  `HOTKEY_ONLY` / `share::allow_files` / `MAX_TOTAL` / 画面範囲 / `Incoming::offer`)✓ の主要因を
  列挙しており ✓ 次の一手もある ✓。
- **残る観察(本コミットの欠陥ではない)**: Mac が**受理した後**に受信で失敗した場合
  (`incoming_drag::receive` の Err 腕 → `DragCancel` 送信 ✓)は、Windows 側が `sending=true` なので
  この通知は出ず、Mac 側も通知しない ✓ → **その1経路だけ無言のまま** ✓(F9/F10 の残り ✓ 別タスク ✓)。

### 実測まとめ

| 項目 | 結果 |
|---|---|
| HEAD / ツリー | `7ba2c25` / クリーン ✓ |
| `cargo build` | exit 0 / 警告行 10(基準同数)✓ |
| `cargo test`(既定メンバー) | **611 passed / 0 failed / 4 ignored** ✓ |
| `DragThreadGuard::arm` | `dragdrop.rs:646` の1箇所 ✓(生 store は消えた ✓)|
| `truncate_utf16` | 上位サロゲートのみ落とす ✓(テストが 258 ユニットを固定 ✓)|
| `refusal_notice_needed` | 4通りをテストで固定 ✓ / DragDone は除外 ✓ |

### 判断を求められた3件への回答

1. **W3 を `KNIT_HUD=1` の opt-in で「暗く」出荷する案 → 条件付きで賛成**(優先度は中)。
   - 賛成の理由: 既定が off なら**出荷物の挙動は1ミリも変わらない** ✓(回帰リスクはゼロに近い ✓)。
     実機が来た日にフラグを反転するだけで価値が出る ✓。判断材料(`ReleaseForecast`)は既に common にあり ✓
     Windows 側は「命令を実行するだけ」の層 ✓ という設計の分離も崩さない ✓。
   - 条件: (i) スイッチは既存の `KNIT_LAZY` と同じ**環境変数ではなく設定**に置くなら、
     既定 off を**テストで固定**する ✓(うっかり既定 on にしない ✓)。
     (ii) 見えない UI は**腐る**ので、`allow(dead_code)` に理由と「反転手順」を書き、
     ランナップ(検証手順書)に**反転前チェックリスト**を1つ足す ✓。
   - ただし**順序は W2 の方が先** ✓(下記2 ✓)。W3 は層付きウィンドウの描画が本体で、
     実機なしでは「出る・消える・クリックを奪わない」が一切確かめられない ✓
     = **入れても検証できないコードが増えるだけ** ✓ なので、W2 で情報を出せる場所を増やしてから ✓。
2. **W2 本体(200ms閾値 + ツールチップ進捗)→ 今やる価値がある**(優先度は高)。
   - 判定は純関数(`should_show_wait(elapsed_ms) = elapsed_ms > 200` 相当 ✓)にできて **CI で守れる** ✓。
   - 表示先は既存のツールチップ/状態行(`tray.rs:410-416`)✓ = **新しい UI 機構を足さない** ✓
     ので、実機なしでも「配線が正しい」ことは読解で確かめられる ✓。
   - これで F2 の「8MB 以下は完全に無言」が消える ✓(現状の W2-lite は 30秒の取消時だけ ✓)。
3. **`st_stat` の `grfMode`/`pwcsName` → 今は触らない**(優先度は低。**私の前回の指摘は過剰でした**)。
   - `grfMode = 0` は **`STGM_READ`(0x00000000)そのもの** ✓ = 既に正しい ✓(直すものが無い ✓)。
   - `pwcsName` は Explorer が `STATFLAG_NONAME` で列挙するため実害なし ✓(リードの判断どおり ✓)。
   - `IStream::Stat` の呼び手が名前を期待する可能性は残るが、**未検証コードを触る価値は今は無い** ✓
     =(設計 §7「やらないこと」の精神 ✓)。実機で問題が出たら直す ✓ で良い ✓。

### 増分の状態(I1・I2・I3・I5・I6・I7・I8・W1・W4)

| 増分 | 依頼 | 判定 | 備考 |
|---|---|---|---|
| W1-glue(COM) | 受領(58240f1) | **合格**(読解+MSDN のみ) | vtbl 3式・STATSTG 80・STGMEDIUM 24・FormatEtc 32(offsets 正)・参照カウント・HGLOBAL・st_read を確認。軽微4点 |
| W1-glue 防御+ABI | 受領(e6b4035・dbccefc) | **合格** | 押下確認・フラグ漏れ・ABI テスト ✓。`e6b4035` の FormatEtc=24 は誤り(実測 32)→ `dbccefc` で修正 ✓ |
| W1-wire-mac | 受領(d07164a) | **合格** | move 前に目録作成 ✓ 版ゲート ✓ 中身を読まない ✓ |
| W1-wire-win | 受領(05a5cd7)→ **修正版(121b7b1)** | **合格** | 欠陥1(accept のタイミング)✓・欠陥2(tray の一本化)✓・低確率競合(DRAG_THREAD ガード)✓ すべて解消。二重開始の防止 ✓ / `resolve_received` 有界・安全 ✓ |
| 能力と受理 | 受領(5b84b02) | **合格** | 受理の事実で反転 ✓(mf1/mf2 赤)/ bit63 ガード ✓ / リセット ✓ |
| W4 | 受領(ef9c43b)→ **修正版(121b7b1)** | **合格** | ロック規律 ✓ 推測しない ✓ 成功語なし ✓。`PeerDidNotPlace` の文面 ✓(mo1/mo2/mo3 赤)/ `PlacedOnScreen` の要注意判定 ✓(mn3'/mn4' 赤)|
| I8 残り2点 | 受領(652afc4) | **合格** | 前ラウンドの残指摘2件が変異で赤 ✓ |
| その他 | I1〜I8・mtime・pairing・W1-core | 合格 | §6.5 の表を参照 |

### 増分の状態(I1・I2・I3・I5・I6・I7・I8)

| 増分 | 依頼 | 判定 | 備考 |
|---|---|---|---|
| I1 | 受領(43de2e2)→ 指摘修正(a420d11) | **合格** | 3件の指摘はすべて修正され、修正自体も変異(m10/m11)で実効性を確認 |
| I2 | 受領(da6129c)→ 指摘修正(53c6291) | **合格** | m8 の穴は私の手でも赤→復元で緑を確認。doc 申し送り3件も反映 |
| I5 | 受領(85fdecb)→ 指摘修正(c7aae32) | **合格** | 条件付き3点(言い切り・取りこぼし窓・無テスト配線)がすべて解消 ✓ |
| I3 | 受領(90bd8eb)→ **修正版(e5fb2f1)** | **合格** | `cell setWraps:` + `setUsesSingleLineMode:` で例外が消えたことをプローブで確認 ✓ / `cursor_screen` でマルチディスプレイ解消 ✓ / `orderOut:nil` ✓。**2行描画の目視は未実施** |
| I6 | 受領(5021c59)→ 修正版(e5fb2f1) | **合格** | 通常送信の成功でも記録が消えるようになり、二重送信の経路が塞がった ✓ |
| I7 | 受領(5cd0a42) | **合格** | 版ゲート・MIN_VERSION 据え置き・副作用なしを確認 |
| I8/I8b | 受領(2f3e61e・54cf983)→ 修正版(e5fb2f1) | **合格** | 過剰受信(ml2)・二重受理(受理直後)・`Accepted` の生死(ml6)は変異で赤 ✓。残り: `Streaming` 中の受理と `Accepted` からの `completed()` が未テスト(軽微) |
| 粘着バグ | 受領(c49f95d)→ 修正版(e5fb2f1) | **合格** | `press()` は `released_once` のみ消す ✓ 反例が回帰テスト化され、変異で赤 ✓ |
| mtime | 受領(1047f61・ecf68a7) | **合格** | 公開後の最終パスへ設定 ✓ macOS の fd 経路維持 ✓ |
| pairing テスト | 受領(2fbf21d) | **合格** | テスト専用 ✓ |
| W1-core | 受領(6bc0791) | **合格**(読解+一次資料のみ) | 592 バイト・オフセット・フラグが MSDN と一致。実行検証は不可 |
| W系その他 | 未受領 | — | 実機が要る(W1 配線・W2・W3・W4・W5)|

**このラウンドの横断的な所見**

- **「純関数+テスト」は定着したが、「呼び出し側の配線」が無テストという穴が繰り返し現れている**
  (I5 の c1〜c3 → I6 のリセット漏れ)。判定の在り処を common の純関数へ出し、
  **配線は1行にする**という形が有効(c7aae32 で実際に効いた)✓。
- 変異テストは「生存=テストの穴」とは限らない。L5/I2 の m1 は**到達不能な等価変異**であり、
  コードの冗長さを示すシグナルとして読む ✓。

---

## 7. 第1段階で気づいた懸念(実装前に共有しておく)

1. **【最重要】B6 は拒否ではない**(§2 T1)。設計 I1 の `Verdict::Refused{reason, next_step}` だけでは
   B6(送れない項目を落としたうえで**越境はする**)を表現できない。`Ready{note}` 系の値が要る。
   B6 を `Refused` に畳む実装は、**一部がリンク等の掴みが越境できなくなる挙動退行**になる。
   なお摩擦doc F1 と設計 §0.1 の補足は B6 を「拒否分岐」の1つとして数えている(`:385-389` スキップ)ので、
   実装役がそのまま8分岐=全部拒否と読む危険が実際にある。
2. **B8 の副作用順序**(§2 T2)。`FILE_TX_BUSY.swap(true)` は判定と送信の間にある。
   値を先に作って後で `swap` する形にすると二重送信の窓ができる。
3. **B4 の `sleep` は純関数に移せない**(§2 T4)。移すと `verdict_for` が 150ms ブロックする。
4. **文言の重複が既に2組ある**(§1.3 の注意)。「同じ理由は1箇所に」という方針で `bulk::send_error_label` を
   掴み経路へ流用すると、通常送信(⌘C)経路の文言まで変わる可能性がある。Tier B の**出現数**で検出する。
5. **I2 の優先順位**(X8)。設計 I2 の検証項目 (d) は「`peer_lazy == true` なら bulk 未完でも `WillPlace` 側に倒れる」
   と読めるが、`refusal` が確定している入力でこれを優先すると**嘘の予報**になる。拒否が優先であるべき。
6. **I3 の HUD は通知と別実装になりやすい**。同じ `Refusal` から組まれているか、
   文言が2箇所に分かれていないかを `filetext` の差分で見る。
7. **`cargo test` の既定メンバーは `crates/common` と `crates/mac` のみ**(`Cargo.toml:default-members`)。
   `crates/win` は私の検証範囲外で、**ビルドすら通ったことを確認していない**(§5 の W 系は未検証)。
8. 摩擦doc の `crates/win` 側の引用(`edge.rs:486-506` の30秒取消、`tray.rs:1537-1538`、
   `host.rs:34,71,95`)は**私も再確認していない**。設計 §9-6 と同じ未検証事項として残す。

---

## 8. 未検証の点(この doc 自身の限界)

- 目視・実機での確認(通知バナーの表示時間、HUD の可視性、Finder/Explorer の実挙動)は**一切実施していない**。
  検証は `cargo build` / `cargo test` / ソース差分 / 文字列抽出のみ。
- `crates/win` は読んでいない(制約により触らない)。Windows 側の文言・挙動は対象外。
- §1 の golden list は**文字列リテラルの静的な集合**であり、実行時に組み立てられる文言
  (`format!` の断片を連結した結果)の最終形は一致しない。バイト一致の判定は
  リテラル単位でしか行えない。
- 第1段階では基準値のハッシュのみを採取し、**`cargo build` / `cargo test` は
  2026-10-18 に verifier が自分で実行した**(§1.2 の実測 = 548 passed / 0 failed / 4 ignored、
  build 警告行 10)。申告値と一致したことを確認済み。
- 第2段階では、**各増分のコミットに対して同じコマンドを再実行**する。実装役の申告と
  1つでも数値が食い違えば、判定の前にその事実を報告する。
- 判定は常に「コミット」に対して行う。作業ツリーの未コミット変更は検証対象にしない
  (2026-10-18 時点で `crates/common/tests/drag_handoff.rs` に未コミットの変更があることを確認済み)。
