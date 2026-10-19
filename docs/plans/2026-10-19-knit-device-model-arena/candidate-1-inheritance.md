# 端末別認証と登録台帳 — 候補1「マスター台帳 + 端末別導出トークン」(継承スタンス)

ランナー1。Noise NNpsk0 と `psk(token) = Blake2s256("knit-psk-v1\0"+token)`(secure.rs:15-21)、登録経路の `serve_credential` が `{token, port}` を渡す形(pairing.rs:397-421)を**一切変えずに**、配るトークンを端末別にする最小拡張。プロトコルメッセージ(Hello/Cfg 等の JSON)は 1 バイトも変わらない。版互換は「配る側が決める」構造で自動的に保たれる(後述)。

---

## Problem

Knit の信頼モデルは「全端末で同一の 256bit 共有トークン1本」。登録は認証済み経路(PAKE/SAS)でその同じトークンを配るだけ。よって (1) サーバは接続者がどの端末かを鍵から区別できない(ハンドシェイク成否は同じ)、(2) 登録台帳がなく、失効は「全登録の初期化」=全端末締め出し(setup.rs:413-470)しかない、(3) 未払い指摘 M2(端末単位失効)が解けない。

形状を自明でなくしている制約(Phase A の ground より):

- `secure::accept` は PSK を1本しか試せない。待受は本線 24900・音声 24901・bulk 24902 の3経路 × Mac/Win 両 OS(mac/session.rs:424, mac/audio.rs:1425, bulk.rs:1416, win/conn.rs:287)
- **bulk と本線は双方向**。Mac が接続側になる経路(client_attempt: session.rs:712, bulk connect_loop: bulk.rs:1476)があり、この時 Mac は「宛先端末の鍵」で connect する必要がある
- hello の id は認証されない(proto の自己申告ID。鍵の根拠にできない)
- v11..v17 相手との相互接続を壊せない(ハード制約1)。Windows ターゲットはこの Mac でコンパイルできない(同3)
- discover の room_id はトークン由来(discover.rs:12-21)。トークンが端末別になると、そのままでは登録済み端末の発見が壊れる
- discover::respond は Win 側も呼ぶ(win/main.rs:540)。応答側のマルチルーム化が必要なのは「台帳を持つ Mac」だけ

## Usage (caller's view)

usage が仕様。以下の呼び出し側コードから型を導いた。

### 呼び出し箇所 0: mac/main.rs — 起動時に台帳を1つ作り、全経路へ Arc で配る

現状の master トークン解決(mac/main.rs:829-846: KNIT_TOKEN ≥32文字 → credentials::load() → 初回登録UI)は**そのまま**。その直後に台帳を構築する:

```rust
// mac/main.rs(現状の token 解決の直後)
let token = /* 変更なし: KNIT_TOKEN / credentials::load / first_run */;

// master を種に台帳を開く。初回は空の台帳 + legacy エントリ(master そのもの)が暗黙に載る
let registry = std::sync::Arc::new(knit_common::registry::DeviceRegistry::open(
    knit_common::envutil::config_dir().unwrap(), &token,
)?);
// 以後、待受・招待・失効はすべてこの Arc を共有する(渡すのは1個のオブジェクトだけ。
// 鍵のリストも master も呼び出し側は再び意識しない)
```

### 呼び出し箇所 1: mac/session.rs:424 — 本線待受(accept)

```rust
// 変更前: let (r, hw) = secure::accept(stream, token, b"knit-main")?;
// 変更後:
let (r, hw, who) = secure::accept(stream, &registry.accept_keys(), b"knit-main")?;
// who: secure::Who — Legacy(従来トークン)か Device(key_id)(端末別鍵)か。
// hello 検証の後に PeerEntry へ格納し、失効と UI の紐付けに使う
```

音声(mac/audio.rs:1425)も同じ 1 行の差し替え。bulk の待受(bulk.rs:1416)は Endpoint が鍵を持つ(後述)。Windows 側(win/conn.rs:287)は自分のトークン1本のまま:

```rust
// win/conn.rs — ServerKeys::single で包むだけ(差分1行)
let (r, mut wr) = secure::accept(stream, &secure::ServerKeys::single(token), b"knit-main")
```

### 呼び出し箇所 2: mac/session.rs:712 / bulk — Mac が接続側になる時の鍵解決

Mac から Win へ本線/bulk を張る時は「宛先端末の鍵」で connect する。宛特定は hello の device_id(既存の PEERS 紐付け)を使い、registry が鍵を解く:

