# 端末別認証と登録台帳 — 候補2(再設計スタンス): 部屋鍵 + 端末静的鍵の二層資格と Noise IKpsk2

ランナー2(プロトコル版18として自由に設計する立場)。プロトコル v18・プロトコル上の下位互換(v11..v17 は従来経路で接続継続)・移行経路・検証可能性をすべて含む。

## Problem

現行の信頼モデルは「全端末で同一の 256bit 共有トークン1本」で、`secure.rs:15-21` の `psk(token)` が Noise NNpsk0 の PSK を1本だけ導く。このため (1) サーバは接続者がどの端末かを鍵から区別できず、(2) 登録台帳が存在せず、(3) 失効は「全登録の初期化」(全端末締め出し、`setup.rs:413-470` / `win/tray.rs:861`)しかない。hello の `id` は認証されない自己申告(Gotcha 1)で、トークンを持つ全員が全員になりすませる。M2(端末単位失効)と G02(端末固有の鍵で認証・端末単位の失効)が未払いのまま。

Phase A で確認した制約: 全4経路(本線 24900 / 音声 24901 / bulk 24902 / 登録 24904)が同じ単一トークンに依存している。Win は `KNIT_ROLE=server` で accept 側にも反転できる(`win/main.rs:474-550`, `win/conn.rs:287`)ので「accept する側」は Mac 固定ではない。snow 0.10 が依存にあり(`crates/common/Cargo.toml:10`)、X25519・Blake2s・ChaChaPoly の resolver は既定で有効。Windows ターゲットはこの Mac でコンパイルできないため、accept/connect のロジックは極力 common に置く。

## Usage (caller's view)

usage が仕様である。以下の3呼び出し箇所が今回の公開面の全部で、型はここから導いた。

### README 形式(概念)

Knit の「机」には1本の**部屋鍵**(既存の登録トークンをそのまま継続使用)と、**端末静的鍵**(登録時に各端末が生成する X25519 鍵ペア)の2層がある。接続の暗号化ハンドシェイクは版18から Noise IKpsk2 に代わり、端末は自分の静的鍵で認証され、サーバは台帳(registry.json)で「この端末は有効か」を**ハンドシェイクの1メッセージ目で**照合する。失効は台帳からその端末だけを `Revoked` にする。旧端末(v11..v17)は従来の NNpsk0+部屋鍵で今までどおり繋がる。

### 呼び出し箇所 1: mac/main.rs のトークン読み込み(現 `main.rs:829-900`)

```text
// 現行: let token = KNIT_TOKEN > credentials::load() > 初回登録UI
// 版18: 読み込んだ部屋鍵・静的鍵ペア・台帳から「待受側の資格一式」を1回で組む

let server = match auth::ServerTrust::load()? {          // Keychain(部屋鍵+自静的鍵)+ registry.json
    Some(t) => t,
    None => { /* 初回登録UI(既存の first_run)。完了後に再度 load */ }
};
// KNIT_TOKEN 運用: 部屋鍵のみ・静的鍵なしの ServerTrust(旧経路専用)。型で表現
let server = match envutil::get("KNIT_TOKEN").filter(|t| t.len() >= 32) {
    Some(t) => auth::ServerTrust::legacy_room(t)?,
    ...
};
session::server_thread(generation, port, server, screen_w, screen_h);
audio::start(host, server.clone(), port + 1);            // 音声も同じ1個の ServerTrust
```

呼び出し側は鍵のリストも新旧の切り分けも意識しない。`ServerTrust` を待受に渡すだけ。

### 呼び出し箇所 2: pairing の serve_credential 後継(`pairing.rs:397-421`)

