# Wave1-E セキュリティレビュー(第三者視点・辛口)

- 実施日: 2026-09-25 / エージェント型: security-reviewer
- レビュー対象: `~/ZCodeProject/seamless-desk`(読み取り専用・全ソース実読)
- レビュー範囲: crates/{mac,win,common}、scripts/、win-dist/、docs/

## 0. 総評

「Tailscale が暗号化するから平文 TCP+トークンで足りる」という設計判断自体は、**Tailscale が実際に唯一の到達経路であれば**成立します。しかし本レビューで確定したのは、**その前提を担保する実装が存在しない**ことです。具体的には:

1. 認証トークンが実運用では既定の弱い値 `"seamless-desk-dev"` で動いている可能性が極めて高い(設定経路が Windows 側に存在しない)
2. Mac のリッスンが `0.0.0.0`(全インターフェース)
3. この2つが合わさると「Tailscale の外から既知トークンで接続できる」状態が現実にある

さらに、Windows 側は SendInput=対話セッションでの任意操作(実質 RCE)を**「hello_ok を返してくれる何か」に対して無条件に開く**構造です。サーバ認証が片方向しかないため、経路制御が破れた時点で Mac だけでなく Windows も即座に落ちます。「動いているから正しい」の最たる例として、**認証は形だけ機能しており、実際の防御は Tailscale のルーティング偶然に依存している**のが実態です。

---

## 1. 脅威一覧(可能性 × 影響)

| # | 脅威 | 可能性 | 影響 | 根拠 |
|---|------|--------|------|------|
| T1 | 既定トークン `seamless-desk-dev` での運用継続(認証実質無効) | **高** | **重大** | mac/main.rs:834, win/main.rs:340-341, restart-mac.sh:30, run_sd.vbs:2, usage.md:96 |
| T2 | TCP:24900 が Tailscale 外(IF 追加/WiFi/LAN)に露出 | **中** | **重大** | mac/main.rs:964 (`0.0.0.0` bind) |
| T3 | 未認証メモリ DoS(巨大1行の無制限 read_line) | **中** | **中** | mac/main.rs:993, 1029 |
| T4 | Windows 側のサーバ認証欠如(hello_ok 無鑑査で入力注入開始) | 低〜中(経路乗っ取り時) | **重大** | win/main.rs:503-506, 412-427 |
| T5 | Mac クリップボード内容の窃取(T1+T2 成立時、ユーザーがコピーするたび自動送信) | **中** | **高** | mac/main.rs:1222-1247(接続中は changeCount 変化で自動送信) |
| T6 | 認証済み通信相手による Mac/Win クリップボードの任意書き換え(偽テキストのペースト誘導) | 低 | 中 | mac/main.rs:1057-1069, win/main.rs:653-665 |
| T7 | ClipData(base64 画像)の受信サイズ上限なし(メモリ消費) | 低 | 中 | mac/main.rs:1044-1056(Clip text の1MB制限あり、ClipData には無い) |
| T8 | hello トークン総当て(レート制限・遅延なし) | 低(Tailscale 内) | 中 | mac/main.rs:1000-1015 |
| T9 | Mac 侵害 → ssh 鍵経由で Windows へ任意コード配布・実行(横展開) | 低(Mac 侵害が前提) | **高** | deploy-win.sh:18-28 |
| T10 | Win 側の並行 writeln による JSON 行破損(画像送信中に pong 混入 → 誤切断) | **中**(画像同期使用時) | 低〜中 | win/main.rs:445-485 vs 507-510 |
| T11 | Win 側 read/write timeout なし → 半開接続時の sd-win 固着 | 低 | 低 | win/main.rs:406-407(set_nodelay のみ) |
| T12 | 不正 BMP が NSBitmapImageRep に到達(構造検証ほぼ無し) | 低 | 低(Apple パーサ依存) | mac/main.rs:207-230, 233-286 |
| T13 | ログインジェクション(hello_ok の name / Focus の title に改行・ANSI) | 低 | 低 | win/main.rs:504, 627, 648, deploy-win.sh:28(生ログをターミナルへ) |

---

## 2. 重要度高(確定した問題)

### H-1. トークン認証が実質的に無効: 既定値 `"seamless-desk-dev"` への静かなフォールバック

- `~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:834`
  ```rust
  let token = std::env::var("SEAMLESS_DESK_TOKEN").unwrap_or_else(|_| "seamless-desk-dev".to_string());
  ```