```rust
// 変更前: client_attempt(s, &token, ...)
// 変更後: token は registry が宛先別に解く(未紐付けの宛先=旧端末なら master そのもの)
let out = registry.outgoing_token(&active_peer_device_id());
match client_attempt(s, &out, ...) { ... }
```

### 呼び出し箇所 3: pairing.rs serve_credential — 登録時の資格発行

```rust
// 変更前: channel.send(&Credential { token: token.into(), port })
// 変更後: 認証済み経路で「この端末専用」の鍵を発行して渡す。
// Credential の JSON 形状 {token, port} は不変(旧 client がそのまま読める)
let enrolled = registry.enroll(&client_name)?;
channel.send(&serde_json::to_vec(&Credential { token: enrolled.token, port })?)?;
// enrolled.token は64桁hex → 旧 client_credential の parse_key をそのまま通る
```

Windows/Android 側の `client_credential`(pairing.rs:687-713)は**無変更**。渡されるトークンが64桁hexである限り、parse_key → credentials::save(単一スロット)の経路は何も変わらない。

### 呼び出し箇所 4: mac GUI — 登録台帳の一覧と失効(M2)

```rust
// 登録済み一覧(設定「接続」に追加)。fp は secure::fingerprint(導出トークン)= Win 側設定画面の略号と一致
for d in registry.devices() {
    // 行: d.name / d.fp(略号)/ d.last_seen_label
}
// 失効(1端末だけ)。2回押しても成功(NotFound も成功)。全登録の初期化は従来どおり別枠
match registry.revoke(&selected.key_id) {
    Ok(RevokeOutcome::Revoked { .. }) => { /* この端末の全経路が即座に拒否になる */ }
    Ok(RevokeOutcome::NotFound) => { /* 冪等: 既に消えている。成功として扱う */ }
    Err(e) => { /* ディスク書込失敗: 台帳は変更されていない */ }
}
```

呼び出し側は上の5箇所以外に鍵・master・台帳の内部を一切触らない。これがこの設計の公開面の全体である。

---

## Shape

### データ構造(他のすべてに先立つ)

```rust
// ── crates/common/src/secure.rs ──────────────────────────────
/// 台帳の1端末を指す鍵ID。OS 乱数16バイトのhex32。newtype であり、
/// `DeviceKeyId::parse` 以外では作らせない(境界で検証し、内部では信頼する)
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeviceKeyId(String);

/// accept が「どの鍵で認証したか」を返す。Legacy は従来の単一共有トークン
/// (KNIT_TOKEN 運用・移行中の旧端末)。Device は端末別導出鍵
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Who {
    Legacy,
    Device(DeviceKeyId),
}

/// 待受側の資格の束。中身(Vec<(Who, PSK)>)は非公開。
/// 空では作れない(コンストラクタが保証)ので accept は常に1本以上を試す
pub struct ServerKeys { /* 非公開 */ }

impl ServerKeys {
    /// 従来運用(KNIT_TOKEN・Windows 側・テスト)。単一 PSK
    pub fn single(token: &str) -> Self;                            // not implemented
    /// 台帳運用(Mac)。legacy(master)+ 全端末の導出鍵。legacy が必ず入る
    pub fn legacy_plus(master: &str, devices: &[(DeviceKeyId, &str)]) -> Self; // not implemented
}

// ── crates/common/src/registry.rs(新モジュール)────────────
/// 端末別トークンの導出(純関数・ドメイン分離は psk/fingerprint と同じ作法):
///   Blake2s256("knit-device-v1\0" + master + "\0" + key_id) → hex64
/// 常に64桁小文字hex → credentials::parse_key を通る(境界検証は既存関数が担う)
pub fn device_token(master: &str, key_id: &DeviceKeyId) -> String;  // not implemented

/// 台帳の1行(非秘密)。ディスク(devices.json)に書かれる全フィールド
pub struct DeviceEntry {
    pub key_id: DeviceKeyId,
    pub name: String,     // 登録時の表示名(safe_peer_name 済み)
    pub fp: String,       // secure::fingerprint(導出トークン)。Win 側画面の略号と一致
    pub added_at: u64,    // unix 秒
}

pub struct DeviceRegistry { /* 非公開: 台帳パス + RwLock<Inner> + master(メモリのみ) */ }
pub struct Enrolled { pub key_id: DeviceKeyId, pub token: String }

/// 失効の結果。NotFound も成功(冪等)。「接続中だったか」は切断したかの報告用
pub enum RevokeOutcome { Revoked { had_session: bool }, NotFound }

impl DeviceRegistry {
    /// master(Keychain の既存スロット or KNIT_TOKEN)を種に台帳を開く。
    /// devices.json が無ければ空台帳。壊れていれば退避(.corrupt)して空台帳
    /// (legacy は master から再構築されるため接続は止まらない。端末だけ登録し直し)
    pub fn open(dir: &std::path::Path, master: &str) -> std::io::Result<Self>;  // not implemented

    /// 認証済み登録経路での発行。key_id を新規採番し、台帳へ原子追記し、
    /// 導出トークンを返す。"saved" ack を待たずにコミットする(Tradeoffs 参照)
    pub fn enroll(&self, name: &str) -> std::io::Result<Enrolled>;              // not implemented

    /// 1端末の失効。台帳から物理削除(原子保存)→ 以後の accept_keys() から消える
    /// → 認証済み稼働セッションを切断。冪等(NotFound も Ok)
    pub fn revoke(&self, key_id: &DeviceKeyId) -> std::io::Result<RevokeOutcome>; // not implemented

    /// legacy(master そのもの)の受付停止。KNIT_TOKEN 運用での M2 相当。
    /// 状態は devices.json の {"legacy_revoked": true} に永続
    pub fn revoke_legacy(&self) -> std::io::Result<()>;                          // not implemented

    /// UI 一覧。表示用コピー
    pub fn devices(&self) -> Vec<DeviceEntry>;                                   // not implemented
    /// accept に渡す資格束。呼び出しごとに read lock で作る(失効が即座に反映)
    pub fn accept_keys(&self) -> secure::ServerKeys;                             // not implemented
    /// 接続側(Mac→Win)の鍵解決。hello の device_id → 導出トークン。
    /// 未紐付けの宛先(旧端末)は master をそのまま返す
    pub fn outgoing_token(&self, peer_device_id: &str) -> String;                // not implemented
    /// accept 後に呼ぶ紐付け(who × hello の id)。メモリのみ・ディスクに書かない
    pub fn note_seen(&self, who: &secure::Who, device_id: &str, name: &str);     // not implemented
    /// discover 応答側の room 一覧: room_id(master) + 全 room_id(導出トークン)
    pub fn discover_rooms(&self) -> Vec<String>;                                 // not implemented
}
```