```text
// 現行: {token, port} の JSON を渡して終わり
// 版18: 端末が生成した静的公開鍵を受け取り、台帳へ登録してから資格を渡す

fn serve_enrollment(channel: &mut Channel, trust: &auth::ServerTrust, port: u16, ...) -> io::Result<()> {
    let req: EnrollRequest = channel.receive_json()?;     // { device_pub, label, proto: 2 }
    let reply = trust.enroll(&req.device_pub, &req.label, SystemTime::now())?;
    //   ↑ 台帳への追記(冪等)。Revoked 済み公開鍵の再登録はここで拒否
    channel.send_json(&reply)?;                           // { port, room, server_static_pub }
    if channel.receive()? != b"saved" { ... }             // 端末の保存成功を見てから台帳を確定
    trust.registry().save()?;                             // write_atomic(失敗すれば台帳不変)
}
// 旧端末が登録に来た場合(最初のフレームが旧形式): 従来の serve_credential をそのまま返す(二重受付)
```

### 呼び出し箇所 3: secure::accept 周り(mac/session.rs::handshake_and_hello `session.rs:424`)

```text
// 現行: let (r, hw) = secure::accept(stream, token, b"knit-main")?;
// 版18: 誰として認証されたかが返る。失効済み端末はここで弾かれる

let (r, hw, who) = secure::accept(stream, &trust, b"knit-main")?;
// who = Authenticated::Device(DeviceId) | Authenticated::Legacy
// PeerEntry の主体キーは who から採る(hello.id は表示名の紐付け用に残る)
```

接続側(win/conn.rs:466・audio.rs:515・bulk・android-bridge)は `secure::connect(addr, &my, label)` に変わる。`my` は起動時に1回組む `ClientCredential`(登録で得た自分の静的鍵ペア+相手の静的公開鍵+部屋鍵+旧トークン)。ストリームを先に張って渡す現行形から、アドレスを渡す形へ(フォールバック再接続を中に隠すため。差分は呼び出し箇所の TcpStream 生成行が消えて済む)。

## Shape

### データ構造(先に)

```rust
// common/src/auth.rs(新規モジュール。台帳と資格の唯一の所有者)
pub struct RoomKey(String);          // 部屋鍵=旧トークン。psk2・旧経路・room_id の導出元。台帳に載せない

/// 端末の身份。静的公開鍵の指紋(Blake2s("knit-device-id-v1\0"+pub) 先頭16進16桁)。
/// hello.id(自己申告)とは別物で、こちらが認証済みの主体キー
pub struct DeviceId(String);

pub struct DeviceKeypair { secret: [u8; 32], /* private */ public: [u8; 32] }
impl DeviceKeypair {
    pub fn generate() -> io::Result<Self>;       // OS 乱数(既存 credentials::platform::random)
    pub fn public(&self) -> [u8; 32];
    pub fn id(&self) -> DeviceId;                // public から指紋
}

#[derive(Serialize, Deserialize)]
pub struct DeviceRecord {                        // registry.json の1エントリ。公開情報のみ(秘密を含まない)
    pub label: String,                           // safe_peer_name 済みの表示名
    pub pub_key: [u8; 32],
    pub added_at: SystemTime,
    pub last_seen: Option<SystemTime>,           // 接続成功時のみ更新
    pub state: DeviceState,
}
#[derive(Serialize, Deserialize)]
pub enum DeviceState { Active, Revoked { at: SystemTime } }  // tombstone。削除しない

#[derive(Default)]
pub struct Registry(BTreeMap<DeviceId, DeviceRecord>);       // idキーJSONストア(peer-sides.json と同型)
impl Registry {
    pub fn enroll(&mut self, pub_key: &[u8;32], label: &str, now: SystemTime) -> io::Result<EnrollReply>;
    //   不変条件: Revoked 済み公開鍵の再登録は Err(PermissionDenied)。既存 Active の
    //   同鍵再登録は label/last_seen 更新の冪等な成功(再接続・再登録の区別が不要)
    pub fn revoke(&mut self, id: &DeviceId, now: SystemTime) -> io::Result<()>;
    //   不変条件: 冪等(既に Revoked なら保存もせず Ok)。存在しない id も Ok(表示は UI 側で)
    pub fn lookup_active(&self, pub_key: &[u8;32]) -> Option<DeviceId>;  // accept の鍵選択。O(log n)
    pub fn note_seen(&mut self, id: &DeviceId, now: SystemTime);         // 成功時のみ
    pub fn save(&self, path: &Path) -> io::Result<()>;  // persist::write_atomic
    pub fn load(path: &Path) -> io::Result<Registry>;    // read_json_or_quarantine(破損は退避して空台帳)
}

/// accept 側(Mac 常時・Win は KNIT_ROLE=server)が組む。不変条件: registry に秘密は入らない
pub struct ServerTrust { room: RoomKey, mine: DeviceKeypair, registry: Registry, /* private */ }
impl ServerTrust {
    pub fn load() -> io::Result<Option<Self>>;        // credentials::load_server + registry.json
    pub fn legacy_room(token: String) -> io::Result<Self>;  // KNIT_TOKEN 運用(旧経路専用・静的鍵なし)
    pub fn enroll(...) / registry() / revoked_contains(...)  // 上記 API の委譲(台帳の書き込みはここだけ)
}

/// connect 側(端末)が組む。登録で得た資格一式
pub struct ClientCredential { room: RoomKey, mine: DeviceKeypair,
                              peer_static: [u8; 32], legacy: Option<String>, /* private */ }
impl ClientCredential {
    pub fn load() -> io::Result<Option<Self>>;        // credentials::load_client(旧形式からの移行読込込み)
    pub fn legacy_only(token: String) -> Self;        // KNIT_TOKEN 運用(フォールバック専用)
}

#[derive(Debug, PartialEq)]
pub enum Authenticated { Device(DeviceId), Legacy }   // accept の戻り値。境界で検証済み
```