- `~/ZCodeProject/seamless-desk/crates/win/src/main.rs:340-341` 同一フォールバック。

**環境変数を設定しなくても黙って起動し、文書化済みの固定トークンで認証します**(usage.md:96 に既定値が明記 = 公開情報と同義)。さらに運用実態を確認すると:

- `~/ZCodeProject/seamless-desk/scripts/restart-mac.sh:30` は `SEAMLESS_SWITCH_MODE` しか渡しておらず `SEAMLESS_DESK_TOKEN` を設定しない
- Windows 側は `~/ZCodeProject/seamless-desk/win-dist/run_sd.vbs:2` が `cmd /c sd-win.exe >> log 2>&1` と起動するだけで、**トークンを渡す仕組み(VBS/bat/install.bat/schtasks のいずれにも)存在しない**
- 共通資料(docs/agent-rebuild-prompt.md:40)は「.env から供給」と主張するが、**`.env` を読むコードはリポジトリのどこにも存在しない**(dotenv クレートなし、自前パーサなし。`.gitignore:3` に `.env` があるだけ)

つまり「コミットはしないが環境変数で供給」という主張に対し、供給経路の一方(Win)は完全に欠落し、他方(Mac)はスクリプトが設定を忘れている。**本番が現在教育されているなら、両バイナリとも既定トークンで動いているはずです**(ログ `[conn] established` が token 設定なしで出ているなら確定)。これは「.env から供給」という資料記述とコードの直接的な食違いでもあります。

**影響**： 認証が存在しないのと同等。T2(後述)と組み合わせるとネットワーク到達可能者全員が正規クライアントになれます。

**対策**：
1. フォールバック廃止。環境変数が無ければ `[fatal]` で起動失敗させる(10行程度の変更)
2. Windows 側は `run_sd.vbs` をやめ、トークンを渡せる起動方式へ(例： schtasks タスク側に `/D SEAMLESS_DESK_TOKEN=...` 相当を渡す、または `setx SEAMLESS_DESK_TOKEN` でユーザー環境変数に設定する手順を install.bat に追加)
3. トークンは 128bit 以上のランダム生成(`openssl rand -hex 32` 等)

**壊すリスク/検証**： 既定値依存のテスト(echo_client.ps1 等の旧テスト)は接続できなくなる。変更後 `verify.sh` の established/クリップ双方向が通ることと、`[fatal]` 出力の単体確認で検証。

### H-2. Mac のリッスンが全インターフェース(`0.0.0.0:24900`)

- `~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:964`
  ```rust
  let listener = match std::net::TcpListener::bind(("0.0.0.0", port)) {
  ```

Mac は WiFi(自宅 AP、AP 隔離あり)+ Tailscale の複数 IF を持つと推定されます。AP 隔離が「端末間ユニキャスト」まで完全に遮断するかは**未検証**です(AP 隔離は実装によってブロードキャストのみ、あるいは Mac 発の接続のみ不通で受信は通るケースがある)。`0.0.0.0` bind は「たまたま AP 隔離に守られている」状態であって、設計による防御ではありません。カフェの hotspot や USB Ethernet アダプタを刺した瞬間に LAN 全体へ露出します。

**影響**： T1 と合成した場合、(a) Mac クリップボードの窃取(Mac 側は接続中 `changeCount` 変化でコピー内容を自動送信、mac/main.rs:1222-1247)、(b) `Return` によるモード強制復帰・カーソル操作、(c) `Clip`/`ClipData` によるクリップボード偽造、(d) 正規 Windows の接続占有妨害(accept が直列処理のため、1接続が占有すると正規接続は accept されない)。

**対策**(いずれも小変更)：
1. bind を Tailscale IF の IP に限定(現行構成なら `100.100.10.9`。win/main.rs:375 の接続先既定値と一致)
2. さらに accept 直後の peer アドレスで allowlist(Windows の Tailscale IP 以外は hello を読む前に即断)
3. Tailscale ACL で port 24900 を当該2ノード間に限定(設定側の防御)

**壊すリスク/検証**： Mac の Tailscale IP 変更時(Tailscale 再インストール等)に起動失敗する可能性→ `[fatal] listen ... failed` を見れば即原因特定可能。verify.sh の established で検証。

### H-3. 未認証でのメモリ DoS: read_line に行長上限がない