台帳のディスクスキーマ(devices.json、非秘密・write_atomic 原子保存):

```json
{
  "v": 1,
  "legacy_revoked": false,
  "devices": [
    { "key_id": "e3b0c4…(hex32)", "name": "事務室PC", "fp": "AB12CD34", "added_at": 1760832000 }
  ]
}
```

master も導出トークンも**このファイルには書かれない**。master は Keychain/DPAPI の既存単一スロット(credentials.rs:117-127)にのみ存在し、registry はメモリで保持するだけ。台帳が漏れても鍵は導出できない(一方向ハッシュ)。

### 支配的アクセスパターンのトレース

提案構造で3つの支配的パターンをトレースする。

**(a) 接続ごとの鍵選択(Mac の全 accept)**。`accept(stream, &registry.accept_keys(), label)`。accept_keys は read lock で `legacy_plus(master, devices)` を組む(数十バイト×N の clone)。secure::accept は最初の Noise レコードを1回だけ読み(read_rec)、束の全 PSK で responder を作り直して read_message を試す。**TCP 往復は増えない**(クライアントから見て現状と同一バイト列)。全滅なら現状と同じ PermissionDenied + 切断。成功した鍵の Who を返す。

**(b) 失効チェック**。`revoke(key_id)` は write lock で entries から除去 → persist::write_atomic で devices.json を保存 → メモリ鍵束に反映 → 稼働中セッション(PEERS 内の PeerEntry.auth == Device(key_id))を writer.shutdown で切断(peers::detach の id+gen 一致パターンを流用)。以後の accept_keys() にその鍵は含まれないため、本線・音声・bulk の全経路で同時に拒否される。**経路ごとに失効チェックを書く場所はない**(鍵束から消えることが失効)。

**(c) 登録時の追記**。`enroll(name)` は key_id 採番 → 台帳追記(write_atomic)→ 鍵束へ反映。2スレッドからの同時 enroll でも write lock で直列化され、key_id は毎回新規乱数なので衝突しない(衝突したら採番し直し、台帳の一意性は revoke/enroll の境界で保証)。

### シグネチャ(既存関数の変更点)