### シグネチャ(secure.rs の変更面)

```rust
// common/src/secure.rs — Noise の全詳細はここに留める(深いモジュール)
const PATTERN_V18: &str = "Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s";  // 2メッセージ。RTT は現行と同じ
const PATTERN_LEGACY: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s"; // 現行パターン・無変更

/// 接続側。IKpsk2 を試み、相手が旧(v11..v17)なら legacy NNpsk0 で張り直す。
/// どちらで繋がったかは呼び出し側には見せない
pub fn connect(addr: SocketAddr, cred: &ClientCredential, label: &[u8])
    -> io::Result<(Reader, Writer)>;

/// 待受側。msg1 のレコード長で新旧をディスパッチし、台帳照合と失効チェックを
/// msg2 を送る前に済ませる。Revoked 済み・未知の静的鍵は Err(PermissionDenied)
pub fn accept(s: TcpStream, trust: &ServerTrust, label: &[u8])
    -> io::Result<(Reader, Writer, Authenticated)>;

pub fn fingerprint_of(room: &str) -> String;  // 表示用。従来の fingerprint と同じ導出(無変更)
```

wire 上の差分はハンドシェイクレコードの長さだけ(NNpsk0 msg1 = 48B / IKpsk2 msg1 = 128B)。この長さはハンドシェイク実行前に `snow` の builder から得て比較し、**コード内に手持ちの定数を置かない**(hand-synced list 回避。テストでパターン毎の長さを assert)。

### hello・メッセージの再構成(最小)

- `Msg::Hello` / `HelloOk` の wire 形式は **v18 でも無変更**。認証はハンドシェイク層で済むため、hello へ新フィールドを足さない。`PeerEntry` の主体キーは `Authenticated` 由来の `DeviceId`、`hello.id` は表示名紐付け用に従来どおり(旧端末は `legacy-{ip}` 代替を継続)。
- 新規メッセージは2つだけ(serde の既定に従い未知の行は旧側が無視・送る側が版ゲートで守る):
  - `Msg::AuthUpgrade { server_static_pub, room_fingerprint }` — 版18 Mac が**旧経路で**接続してきた版18端末にのみ送る(`hello.ver >= 18` ゲート。旧端末は未知の行として無視し何も起きない)
  - `Msg::AuthUpgradeOk { device_pub, label }` — 端末が静的鍵ペアを生成(初回のみ保存)して返す。Mac は `trust.enroll()` で台帳へ。以後の再接続は IKpsk2

### モジュール map