- `~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:993`(hello 待ち)および `:1029`(受信ループ)

`String::new()` に対する `read_line` は**改行が来るまで、または read_timeout(12秒)まで際限なく String を伸長**します。認証前(hello 未送信)の接続でも到達可能で、攻撃者は改行なしの巨大データを流し続けるだけで、Mac 側にネットワーク帯域 × 12秒分のメモリ確保を強制できます(ギガビット WiFi 環境なら 1.5GB 級)。`Msg::Clip` の 1MB 上限(mac/main.rs:1058)は**デコード後のチェックであり、読み込み段階の防御ではありません**。

**対策**： 1行上限(実用上 8MB 程度で十分。画像 base64 5MB 上限に対し余裕を持たせる)を `read_line` 前に設ける。`BufReader<TcpStream>` なら `reader.take(limit)` と組み合わせるか、`Read::take` + `read_until(b'\n')` で手動上限チェック。上限超過で接続切断。

**壊すリスク/検証**： 上限を画像同期の実用最大(5MB base64 ≒ 5MB 文字列)より大きく設定すれば既存機能は無影響。巨大行送信でプロセスメモリが伸びないことをアクティビティモニタで確認。

### H-4. Windows 側にサーバ認証がない: hello_ok を返す任意のエンドポイントが sd-win を操作できる

- `~/ZCodeProject/seamless-desk/crates/win/src/main.rs:503-506`
  ```rust
  Msg::HelloOk { name, w: mw, h: mh } => {
      println!("[hello] ok from {name} ...");
      hello_done = true;
  }
  ```

`hello_done` は「何らかの応答があった」ことしか検証しません。hello_ok にはトークン由来の証跡が一切ありません。sd-win は hello_ok 後、`Key`(任意 VK)、`MouseAbs`/`MouseButton`/`Scroll`(SendInput)、`Warp`(SetCursorPos)、`Focus`/`Minimize`(任意ウィンドウ操作)、`Clip`(クリップボード書込)を**無条件実行**します。対話セッションのユーザー権限で Win キー→文字入力→PowerShell 起動まで可能で、**実質リモートコード実行**です。

現状の到達可能性は「`--host` の向け先変更」か「Tailscale 経路の乗っ取り」に限定され、Tailscale 前提では可能性は低い(推測ではなく構造の指摘として提示)。ただし macOS 側 `notify()` の osascript 実行などと異なり、**侵害時の爆発半径が最大**の経路であることは確定しています。

**対策**(Tailscale 前提を維持した範囲で)：
1. Mac 側の hello_ok に `hmac_sha256(token, client_nonce)` を含め、Win 側で検証する(チャレンジ・レスポンス化)。hello に nonce フィールド追加、プロトコル VERSION を 2→3
2. 短期的には最小限として、Win 側で「hello_ok の受信元が接続先アドレスと一致すること」(当然成立するが流程の明示)と、接続先を引数/環境変数でのみ指定可能とし既定 IP のハードコード(win/main.rs:375)を残さない運用

**壊すリスク/検証**： VERSION を上げるため旧バイナリ混在で早期切断される(設計意図通り)。HMAC 検証失敗時に hello_done が立たないことの単体試験と、verify.sh の established で検証。

---

## 3. 重要度中

### M-1. ClipData(Mac 受信)にサイズ上限がない

- `~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:1044-1056`

`Msg::Clip { text }` には `text.len() <= CLIP_MAX_BYTES`(1MB)がありますが、`Msg::ClipData { data }` の base64 文字列には上限がなく無制限に `b64::decode` されます。対称性の欠落です。認証後の脅威とはいえ、H-3 と同じく「読み込み後デコード」でメモリを2重確保(base64 String + Vec<u8>)します。**data 文字列長の上限(8MB 程度)をデコード前にチェック**してください。

### M-2. Windows 側に不要な inbound ファイアウォール許可(出所は remoteip 無制限)

- `~/ZCodeProject/seamless-desk/win-dist/install.bat:10-11`

sd-win は listen しない(outbound 接続のみ)ため、`netsh advfirewall ... dir=in ... localport=24900` の受信許可は**不要な穴**です。design.md:88 に記載がありますが Phase 1 時代の名残で、現在の逆転構成では意味がなく、誤解のもとです。削除を推奨(既存ルールの delete も追記)。

### M-3. Win 側の並行書き込みによる JSON 行破損(「たまたま動いている」の典型)