```rust
// secure.rs — connect は不変。accept だけ資格束と認証結果を返す
pub fn connect(s: TcpStream, token: &str, label: &[u8])
    -> io::Result<(Reader, Writer)>;                                          // 不変
pub fn accept(s: TcpStream, keys: &ServerKeys, label: &[u8])
    -> io::Result<(Reader, Writer, Who)>;                                     // 変更(戻り値+1)

// discover.rs — 応答側だけ room の集合を受け取る(問い合わせ側は不変)
pub fn respond(bind: &str, port: u16, rooms: &[String],
               allow: fn(IpAddr) -> bool, should_stop: impl Fn() -> bool)
    -> io::Result<()>;                                                        // 変更

// bulk.rs — Endpoint.token を受信資格束に置換。接続側の鍵は呼び出し側が供給
pub struct Endpoint {
    pub keys: secure::ServerKeys,   // 旧 token: String(accept 用)
    /* 他フィールド不変 */
}
pub fn connect_loop(ep: &'static Endpoint, addr: impl Fn() -> Option<SocketAddr>,
                    token: impl Fn() -> String, main_up: fn() -> bool,
                    should_stop: impl Fn() -> bool);                          // token 供給を追加

// pairing.rs — Invitation が台帳を参照する(発行は serve_credential 内で enroll)
impl Invitation {
    pub fn open(registry: std::sync::Arc<DeviceRegistry>,
                service_port: u16) -> io::Result<Self>;                       // master から registry へ
}
struct Credential { token: String, port: u16 }                                // 形状不変

// mac/conn.rs — PeerEntry に認証結果を運ぶ(後段の許可行列・操作権設計への境界)
pub struct PeerEntry { /* 既存フィールド */ pub auth: knit_common::secure::Who }
```

**後段設計への境界の名前**: `PeerEntry.auth: Who` がそれ。後段の許可行列(share スコープの端末別化)・操作権リースは「誰と認証した接続か」をこの1フィールドから引き、台帳側に `policy(key_id) -> …` のような照点を足す時は Who をキーにする。この設計は Who を運ぶところまで。

### モジュール map

```
crates/common/src/
  registry.rs(新)   台帳の全知識: 登録・失効・鍵束・宛先解決・discover rooms
  secure.rs          Who/ServerKeys/DeviceKeyId を追加。accept の全鍵試行。
                     psk()/fingerprint()/connect は不変
  pairing.rs         Invitation::open の引数を registry に。serve_credential が enroll
  bulk.rs            Endpoint.token→keys、connect_loop の token 供給
  discover.rs        respond の rooms 化(問い合わせ側・room_id 純関数は不変)
crates/mac/src/
  main.rs            master 解決の直後に DeviceRegistry::open → Arc 配布
                     (待受・audio::start・discover respond・Invitation へ渡す先を変える)
  session.rs         accept 3元組受け取り、handshake_and_hello に who、PeerEntry.auth
  conn.rs            PeerEntry.auth / bulk BULK Endpoint 構築時に keys
  gui/setup.rs       登録済み一覧+失効 UI(追加)。「すべての登録を初期化」は従来どおり
crates/win/src/      差分は次の3箇所のみ(詳細は「3経路」節)
  main.rs, conn.rs, xfer.rs
```

状態の所有権は単一(split ownership の回避): **台帳と鍵束の唯一の書き手は DeviceRegistry**。Keychain/DPAPI の単一スロットは master のみ(credentials.rs のまま)。PeerEntry.auth はスナップショット(認証時点の記録)で、revoke が参照する読み手。peer-sides.json(配置・エイリアス)と devices.json(鍵の発行記録)は別物で、UI の「登録済み: N台」は**台帳からのみ**出す(二重管理にしない)。

### 不変条件と、それを型と境界で表現する場所

1. **導出トークンは常に `parse_key` を通る64桁hex** — `device_token()` の出力型で保証(関数が hex64 以外を返せない)。登録経路の境界(pairing serve_credential)では受け側 client_credential の既存 `parse_key` が再検証する。二重だが既存の防御をそのまま活かす
2. **ServerKeys は空でない** — コンストラクタ(single/legacy_plus)以外から作れない。accept は「1本以上の鍵を試す」ことが型で保証され、鍵ゼロでの待受(全拒否の見落とし)が起こり得ない。legacy_plus は legacy_revoked でも端末が1台でも鍵を供給する(0台かつ legacy 失効なら ServerKeys は作れず=待受が起動時に失敗し、UI は初期化へ導く)
3. **DeviceKeyId は hex32** — `DeviceKeyId::parse` のみで生成。JSON からの復元も parse を通る(壊れた台帳行は退避)
4. **未接続=版0(win)・未知の行は無視・新フィールドは serde(default)** — 本設計は Msg を1つも増やさないので自動的に維持される
5. **master はディスクに平文で書かれない** — registry の書くファイルは devices.json(非秘密)のみ。テストで「台帳ファイルに master も導入トークンも部分文字列として現れない」ことを検証する