```
common/src/auth.rs         (新) 台帳 Registry・DeviceId・ServerTrust/ClientCredential 構築・失効
common/src/secure.rs       (改) IKpsk2 accept/connect + legacy ディスパッチ + フォールバック
common/src/credentials.rs  (改) 保存物が ServerCredential/ClientCredential に(Keychain 1エントリ・DPAPI 1ファイル。JSON 化。上書き拒否・原子保存・errSecItemNotFound 冪等は既存パターン踏襲)。旧単一トークンの移行読込
common/src/pairing.rs      (改) serve_credential → serve_enrollment(双方向の鍵交換)。旧形式リクエストは従来経路(二重受付)
common/src/proto.rs        (改) Msg に AuthUpgrade/AuthUpgradeOk 追加・peer_features に v18
common/src/discover.rs     (無変更) room_id は部屋鍵由来のまま
crates/mac:  main.rs(trust 構築)・session.rs(accept 戻り値)・audio.rs(label 同一)・gui/setup.rs(台帳表示+端末別失効UI)
crates/win:  main.rs(load 戻り型)・conn.rs(connect/accept 引数)・audio.rs・tray.rs(失効通知表示)
android-bridge: lib.rs(connect 引数)
```

### 支配的アクセスパターンのトレース

1. **接続ごとの鍵選択**: accept は msg1 を1レコード読み → 長さで新旧分岐 → 新なら `snow` responder が msg1 を復号した時点で `get_remote_static()` → `Registry::lookup_active`(BTreeMap 参照・O(log n))→ Active なら msg2、そうでなければ切断。ループも鍵リストも呼び出し側に見えない。
2. **失効チェック**: 上記と同じ1回の lookup。`state == Revoked` なら `Err(PermissionDenied)`。遅延なくハンドシェイク内で効く。
3. **登録時の追記**: `serve_enrollment` → `Registry::enroll` → `write_atomic`。失敗すれば台帳不変(現行の「保存失敗を成功としない」`pairing.rs:686-713` の作法を踏襲)。
4. **接続成功時**: `note_seen`(失効後に接続試行があっても記録しない=列挙の手がかりを出さない)。

### 不変条件の型表現と検証位置

- 「台帳に秘密が入らない」: `DeviceRecord` のフィールド型がそれを表現(秘密鍵を持つ `DeviceKeypair` の `secret` は private で `Registry` は取り得ない)。
- 「`Authenticated` は境界で検証済み」: `accept` の戻り値であること自体が証明。内部では信頼し、UI 表示以外で再検証しない。
- 「失効の冪等」: `revoke` の署名が `io::Result<()>` で状態を返さない。2回目は no-op 成功。
- 「未接続=版0」「版ゲートは peer_features 集約点」: 既存のまま。`AuthUpgrade` の送信も peer_features 経由で版ゲートする。

### インターフェースの深さ

公開面は実質4つ: `ServerTrust::load / ClientCredential::load / secure::accept / secure::connect`。隠すもの: 新旧プロトコルの選択とフォールバック、台帳照合と失効、psk 導出、レコード長ディスパッチ、旧形式資格の移行読込。露出するもの: 「誰として認証されたか」(`Authenticated`)と台帳の運用(`enroll/revoke/一覧`)だけ。これは呼び出し側(mac/main・win/main)が鍵リスト・版・新旧の区別を一切扱わないために必要最小限の面である。

### 音声 24901・bulk 24902・登録 24904 の各経路

- **音声 24901**: `audio::start(host, trust, port+1)`。`secure::accept/connect` の label(`knit-audio`)は無変更で、prologue による経路分離は継続。IKpsk2 の静的鍵は本線と同じものを使う(経路ごとに別鍵にしない。取り違えは label が防ぐ)。`Reader/Writer` の構造は無変更。
- **bulk 24902**: `bulk::Endpoint.token: String` が `cred: ClientCredential`(Win)に変わる。ハンドシェーク本体(`bulk.rs:1416/1476`)は common 内なので両 OS の diff は Endpoint 初期化の1フィールド。Endpoint ハンドシェーク時の `hello` 相当の交換(bulk::Endpoint のやり取り)も `Authenticated` を返す形に統一。
- **登録 24904**: SPAKE2+6桁 / SAS 承認の前段は無変更(部屋スコープの人数認証として従来どおり)。その中の `serve_credential` が `serve_enrollment` に拡張され、部屋鍵と Mac 静的公開鍵を渡して端末静的公開鍵を受け台帳へ登録する。最初のフレーム(旧 `Credential{token,port}` 形式か新 `EnrollRequest` か)で二重受付。