- `~/ZCodeProject/seamless-desk/crates/win/src/main.rs:445-485`(クリップ監視スレッドが `cb_writer` を保持)と `:507-510`(メインループの pong)、`:690`(Return 通知)

`try_clone` した同一 TcpStream へ2スレッドが並行で `writeln!` しています。Mac 側は単一送信スレッド+チャネルで正しく直列化している(mac/main.rs:924-957)のに、**Win 側にだけ同じ仕組みがない**。`writeln!` はフォーマット断片ごとに複数回 `write` を呼び得るため、画像同期(数MBの base64)送信中に pong や Return が混入すると行が破損します。破損行は Mac 側 `decode` 失敗で静かに捨てられるだけなので**普段は見えず、pong が欠けた時にだけ Mac 側の 10秒切断判定(mac/main.rs:944)が発火し、原因の掴めない切断/再接続ラッシュとして現れます**。まさに「たまたま動いている」箇所です。

**対策**： Mac と同じ mpsc + 単一送信スレッドへ統一(Win の書き込み口を3箇所→1箇所に集約)。
**検証**： 画像コピー中に高頻度スクロールを入れて Mac 側ログの `[conn] lost` 有無を確認。

### M-4. Win 側に read/write timeout がない(半開接続で sd-win が固着)

- `~/ZCodeProject/seamless-desk/crates/win/src/main.rs:406-407`(set_nodelay のみ)

Mac 側は read timeout 12秒+ping/pong(mac/main.rs:983, 940-948)で半開を撥ねていますが、Win 側は `reader.lines()` にタイムアウトがなく、相手消滅時は TCP 再送タイムアウト(概ね10〜15分)まで固着します。ユーザー入力の閉じ込めは Mac 側の切断処理(mac/main.rs:1084-1086)が救済するため致命的ではありませんが、設計資料の「切断検知の遅れ、半開接続」懸念は **Win 側では未対策**です。`serve()` 冒頭に `set_read_timeout(12s)` を追加するだけで、ping が3秒間隔で来る現運用と整合します。

### M-5. デプロイ SSH 鍵が実質無制限のリモート実行権(横展開経路)

- `~/ZCodeProject/seamless-desk/scripts/deploy-win.sh:18-28`(schtasks /Create /TR、taskkill、type)

`ssh -o BatchMode=yes home` が通る=Mac の鍵で Windows の owner アカウントに任意コマンド実行が可能です。sd-win の SendInput と異なりファイアウォールもデスクトップ制限もありません。開発効率上必要な経路ですが、**Mac 侵害が即 Windows 侵害に連結する**ため、爆発半径の意味では本体内のどの脆弱性より大きい。最低限、(a) デプロイ専用の鍵ペアに分離、(b) Windows 側 authorized_keys に `from="100.100.10.9"` 制限、(c) 鍵のパスフレーズ運用(あるいは ssh-agent)を推奨します。

### M-6. Win の画像読み出しでサイズチェックが b64 エンコード後

- `~/ZCodeProject/seamless-desk/crates/win/src/main.rs:452-456`

`GlobalSize` の全コピー→base64 エンコード(1.33倍の String)→**その後** 5MB チェック。巨大スクリーンショットで一時的にメモリが数倍化します。`GlobalSize` の値で先に上限チェックしてください(ローカル由来のため攻撃ではなく品質問題)。

### M-7. hello 認証の総当て耐性(レート制限なし・非定数時間比較)

- `~/ZCodeProject/seamless-desk/crates/mac/src/main.rs:1000-1015`

不一致で即 `continue`、遅延も接続数制限もありません。`t == token` の String 比較は非定数時間ですが、リモートからのタイミング測定は TCP/JSON のノイズに埋もれるため**実用攻撃にはならないと評価します(推測)**。ただしレート制限(N秒に1回の hello 検証、失敗ペナルティ)は H-1/H-2 と合わせて講じる価値があります。

---

## 4. 重要度低 / 指摘のみ

