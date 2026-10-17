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

### I1〜I3 の状態

| 増分 | 依頼 | 判定 | 備考 |
|---|---|---|---|
| I1 | 受領(43de2e2)→ 指摘修正(a420d11) | **合格** | 3件の指摘はすべて修正され、修正自体も変異(m10/m11)で実効性を確認 |
| I2 | 受領(da6129c) | **合格** | 欠陥は「テスト欠落2件・doc 1件」で挙動は正しい。X8/X9 は変異で実効を確認 |
| I3 | 未受領 | — | §7-5 の `TooMany` 粒度、HUD と通知の同一値性、tap スレッド UI を突く |

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