### 既存単一トークンからの移行経路

1. **Mac を v18 に上げるだけ**で、登録済みの v11..v17 端末は旧経路(NNpsk0+旧トークン=部屋鍵)で今までどおり繋がる。台帳は空から開始。全再登録は不要。
2. **v18 端末が旧経路で繋いだ場合**、Mac は hello の版を見て `AuthUpgrade` を送る。端末は静的鍵ペアを生成・保存し `AuthUpgradeOk` を返す。Mac は台帳へ追加。以後その端末は IKpsk2(1RTT)。**昇格は認証済み経路でしか起きない**(旧トークン=部屋鍵を知っている証明が既に済んでいる)。
3. **新規登録**(v18 同士)は最初から IKpsk2 用の鍵交換。旧 Mac への登録は従来形式(`Credential{token,port}`)のまま下位互換。
4. **KNIT_TOKEN 運用**: `legacy_room` / `legacy_only` として従来経路で動き続ける。ただし v18 同士なら昇格でき(手動トークンを知っている端末が台帳に載る)、以後その端末は端末別失効の対象になる。KNIT_TOKEN の再生成=全締め出しは従来どおり(手動配布の部屋鍵なので設計上やむを得ない。案内文に明記)。
5. **旧経路のクローズ**: 旧経路は「部屋鍵を知る者のなりすまし」が残る移行期間の窓。設定 `KNIT_ALLOW_LEGACY=0`(既定1)で旧経路を閉じられる。すべての端末が昇格済みになった部屋では閉じてよい。最終削除は別リリース。

### 「登録→失効→拒否・他端末は無影響」のテスト計画(ループバック自動テスト)

既存テスト群の作法(`pairing.rs` tests・`lib.rs:662-765`・`bulk_large.rs`)と同じく、127.0.0.1 の任意ポートでスレッド2本(accept 側・connect 側)を立て、tempdir に台帳を置く。関数・メッセージ単位:

1. `auth::tests`(純粋・ファイル不要):
   - `enroll` → `lookup_active` が `Some(DeviceId)`・`revoke` → `lookup_active` が `None`(ここが「登録→失効→拒否」の核)
   - `revoke` を2回実行 → どちらも Ok・台帳バイト列が同一(冪等)
   - Revoked 済み公開鍵の `enroll` → `Err`(他端末を再登録させない=同じ鍵の復活拒否)
   - 同一 Active 鍵の再 `enroll` → Ok・label 更新(再登録と再接続の区別が不要なこと)
2. `auth::tests`(ファイル): `save`→`load` 往復・破損 JSON は退避され空台帳で起動(`read_json_or_quarantine` 経由)・`write_atomic` 失敗注入で台帳不変。
3. `secure::tests`(ループバック・socketpair):
   - IKpsk2: `ClientCredential` で connect ↔ `ServerTrust` で accept → `Authenticated::Device(id)` が `ClientCredential.mine.id()` と一致
   - **失効の即時性**: accept スレッドが msg1 を読んだ後・msg2 を書く前に切ることを、connect 側が「書き込み成功後に読み出しエラー」で観測
   - 台帳にない鍵・Revoked 済み鍵で connect → accept が `Err(PermissionDenied)`(メッセージは台帳にある場合と区別できない文字列)
   - **他端末無影響**: 端末A/B を enroll → A を revoke → A の connect 失敗・B の connect 成功を同時に検証
   - legacy: `legacy_only` で connect(旧 NNpsk0)↔ v18 accept → `Authenticated::Legacy`・通信可
   - フォールバック: v18 ClientCredential を**旧 accept(現行 `secure::accept(stream, token, label)` を残したテスト用エイリアス)**に繋ぐ → IKpsk0 失敗→legacy で再接続し成功
   - label 取り違え: `knit-main` で connect したのを `knit-audio` で accept → 失敗(既存テストと同じ形)