### インターフェースの深さ

公開面は `DeviceRegistry` の8メソッド + `ServerKeys` の2コンストラクタ + `accept` の戻り値。この小さな面の裏に隠れるもの:

- 複数鍵の試行順序・legacy の扱い・失効済み鍵の除外(accept_keys の内側)
- 宛先 device_id → トークンの解決と legacy フォールバック(outgoing_token の内側)
- discover のマルチルーム応答(discover_rooms の内側)
- 失効のディスク保存・メモリ反映・稼働セッション切断の順序(revoke の内側)

呼び出し側(mac/win の各待受)が意識するのは「registry を渡す」ことだけ。深いモジュールの条件(小さな面・大きな隠蔽)を満たす。逆に呼び出し側に残る露出は2つだけ:(1) Mac の接続側は宛先の device_id を供給する、(2) accept の呼び出し側は who を PeerEntry に運ぶ。どちらもその場所でしか持てない情報(接続先・ピア表)なので隠しようがない。

---

## 検証可能性: 「登録→失効→拒否・他端末無影響」のテスト計画

すべて `crates/common/tests/device_registry.rs`(新)+ registry.rs/secure.rs の単体テストで、**ループバックのみ・実機2台不要**(ハード制約5)。lib.rs:637-745 の bulk ループバックテスト(実 TCP + Noise)が先行例。

```rust
// 1. enroll_revoke_reject_and_others_unaffected(証明の主軸)
//    tempdir に DeviceRegistry::open(master)。enroll("A"), enroll("B")。
//    127.0.0.1:0 の listener で accept(&registry.accept_keys(), b"knit-main") を回し、
//    疎通チェック:
//      connect(s, &token_a, b"knit-main") → Ok。戻り who == Device(A)
//      connect(s, &token_b, b"knit-main") → Ok。戻り who == Device(B)
//      connect(s, master,       b"knit-main") → Ok。戻り who == Legacy(移行互換)
//    revoke(&A) の後:
//      connect(s, &token_a, …) → Err(PermissionDenied)  …「拒否」
//      connect(s, &token_b, …) → Ok                        …「他端末無影響」
//      connect(s, master,   …) → Ok                        …legacy は無影響
//    revoke(&A) をもう一度 → Ok(NotFound)。冪等性。
//    台帳 JSON を読み: master・token_a・token_b の部分文字列を含まない(不変条件5)
//
// 2. three_paths_main_audio_bulk(ハード制約7)
//    同じ registry から3ラベル b"knit-main" / b"knit-audio" / b"knit-bulk" で
//    accept×connect の成立、およびラベル不一致(connect を knit-audio で accept を
//    knit-main にする等)が弾かれることを確認(既存の経路分離が台帳鍵でも効く)
//
// 3. device_token_shape_and_domain_separation(単体)
//    64桁小文字hex・parse_key 通過・key_id が違えば違う値・master が違えば違う値・
//    "knit-psk-v1"/"knit-room-v1" 由来とは衝突しない(プレフィクス分離)
//
// 4. pairing_loopback_enrolls_derived(登録経路の結合)
//    pairing_probe.rs の構成を loopback で: Invitation::open(registry, port) を
//    127.0.0.1 で動かし、pairing::enroll(addr, code, save) が受け取る token が
//      (a) parse_key を通る
//      (b) registry.devices() に増えたエントリの device_token(master, key_id) と一致
//      (c) その鍵で accept_keys() に対し connect が通る(who == Device(key_id))
//    を検証。SAS 方式(request_approval)は wire の期限が実時間依存のため
//    6桁方式の enroll で代表させ、SAS は serve_credential 共通部のみを対象にする
//
// 5. revoke_cuts_active_session(稼働中切断)
//    accept 済み・hello 済みの模擬ピア(PeerEntry に auth=Device(A) を想定した
//    registry.note_seen 呼び出し)に対し revoke(&A) → Reader が EOF/エラーで
//    終わること。Mac 固有の PEERS を直接掴めない common テストでは、
//    registry が切断通知を返すコールバック/戻り値(had_session)で境界を検証し、
//    PEERS 結線は mac 側の統合テスト(既存 drag_handoff.rs パターン)に残す
//
// 6. accept_tries_all_keys_under_throttle(攻撃面回帰)
//    3端末分の ServerKeys に「どれでもない」トークンで connect を繰り返し
//    全て失敗(成否オラクルが出ない=応答は常に同一の切断)。
//    FailThrottle との組み合わせは「失敗1接続=throttle.fail() 1回」を
//    accept の呼び出し規約として secure のテストに明記する
```