- **L-1 osascript へのエスケープ不完全**(`crates/mac/src/main.rs:312-324`): `"` を `'` へ置換するだけ。現状は固定文字列("Windows に接続しました" 等)のみで**インジェクション経路は存在しない**が、将来クリップ内容や相手名を通知に流す瞬間にコマンドインジェクションに化けます。引数を stdin 渡し(`osascript -`)へ変更を推奨。
- **L-2 `--test2` E2E コードの常駐混入**(`crates/mac/src/main.rs:1092-1157`): フラグ1つで notepad へのタイピングとファイル保存まで実行するコードが本番バイナリに含まれる。機能しない場合の害は低いが、GUI 昇格時のモジュール分割で必ず除外すること。
- **L-3 ログインジェクション**(`crates/win/src/main.rs:504, 627, 648`): hello_ok の `name`、Focus/Minimize の `title` をそのまま println。通信相手が改行/ANSI を含めると sd-win.log に偽ログ行や制御シーケンスを注入可能(deploy-win.sh:28 が生の `tail -2` をターミナルへ出すため、ターミナル側での OSC 52 等の影響も理論上あり)。制御文字の除去を推奨。
- **L-4 ログの置き場所**： Mac 側 `/tmp/sd-mac-run.log`(restart-mac.sh:30)。umask 次第で他ローカルユーザーに読まれる。トークンは載っていない(確認済み)が操作ログ・画面構成が漏れる。単一ユーザー運用なら影響なし。
- **L-5 VBS 起動ではログローテーションが効いていない**： `run_sd.bat:4-5` に1世代ローテーションがあるのに、`run_sd.vbs:2` は直接 exe を叩くため**未使用**。sd-win.log は無限増殖します(機能インベントリの「ログローテーション」観点への回答: 実装はあるが効果なし)。
- **L-6 トークンのメモリ上の滞留**： String としてプロセス生存中保持、zeroize なし。コアダンプ/メモリフォレンジックで読める。ローカル攻撃者前提の低リスク。対応は優先不要。
- **L-7 不正 BMP が NSBitmapImageRep へ到達**(`crates/mac/src/main.rs:207-230`): `len >= 40` 以外に構造検証なし(biSize/clr_used の整合無検証)。Rust 側のパニック経路は確認できず(インデックスは安全、`with_capacity` は dib.len() ベース)、Apple のパーサが最終防衛。OS 最新化に依存する旨を残リスクとして明記すべきです。
- **L-8 ハードコード IP**(`crates/mac/src/main.rs:827` 未使用 `_host`、`crates/win/src/main.rs:375`): 漏洩影響は軽微だが、`_host` はデッドコードのため削除推奨。
- **L-9 hello の w/h 検証が `> 0` のみ**(`crates/mac/src/main.rs:1004-1005`): 極端値で WIN_CUR スケールが歪む程度(クラッシュなし)。上限(例 16384)の追加を推奨。

---

## 5. 共通資料・docs とコードの食違い(指示に基づく指摘)

| 項目 | 資料の記述 | コード実測 | 判定 |
|---|---|---|---|
| トークン供給 | 「.env/環境変数から供給」(agent-rebuild-prompt.md:40) | .env を読む実装は存在しない | **資料が誤り(H-1)** |
| ping/pong | 「5秒毎/15秒切断」(資料・design.md:78-79) | 3秒毎/10秒切断(mac/main.rs:940-948) | design.md が旧情報。usage.md:68-70 は正しい |
| 再接続バックオフ | 「max5s」(資料・design.md:78) | max 3秒(win/main.rs:402) | design.md が旧情報 |
| ダブルタップ窓 | 資料は700ms(正)、usage.md:94 は「500ms以内」 | 700ms(mac/main.rs:650) | **usage.md が誤り** |
| 復帰ガード | 資料は400ms(正)、usage.md:20 は「250ms」 | 400ms(mac/main.rs:481) | **usage.md が誤り** |
| ライブ同期間隔 | usage.md:44「32イベントに1回」 | 16イベント毎(mac/main.rs:616) | usage.md が誤り |
| IME 実装 | usage.md:29-35「ImmGetContext/ImmSetOpenStatus」 | ImmGetDefaultIMEWnd + WM_IME_CONTROL(win/main.rs:75-96) | usage.md が旧実装のまま。ログ例(`ImmSetOpenStatus(true) ok`)も実出力(`WM_IME_CONTROL open=true -> sent`)と不一致。verify.sh:88 は `[ime]` の grep のためこの食違いを検出できない |
| 役割記述 | design.md:97「sd-mac: TCP クライアント」 | サーバ(listen) | design.md が Phase 1 時点の旧記述 |
| firewall 必要性 | design.md:88「TCP 24900 を許可」 | Win は listen しないため inbound 不要 | M-2 参照 |