4. `pairing::tests`(既存 `invitation()` ヘルパを拡張):
   - `Invitation::bind` に `ServerTrust` を渡す形へ。`enroll`(端末側)→ 台帳に `{label, pub_key}` が載り `EnrollReply{room, server_static_pub}` が返る
   - 登録成功後、同じ招待コードの再使用は従来どおり失敗(単発性は無変更)
   - 旧形式クライアント(旧 `Credential{token,port}` を想定した生フレーム)→ 従来どおり部屋鍵が渡る(二重受付)
   - Revoked 済み鍵を持つ端末の登録 → `Err`(登録経路からの失効回避)
5. 昇格(メッセージ単位・認証済み legacy セッション上で):
   - Mac 側から `AuthUpgrade` を送る関数 → 端末側ハンドラが `ClientCredential` を保存し `AuthUpgradeOk` を返す → Mac の台帳に載る → 同じ2者で再 connect は IKpsk2 で成功(1 RTT であることは記録ロジックでなく「接続完了までの往復回数」をカウントして検証)
   - 旧版(`hello.ver=17` 相当)には `AuthUpgrade` を送らない(peer_features 版ゲートのユニットテスト)
6. 失効がオフライン端末に届かない(L03 の扱い): オフライン端末の `ClientCredential` は失効後もローカルに残る。テストは「revoke 後の connect が拒否される」ことで効力を証明(上記3)。**削除要求の配送はこの設計の範囲外**(失効は Mac の accept 面の状態であり、オフライン端末への通知機構ではない)。台帳 `last_seen` に「最後に繋がった時刻」を残すことで、UI は「失効した端末がそれ以降一度も接続していない」ことを示せる。

## Synthesis decision

(空欄 — arena が記入する)

## Tradeoffs accepted

- 我々は**旧経路(NNpsk0+部屋鍵)を移行期間中残す**ことを引き受ける。部屋鍵が漏えている間は旧端末のなりすましが可能なまま(`KNIT_ALLOW_LEGACY=0` で閉じられるまで)。引き換えに v11..v17 との相互接続が完全に保たれ、全再登録を強制しない。
- 我々は**静的鍵の導入によるプロトコル変更**を引き受ける(snow のビルダ引数・レコード長ディスパッチ・フォールバック)。引き換えに台帳照合と失効がハンドシェイクの1メッセージ目に効き、accept 側が複数 PSK を総当たりする案(代替案1)のような台数比例の計算とタイミング漏れを避ける。
- 我々は**成否オラクルを残す**(accept が台帳にない鍵を即切断する vs ある鍵には msg2 を返す)。256bit の静的鍵推測が必要で、既存の FailThrottle が連打を鈍らせるため、列挙には使えない。旧経路の PSK 成否オラクルも従来どおり。
- 我々は**レコード長を平文で出す**(新旧のディスパッチに必要・48B/128B)。長さ自体は情報量がなく、部屋や端末の特定に使えない。
- 我々は**端末静的鍵を Mac の静的鍵でのみ暗号化して送る(IK)**ことを受け入れる。XX(代替案2)ほどの identity hiding はないが、このシステムでは端末鍵は Mac(responder)にだけ見えればよく、Mac の静的鍵は登録で配布される公開情報なので、隠す対象が最初からない。
- 我々は**KNIT_TOKEN 運用を端末別失効の第一級対象にしない**(昇格すれば対象になる)。手動配布の部屋鍵は配布先を制御できないため、再生成=全締め出し以外の失効原理がない。従来からの変更点は「昇格すれば端末別にできる」ことだけ。
- 我々は**`secure::connect` のシグネチャを TcpStream から SocketAddr に変える**ことを引き受ける(呼び出し側で数行ずつ変わる)。フォールバック再接続を中に隠すためであり、nodelay 等の設定も内へ移して呼び出し側はむしろ単純になる。

## Alternatives considered