CI は `cargo test -p knit-common` で完結(windows-luest 依存の win クレートをビルドしない)。win 側の ServerKeys::single 差し替えは1行なので、win の結合は既存の人力確認(2台)で担保し、鍵選択ロジック自体は共通コードのみに置く(ハード制約3)。

## 移行経路(既存単一トークン運用から)

**移行の原理**: master は現行のトークン解決そのまま(Keychain の既存スロット or KNIT_TOKEN)なので、**既存トークンは自動的に「legacy エントリ」になる**。PSK 導出が不変なため、旧端末の接続は現状どおり成功する。全再登録は不要。

1. **既存の Keychain 登録(64hex 共有キー)**: 更新した Mac を起動 → 台帳は空+legacy=既存キー → 既存 Win は無変更で接続可(Who=Legacy)。以後の新規登録は導出トークン。全端末が導出に移行したら、UI で「旧形式での接続(共通キー)を拒否」= revoke_legacy()。これで完全な端末別鍵体制
2. **KNIT_TOKEN 運用(手動直接接続)**: master=KNIT_TOKEN。同上に legacy として載る。既存の対向端末は無変更。**revoke_legacy() が M2 の KNIT_TOKEN 版**になる(トークン自体は変えられないが、この Mac が受け付けなくなる。「KNIT_TOKEN は UI から初期化不能」(setup.rs:418-423)という現状の閉じ込めを、失効方向にだけ開く)。env から KNIT_TOKEN を消して再起動すると master が Keychain 側へ代わる=台帳の導出鍵が全て変わるため、その場合は台帳初期化(全再登録)の案内を出す。UI 文言で明示する
3. **旧 Mac ↔ 新 Win**: 旧 Mac の serve_credential は単一トークンを配る → 新 Win はそれを parse_key で保存。動く。**新 Win に旧専用のコードは不要**
4. **新 Mac ↔ 旧 Win(v11..v17)**: 新 Mac は導出トークン(64hex)を配る → 旧 Win の client_credential は parse_key を通して保存し、connect はそのトークンで成功する。Mac 側 accept は台帳照合。Noise/JSON が不変なので版ゲートは現状どおり hello の ver が担う
5. **再登録(名前・IP 変更後の同じ端末)**: enroll は毎回新 key_id を採番(旧エントリの再利用はしない)。クライアントは単一スロットなので保存が新しい鍵で上書きされ、旧鍵は自然に使われなくなる(未使用エントリは UI から手動で掃除可能)。G01 の「同じ端末と識別」は hello の device_id が既に担っており、台帳は note_seen で key_id×device_id を紐付ける

## 3経路での扱い(音声 24901・bulk 24902・登録 24904)

| 経路 | accept 側 | connect 側 | 備考 |
|---|---|---|---|
| 本線 24900 | Mac: `registry.accept_keys()`(session.rs:424)/ Win: `ServerKeys::single`(conn.rs:287) | Win: 自分のトークン(不変)/ Mac: `registry.outgoing_token(device_id)` | who は PeerEntry.auth へ |
| 音声 24901 | Mac: `registry.accept_keys()`(audio.rs:1425) | Win: 自分のトークン(不変、win/audio.rs:515) | 音声は Win→Mac 一方向。Win 側 diff なし |
| bulk 24902 | `Endpoint.keys`(Mac=accept_keys / Win=single) | Mac: connect_loop へ token 供給クロージャ(`registry.outgoing_token(…)`)。Win: 自分のトークン | Endpoint は共通。Mac→Win の画像送信が接続側 |
| 登録 24904 | Invitation が registry を参照。PAKE/SAS/6桁の認証は不変 | client_credential は無変更(渡るトークンが導出鍵になるだけ) | Channel::handshake の PSK=PAKE 鍵(不変) |

**discover(24903/UDP)**: Win/Android の seek は `room_id(自分の保存トークン)`= 導出トークン由来。Mac の respond は `discover_rooms()` = room_id(master) + 全 room_id(導入トークン) に応答するので、導入トークン端末の発見が壊れない。Win が応答側になる構成(win/main.rs:540)は自分のトークン1本の単一 room のまま(`&[room_id(&token)]` への 1 行差分)。問い合わせ側(seek_first/seek_first_lan)のシグネチャは不変。