docs の数値ズレは実害より「検証の根拠として docs を引用できなくなる」問題です。今回の再構築で design.md の接続・安定性セクションは実装ベースへの全量書き直しを推奨します。

---

## 6. 即効ある対策(優先順位)

1. **[30分] 既定トークン廃止**(H-1): フォールバック削除+起動失敗。Win 側トークン供給手順の追加(setx)。`verify.sh` に「ログに token 未設定警告が出ていないこと」のチェックを追加
2. **[30分] bind 先限定 + ピア allowlist**(H-2): `0.0.0.0` → Tailscale IP(100.100.10.9)。accept 時の peer アドレス検査
3. **[1時間] read_line 行長上限**(H-3): 8MB 上限で超過時切断
4. **[30分] ClipData 上限**(M-1): base64 文字列長 8MB
5. **[30分] install.bat の inbound ルール削除**(M-2)
6. **[2時間] Win 送信の直列化**(M-3): mpsc+単一送信スレッドへ統一。変更後は画像コピー+スクロール同時の動作確認が必須(壊すリスク: 送信順序変化で hello 再送ロジックの見直しが必要になる可能性)
7. **[10分] Win の read timeout**(M-4): 12秒設定
8. **[設計] hello_ok のチャレンジ化**(H-4): VERSION 3 で HMAC 導入。旧バイナリ混在の早期検知という設計意図を踏襲
9. **[運用] Tailscale ACL で 24900 を2ノード間に限定** + デプロイ SSH 鍵の分離(M-5)

---

## 7. Tailscale 前提を維持した範囲での推奨アーキテクチャ

- 認証は現状の PSK を維持しつつ、「**フォールバックなし・両側での相互証明**(hello に nonce、hello_ok に HMAC)・1行上限・ピア allowlist」の4点を最小セットとして導入。TLS は不要という設計判断は、この4点が入れば妥当と評価します
- Mac は入力を受信しない構造(Mac→Win 一方向の入力、Win→Mac は Return/Clip のみ)であり、これは**侵害時の爆発半径が Windows 側に集中する良い設計**です。再構築時もこの非対称性を明示的に残すことを推奨します
- sd-win の SendInput は「認証済み単一ピアのみ」をコードで強制(現在は--host の指定値+hello_ok の2段階だが後者が無鑑査)。これが「接続可否の制御は十分か」への回答です: **不十分。ピン留めは接続先アドレスの既定値のみで、応答者の認証がない**

## 8. 検証サイクルへの組み込み

既存 `verify.sh` は通信内容の健全性を検証できません。追加提案: (a) `nc` での巨大行送信→sd-mac のメモリが平坦であること(H-3)、(b) 不正トークン hello→即切断とログ(H-1)、(c) Tailscale IP 以外からの bind 到達不可(H-2、別 IF を付けた時のみ検証可能)、(d) docs 数値の自動照合(コメントの定数と実装定数の一致をテストコードで固定)。

## 9. 残存リスク(対策後も残るもの)

- Tailscale テールネット内の他ノード、または Tailscale アカウント侵害者が経路を握れば Windows は陥落(H-4 完全実装で軽減、PSK 共有者は残る)
- Mac のアクセシビリティ権限(CGEventTap)を持つローカルプロセスは sd-mac と同等の入力制御が可能(構造上回避不可)
- 画像パースは NSBitmapImageRep(Apple)依存、OS パッチ依存
- SSH デプロイ経路の完全廃止は開発効率とトレードオフ。鍵分離+from 制限が現実的な線

以上。最重要は H-1(トークン運用の実態)と H-2(0.0.0.0)の合成リスクであり、いずれも小修正で封じ込め可能です。認証と経路の2層が「たまたま Tailscale と AP 隔離に守られているだけ」の状態から、「コードで担保された状態」へ移すことが、GUI 昇格前の必須作業と判断します。

---

## 10. 【統合時追記 2026-09-25】オーケストレータによる実機検証結果

Wave2 統合時にオーケストレータが実機確認したところ、**H-1 の懸念は現実に確定**:

- 稼働中 sd-mac プロセスの環境変数に `SEAMLESS_DESK_TOKEN` は**未設定**
- リポジトリに `.env` ファイルも存在しない
- つまり本番は既定トークン `seamless-desk-dev` で認証運用されている

H-1・H-2 は「可能性」ではなく「現在進行形」のため、統合優先リストでは P0(最優先)に格上げ。