1. **端末別 PSK の総当たり(NNpsk0 維持・accept が全 PSK を順に試す)** — 実装差分が最小の案。だが accept 側の計算が台数比例、どの PSK で当たったかが処理時間に漏れる素地、PSK が部屋鍵と独立になり登録・discover の部屋概念と割れる。台帳照合が1回の map lookup で済む静的鍵案に劣る。静的鍵は「登録時に一度公開鍵を預ける」だけで資格の検証が鍵空間の暗号論的な強さに立つ。
2. **Noise_XXpsk2(静的鍵を相互に隠す・3メッセージ)** — identity hiding は最も強いが RTT が3メッセージに増え、音声・bulk の再接続遅延が現行より悪化する。このシステムでは initiator の鍵は responder に見えてよく、responder(Mac)の鍵は登録で配る公開情報。隠す利点がコストに見合わない。
3. **平文デバイスタグ+NNpsk0(端末IDを平文で送りサーバが鍵を選ぶ)** — TLS SNI 型。タグが LAN に平文で列挙され、タグ改ざんの余地が残り、「探索された端末やIPを信頼の根拠にしない」(G02)の趣旨に反する。
4. **hello への署名追加(ハンドシェイクは部屋鍵のまま)** — 端末鍵の署名を hello に載せて台帳照合する案。wire の変更は最小だが、失効が hello 以降(データ受信直前)まで効かず、署名検証がアプリ層(mac/session・win/conn の両方)に散らばる。プロトコルの根に置く方が呼び出し側が単純。
5. **部屋鍵の廃止(端末静的鍵のみへ)** — 失効の純度は最も高いが、登録の PAKE/SAS と discover の room_id が部屋鍵に載っており、廃止は全再登録と discover の再設計を強いる。移行要件(ハード制約2)に反するため不可。
6. **accept 側で N 本の listener を立ててポートで弁別** — ポート増は防火壁・既存配置(24900..24904 固定)と衝突し、旧端末の接続先が変わる。後方互換が壊れる。

## Open questions and risks

- snow 0.10 が `"Noise_IKpsk2_25519_ChaChaPoly_BLAKE2s"` をパースするか?(実装初日に `snow::Builder` で検証する。この Mac では `cargo test -p knit-common` が動くので確認可能。不可なら XXpsk2 への置換が最小の後退で、RTT が1増えるだけで他の設計は不変)
- Win が `KNIT_ROLE=server` のときの台帳は「Mac の静的鍵1エントリ」で足りるか?(現行の 1:1 モデルなら足りる。Win が複数 Mac を受ける運用が生まれたら registry.json を Win 側にも持たせる必要がある。設計上は `ServerTrust` が同じ形で載るので拡張点は1箇所)
- Android の Keystore 保存(`enroll_existing_key` の延長)で鍵ペアを Keystore 内で非露出にできるか、それとも既存どおり OS 乱数で作って保護保存で十分か?(Android bridge は現状 common の secure を呼ぶだけなので、`ClientCredential` を渡す形に揃う。Keystore ハードウェア鍵は別途)
- 昇格(`AuthUpgrade`)を送るタイミングの詳細: hello_ok 直後でよいか、それともアイドル時にすべきか?(接続直後の方が単純。旧端末は無視するだけなので影響なし)
- `secure::fingerprint` の表示(「鍵の略号」)は部屋鍵由来のままか、端末鍵由来に変えるか?(ユーザー確認 UI の意味論。部屋が同じなら同じ略号が今までどおり機能するので、まず現行維持を推奨)
- 台帳 registry.json の同時書き込み(登録中に失効 UI を開く等)は `ServerTrust` 内の Mutex で直列化する想定だが、UI スレッドとの待ち合わせ設計は実装時の論点。

## Next implementation step

`common/src/auth.rs` に台帳(`Registry::enroll/revoke/lookup_active/save/load` と冪等性)を書き、冪等・Revoked 再登録拒否・破損退避の単体テストを通す。次に `secure.rs` へ IKpsk2 の accept/connect を足し、ループバックテストで「登録→失効→拒否・他端末無影響」を最初に証明する。