**Windows 側の差分の全リスト**(ハード制約3): (1) win/main.rs:540 respond の引数を配列で包む 1 行、(2) win/conn.rs:287 accept を `ServerKeys::single` で包む 1 行、(3) win/main.rs:508 / win/xfer.rs:398 の bulk Endpoint 構築で `token` → `keys`(各 1 行)。音声・credentials・UI は無変更。

## オフライン端末への失効(L03)

失効は **Mac ローカルの台帳削除**であり、相手端末へ何も送らない。設計は L03 の「届いていない削除要求を成功としない」を次の形で満たす:

- revoke の意味は「この端末からの**以後の**接続を受け付けない」。オフラインの相手にも「削除が届いた」とは主張しない。UI の確認文は「この端末からの接続を受け付けなくします(相手の端末のデータは消しません)」と書く
- 相手がオフラインでも Mac の防御は即座に完了する(鍵束から消えるだけ。通信を要しない)。紛失端末が後からオンラインで繋ごうとしても、全鍵ミスマッチ → PermissionDenied → FailThrottle の対象
- 失効済み端末に残った旧トークンは無害(どこにも受け口がない)。リモートワイプはスコープ外であり、しない

## Synthesis decision

(arena が記入する。空欄)

## Tradeoffs accepted

- **マスター漏えい = 全端末トークンの導出可能性と引き換えに、台帳を非秘密(JSON+write_atomic)に保つ**。Mac の Keychain を割られた時点で「新しい鍵を配る」権限も漏れているため、実害の差は小さい。端末側の漏えいは導出トークン1本分に隔離される(マスターは逆算不能・他端末の key_id は不明)という G02 の核心は保たれる
- **accept の全鍵試行(Compute 増幅)と引き換えに、プロトコル・クライアントの完全な無変更を得る**。1接続あたり最大 N 回(N=台帳+legacy、上限 cap 64 を ServerKeys に設ける)の復号試行。各試行はBlake2s+ChaChaPoly 数回でマイクロ秒 order、FailThrottle が接続単位(鍵単位でない)で連打を殺すので、実被害は評価できないほど小さい。平文ヒントによる鍵選択(増幅なし)は列挙可能性とフォールバックの二重実装を生むため拒った(Alternatives 1)
- **enroll を "saved" ack 前にコミット**。ack 欠落で「Win は保存済み・Mac 台帳なし」=二度と接続不可、という取り返しのつかない不整合を避ける(pairing.rs:708-712 の既存哲学: 保存済みなら通常接続で回復)。代償は「認証はされたが保存されなかった」未使用エントリが台帳に残り得ること。招待は3回試行・使い捨てのため稀で、UI から手動で掃除できる
- **enroll は常に新 key_id(旧エントリ再利用しない)**。同一端末の再登録で台帳に 2行残り得る(片方は未使用のまま)。鍵の使い回しによる「失効したはずの鍵が再登録で復活」事故のクラスを根絶できる方が高い
- **legacy エントリは明示 revoke されるまで常設**。移行を急かせない代わりに、いつまでも旧形式共有鍵での接続が生きる。UI に「旧形式での接続を受け付けています」を表示し、revoke_legacy へ導く
- **フィンガープリントの一致確認は「Win 側の略号が Mac の台帳一覧のどれかと一致する」形式に変わる**(従来は両画面に同じ文字列)。secure::fingerprint と Win 側表示コードは無変更のまま、mac GUI の一覧表示(台帳の fp)で対になる。 KNIT_TOKEN 運用(legacy)は従来どおり両画面一致

## Alternatives considered

1. **平文の鍵ヒント(initiator が Noise 前に端末IDプレフィクスを送り accept 側が鍵を選ぶ)**。鍵試行が O(1) になるが、(a) 登録端末のヒントが LAN の盗聴者に列挙される、(b) v11..v17 はヒントを送らないので全鍵試行フォールバックが結局必須=同じ仕事の2方式(two ways to do one task)、(c) accept の最初のバイトが新旧で分かれ、相互接続の分岐がプロトコル階に漏れる。複雑さの削減より漏出と二重実装が上回るため棄却
2. **Noise パターンの変更(NK/IK でサーバ静的鍵+クライアント鍵)**。端末の公開鍵でサーバが鍵を選べ、失効は公開鍵ブラックリストで済む理屈。しかし (a) v11..v17 相手は NNpsk0 しか喋らず二重スタックが必須、(b) win 側のsnow 構築が全経路で変わる=差分最小制約に正面衝突、(c) 静的鍵の保存・配布をもう一式作る必要がある。ランナーの立場(継承スタンス)として主案にはしない。将来の IK 化は本設計の台帳(DeviceKeyId)をそのまま公開鍵のキーにできるので、移行の土台は互換
3. **台帳に端末ごとの完全乱数トークンを秘密として保持(導出なし)**。マスター漏えいの連鎖はないが、台帳が秘密を保持するため Keychain/DPAPI の複数スロットか暗号化ストアが必要になり、失効・冪等・原子保存を全部手作りする。win.rs の単一ファイル DPAPI に載らず共通コードに置けない。導出案は「台帳は非秘密・保存は既存単一スロット」で既存パターンを全て踏襲できる。棄却
4. **hello の id で accept 側が端末を特定する**。hello は PSK 認証の**後**にしか読めない。PSK が合わなければ hello が復号できないので、鍵選択に使えない(循環)。そもそも id は自己申告・認証されない(gotcha 1)。論理的に不可能であり棄却
5. **ServerKeys を Vec<String> トークン列として呼び出し側に渡させる**。実装は最短だが、legacy の扱い・順序・失効済み除外が全待受に重複して現れる(information leakage)。ServerKeys 型で隠した
6. **discover を「導出トークン側で room を統一」するため Credential に room を同梱して Win に保存させる**。Win 側に room の保存(paired-host.txt の類い増設)が必要になり差分が増える。Mac の応答側だけrooms を増やす方が Win diff ゼロで済む。棄却

## Open questions and risks

- ServerKeys の鍵数上限 cap(設計では 64)は十分か。数十台の教室運用を想定するなら台帳 UI が先に破綻する可能性があり、cap 到達時の enroll エラー案内も要る
- legacy revoke を「常設エントリの手動失効」としたが、全端末の導出化が済んだことを自動検知(一定期間 Legacy 認証がゼロ)して案内する方が良いか。UI 判断を残す
- Android ブリッジ(enroll_existing_key/preserve_existing)は validate_transport_key(32..=512)なので導出トークン(64hex)を保存可能で、secure::connect・room_id も現行コードのままで動くはず。ただし Android 実機での discover・音声結合はこの設計の検証範囲外。確認を別チケットに残すべきか
- bulk connect_loop への token 供給クロージャは「宛先 device_id の入手」を mac/conn.rs 側の閉包に置いた。アクティブピア切替の瞬間(経路昇格で一時 2本)に旧宛先のトークンで新経路を張ろうとする短冊窓がある。connect 失敗→2秒後の再試行で自然回復するが、ログの落ち着かせ方だけ実装時に見る
- revoke での稼働セッション切断を PEERS 走査で実装する際、ロック順(STREAM_SLOT→PEERS→ACTIVE_PEER)を壊さないこと。テスト 5 はこの回帰を mac 統合側に残す
- KNIT_TOKEN 運用から Keychain 運用への切替(master 変更)は台帳全喪失=全再登録になる。この案内文言と、切替時に台帳を自動で空にするか(現案: 空にする+案内)は UI 設計で確定させる

## Next implementation step

`registry.rs` の `device_token`/`DeviceKeyId`/`DeviceRegistry::open/enroll/revoke` と `secure::ServerKeys`・全鍵試行版 `accept` を、テスト 1(enroll_revoke_reject_and_others_unaffected)と 3(device_token_shape)を最初に書いて TDD で通すこと。pairing・mac 各待受の差し替えはその後(テスト 4・2 が順にゲートになる)。

---

## 検討した代替案と棄却理由(まとめ)

| # | 代替案 | 棄却理由(一言) |
|---|---|---|
| 1 | 平文ヒントで accept 側が鍵選択 | 端末の列挙可能性+旧版フォールバックの二重実用。全鍵試行は往復不変でコスト無視でき、1方式で済む |
| 2 | Noise NK/IK 化(静的鍵) | v11..v17 互換の二重スタック必須・Win 差分大。将来の土台としては台帳がそのまま使える |
| 3 | 台帳に完全乱数トークンを秘密保持 | 秘密の複数スロット保存を手作り。win.rs 単一 DPAPI に載らず共通化できない |
| 4 | hello の id で鍵選択 | PSK 不一致では hello が復号できない(循環)。id は自己申告・認証されない |
| 5 | 呼び出し側へ鍵リスト露出 | legacy・順序・失効済みの規約が全待受に重複(information leakage) |
| 6 | Credential に room 同梱で Win 保存 | Win 側の保存増設が必要。Mac のマルチルーム応答だけで Win diff ゼロで済む |
| 7 | 失効を台帳の tombstone(論理削除)で表現 | 物理削除+NotFound 冪等で十分。鍵は再採番されるので復活要件がない |
