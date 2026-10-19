//! 端末別資格の登録台帳(devices.json)。
//! 信頼の唯一の記録であり、表示用の peer-sides.json(配置・エイリアス)とは別物。
//! 台帳は非秘密(key_id・表示名・略号・時刻だけ)で、master も端末別トークンも
//! 一切書かない。master を知らない者が台帳を読んでも鍵は導出できない
//! (端末別トークンは master からの一方通行ハッシュ導出)。
//! 書き込みは write_atomic(原子保存)。書込失敗ならメモリも変えない=台帳不変。
//! 失効は物理削除で冪等(NotFound も成功)。「鍵束から消えること」が失効そのもので、
//! 本線・音声・bulk の全経路に同時に効く(経路ごとに失効チェックを書く場所はない)

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::discover::room_id;
use crate::persist::{quarantine, write_atomic};
use crate::proto::safe_peer_name;
use crate::secure::{fingerprint, DeviceKeyId, ServerKeys, Who};

/// 台帳に載せられる端末数の上限。accept は最初のレコードを全鍵で試すため
/// 計算量が台数に比例する(1試行はマイクロ秒 order だが無制限にはしない)
pub const MAX_DEVICES: usize = 64;
const FILE: &str = "devices.json";

/// 端末別トークンの導出(純関数)。psk("knit-psk-v1")・fingerprint
/// ("knit-fingerprint-v1")・room_id("knit-room-v1")と同じ作法で、固定の
/// プレフィックスでドメイン分離する。常に64桁小文字hex なので
/// credentials::parse_key を通る(登録経路の境界検証は既存関数が担う)
pub fn device_token(master: &str, key_id: &DeviceKeyId) -> String {
    use blake2::Digest;
    let mut h = blake2::Blake2s256::new();
    h.update(b"knit-device-v1\0");
    h.update(master.as_bytes());
    h.update(b"\0");
    h.update(key_id.as_str().as_bytes());
    let out: [u8; 32] = h.finalize().into();
    out.iter().map(|b| format!("{b:02x}")).collect()
}

/// 台帳の1行(非秘密)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub key_id: DeviceKeyId,
    /// 表示名(safe_peer_name 済み)
    pub name: String,
    /// 端末別トークンの略号(secure::fingerprint)。相手側設定画面の略号と一致する
    pub fp: String,
    /// 登録時刻(unix 秒)
    pub added_at: u64,
    /// 最終接続成功時刻(unix 秒)。接続成功時だけ更新する(失効後の試行は記録しない)
    #[serde(default)]
    pub last_seen: Option<u64>,
}

/// 台帳ファイルの中身(devices.json)。v は将来の互換のために固定
#[derive(Serialize, Deserialize)]
struct LedgerFile {
    #[serde(default = "ledger_version")]
    v: u32,
    /// 旧形式(単一共有トークン)での受付を止めたか。revoke_legacy で立てる
    #[serde(default)]
    legacy_revoked: bool,
    #[serde(default)]
    devices: Vec<DeviceEntry>,
}

fn ledger_version() -> u32 {
    1
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}


/// 台帳ディレクトリのプロセス間排他(ロックファイル)。本体と --registry CLI の
/// 書き込み(read→変更→rename)が重ならないためのもの。保持者が死んだ場合に備え、
/// 5秒を過ぎたロックは破棄して取り直す(書き込みはミリ秒で終わるため、実運用で
/// このタイムアウトが効くことはほぼ無い)。取れない場合は None を返し、呼び出し側は
/// 楽観排他(バイト列比較)だけを続行する(窓は大幅に狭まる)
struct LedgerLock(std::path::PathBuf);

fn lock_ledger(path: &Path) -> Option<LedgerLock> {
    let lock = path.with_extension("lock");
    for _ in 0..40 {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&lock) {
            Ok(_) => return Some(LedgerLock(lock)),
            Err(_) => {
                let stale = std::fs::metadata(&lock)
                    .and_then(|md| md.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|e| e > std::time::Duration::from_secs(5));
                if stale {
                    let _ = std::fs::remove_file(&lock);
                    continue;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }
    None
}

impl Drop for LedgerLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 登録の成果物。token は認証済み登録経路でこの端末へ渡すもの
pub struct Enrolled {
    pub key_id: DeviceKeyId,
    pub token: String,
}

/// 失効の結果。NotFound も成功(冪等)
#[derive(Debug, PartialEq, Eq)]
pub enum RevokeOutcome {
    Revoked,
    NotFound,
}

struct Inner {
    legacy_revoked: bool,
    devices: Vec<DeviceEntry>,
    /// 最後に読み込み・書き込みで確定させた台帳ファイルのバイト列。
    /// 楽観排他の基準(書込直前にファイルがこれと同じか確かめる)
    raw: Option<Vec<u8>>,
    /// 最後に読んだ台帳ファイルの (更新時刻, サイズ)。--registry CLI のように
    /// 別プロセスが書き換えた変更を、再起動なしで拾うための目安
    stamp: Option<(std::time::SystemTime, u64)>,
    /// discover 応答の部屋一覧のキャッシュ(stamp が同じ間は再計算しない)。
    /// 応答は UDP パケットごとに呼ばれるため
    rooms: Option<((std::time::SystemTime, u64), Vec<String>)>,
}

/// 登録台帳。master(既存トークン)はメモリにのみ保持し、ディスクへは書かない。
/// 台帳と鍵束の唯一の書き手(登録・失効・紐付けはすべてここを経る)
pub struct DeviceRegistry {
    path: PathBuf,
    master: String,
    inner: Mutex<Inner>,
}

impl DeviceRegistry {
    fn lock_inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        // 毒されたロックでも台帳の保存を止めない(改善監査#3 と同じ方針)
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        self.reload_if_changed(&mut guard);
        guard
    }

    /// 台帳ファイルが別プロセス(--registry CLI 等)で書き換わっていれば読み直す。
    /// 判定は mtime+サイズ(毎ロックの stat 1回で済む)。読み直しで壊れた内容を
    /// 見たら退避して空で起動する(open と同じ扱い)
    fn reload_if_changed(&self, inner: &mut Inner) {
        let Ok(md) = std::fs::metadata(&self.path) else {
            return;
        };
        let stamp = (md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH), md.len());
        if Some(stamp) == inner.stamp {
            return;
        }
        let Ok(bytes) = std::fs::read(&self.path) else {
            return;
        };
        match serde_json::from_slice::<LedgerFile>(&bytes) {
            Ok(f) => {
                let mut devices = f.devices;
                devices.retain(|e| DeviceKeyId::parse(e.key_id.as_str()).is_some());
                inner.legacy_revoked = f.legacy_revoked;
                inner.devices = devices;
                inner.raw = Some(bytes);
                inner.rooms = None;
                inner.stamp = Some(stamp);
            }
            Err(e) => {
                quarantine(&self.path);
                eprintln!("[registry] 台帳の形が読み取れません({e})。退避して空で起動します");
                inner.legacy_revoked = false;
                inner.devices = Vec::new();
                inner.raw = None;
                inner.rooms = None;
                inner.stamp = None;
            }
        }
    }

    /// 台帳への書き込みを楽観排他で行う: 読み直し→変更→「ファイルが読んだ時の
    /// バイト列のままか完全比較」→書き込み。--registry CLI のような別プロセスの
    /// 書き込みが読みと書きの間に割り込んだ場合は、こちらの変更を捨てて台帳を
    /// 引き直してやり直す(失効が本体の書き戻しで無音に取り消されるのを防ぐ
    /// 唯一の防壁。プロセス内の Mutex は他プロセスには効かない)
    fn commit<T>(&self, f: impl Fn(&mut Inner) -> (io::Result<T>, bool)) -> io::Result<T> {
        for attempt in 0..16 {
            if attempt > 0 {
                // 競合が続く場合は少し空ける(書き合う両方が即再試行すると
                // またぶつかるため。実運用の競合は1回の再試行で消える)
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            // 可能ならプロセス間ロックを取ってから読む(ロックが取れた場合は
            // 「確認→rename」の間に他プロセスが入る余地が無くなる)
            let guard = lock_ledger(&self.path);
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            self.reload_if_changed(&mut inner);
            let basis = inner.raw.clone();
            let (out, wrote) = f(&mut inner);
            let out = match out {
                Err(e) => return Err(e),
                Ok(v) => v,
            };
            if !wrote {
                return Ok(out);
            }
            // バイト列の完全比較(mtime の分解能に頼らない)
            if std::fs::read(&self.path).ok() == basis {
                let file = LedgerFile {
                    v: 1,
                    legacy_revoked: inner.legacy_revoked,
                    devices: inner.devices.clone(),
                };
                let bytes = serde_json::to_vec(&file)
                    .map_err(|e| io::Error::other(format!("台帳の直列化に失敗: {e}")))?;
                write_atomic(&self.path, &bytes)?;
                inner.raw = Some(bytes);
                inner.stamp = std::fs::metadata(&self.path).ok().map(|md| {
                    (md.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH), md.len())
                });
                inner.rooms = None;
                return Ok(out);
            }
            // 競合: この周回の変更は捨て、引き直してやり直す
            drop(inner);
            drop(guard);
        }
        Err(io::Error::other(
            "台帳の書き込みが競合しました(他のプロセスが書き続けています)。少し待ってやり直してください",
        ))
    }

    /// master を種に台帳を開く。ファイルが無ければ空台帳。壊れていれば退避
    /// (.corrupt)して空台帳で起動(legacy は master から再構築されるため
    /// 接続は止まらない。端末だけ登録し直し)
    pub fn open(dir: &Path, master: &str) -> io::Result<Self> {
        let reg = Self {
            path: dir.join(FILE),
            master: master.to_string(),
            inner: Mutex::new(Inner {
                legacy_revoked: false,
                devices: Vec::new(),
                raw: None,
                stamp: None,
                rooms: None,
            }),
        };
        // 実ファイルの内容・バイト列・stamp を確定させる(壊れていれば退避)
        drop(reg.lock_inner());
        Ok(reg)
    }

    /// 認証済み登録経路での発行。key_id を新規採番(使い回ししない)し、
    /// 台帳へ原子追記して、導出トークンを返す。"saved" ack を待たずに
    /// コミットする(ack 欠落で「相手は保存済み・台帳なし」の二度と繋がらない
    /// 不整合を避ける。未使用エントリは UI から掃除する)
    pub fn enroll(&self, name: &str) -> io::Result<Enrolled> {
        self.commit(|inner| {
            if inner.devices.len() >= MAX_DEVICES {
                return (
                    Err(io::Error::other(format!(
                        "登録できる端末は {MAX_DEVICES} 台までです。不要な端末を登録解除してください"
                    ))),
                    false,
                );
            }
            let key_id = match DeviceKeyId::generate() {
                Ok(k) => k,
                Err(e) => return (Err(e), false),
            };
            // key_id は毎回新規乱数なので衝突しない(万一ぶつかったら採番し直し)
            if inner.devices.iter().any(|d| d.key_id == key_id) {
                return (Err(io::Error::other("key_id の採番が衝突しました。もう一度お試しください")), false);
            }
            let token = device_token(&self.master, &key_id);
            inner.devices.push(DeviceEntry {
                fp: fingerprint(&token),
                key_id: key_id.clone(),
                name: safe_peer_name(name),
                added_at: now_unix(),
                last_seen: None,
            });
            (Ok(Enrolled { key_id, token }), true)
        })
    }

    /// 1端末の失効。台帳から物理削除(原子保存)→ 以後の鍵束から消える。
    /// 冪等(NotFound も成功)。書込失敗なら Err でメモリも変えない
    pub fn revoke(&self, key_id: &DeviceKeyId) -> io::Result<RevokeOutcome> {
        self.commit(|inner| {
            let Some(pos) = inner.devices.iter().position(|d| &d.key_id == key_id) else {
                return (Ok(RevokeOutcome::NotFound), false);
            };
            inner.devices.remove(pos);
            (Ok(RevokeOutcome::Revoked), true)
        })
    }

    /// 旧形式(単一共有トークン=master そのもの)での受付停止。
    /// KNIT_TOKEN 運用での M2 相当。元に戻す操作は用意しない(全登録の初期化で
    /// 初期状態に戻る)
    pub fn revoke_legacy(&self) -> io::Result<()> {
        self.commit(|inner| {
            if inner.legacy_revoked {
                return (Ok(()), false);
            }
            inner.legacy_revoked = true;
            (Ok(()), true)
        })
    }

    /// 旧形式での受付を受け付けているか(UI 表示と鍵束構築の両方から使う)
    pub fn legacy_accepts(&self) -> bool {
        !self.lock_inner().legacy_revoked
    }

    /// UI 一覧用のコピー
    pub fn devices(&self) -> Vec<DeviceEntry> {
        self.lock_inner().devices.clone()
    }

    /// accept の鍵束構築用(増分B で ServerKeys へ渡す)
    pub fn key_ids(&self) -> Vec<DeviceKeyId> {
        self.lock_inner()
            .devices
            .iter()
            .map(|d| d.key_id.clone())
            .collect()
    }

    /// accept へ渡す鍵束。呼び出しごとに組む(失効・登録が次の接続から即座に
    /// 反映される。レコード1回×鍵数の試行はマイクロ秒 order)
    pub fn accept_keys(&self) -> io::Result<ServerKeys> {
        let inner = self.lock_inner();
        let legacy = !inner.legacy_revoked;
        let devices = self.device_tokens_locked(&inner);
        drop(inner);
        ServerKeys::legacy_plus(&self.master, legacy, &devices)
    }

    /// 登録済み各端末の (key_id, 端末別トークン)。鍵束・発見の部屋・UI の
    /// 略号表示の共通の源(導出は常にこの関数を経る=1つの source of truth)
    pub fn device_tokens(&self) -> Vec<(DeviceKeyId, String)> {
        self.device_tokens_locked(&self.lock_inner())
    }

    fn device_tokens_locked(&self, inner: &Inner) -> Vec<(DeviceKeyId, String)> {
        inner
            .devices
            .iter()
            .map(|d| (d.key_id.clone(), device_token(&self.master, &d.key_id)))
            .collect()
    }

    /// 接続成功の記録。last_seen と表示名を更新する。台帳に無い鍵(Legacy 等)は
    /// 何も記録しない(失効後の試行は accept が弾くため、そもそも届かない)
    pub fn note_seen(&self, key_id: &DeviceKeyId, name: &str, now: u64) -> io::Result<()> {
        self.commit(|inner| {
            let Some(entry) = inner.devices.iter_mut().find(|d| d.key_id == *key_id) else {
                return (Ok(()), false);
            };
            let name = safe_peer_name(name);
            if entry.last_seen == Some(now) && entry.name == name {
                return (Ok(()), false);
            }
            entry.last_seen = Some(now);
            entry.name = name;
            (Ok(()), true)
        })
    }

    /// 認証結果から接続側(Mac→宛先)のトークンを解く。Device は台帳の導出
    /// トークン、Legacy(旧端末・KNIT_TOKEN 運用)は master。宛先解決に自己申告の
    /// hello id は使わない(accept が認証した Who のみが根拠)
    pub fn token_for(&self, who: &Who) -> String {
        match who {
            Who::Device(k) => device_token(&self.master, k),
            Who::Legacy => self.master.clone(),
        }
    }

    /// この鍵が今も登録済みか(外部プロセスでの失効を、稼働中セッションの
    /// 切断へ反映するための定期チェックに使う)
    pub fn enrolled(&self, key_id: &DeviceKeyId) -> bool {
        self.lock_inner().devices.iter().any(|d| &d.key_id == key_id)
    }

    /// 未接続のまま残った古い行を掃除する(登録途中で保存されなかった orphan や、
    /// 長期に接続していない端末)。判定は「最終接続(未接続なら登録時刻)」が
    /// `older_than_secs` より前か。削除した台数を返す(0 なら書込もしない)
    pub fn prune_unseen(&self, older_than_secs: u64, now: u64) -> io::Result<usize> {
        self.commit(|inner| {
            let before = inner.devices.len();
            inner
                .devices
                .retain(|d| now.saturating_sub(d.last_seen.unwrap_or(d.added_at)) < older_than_secs);
            let removed = before - inner.devices.len();
            (Ok(removed), removed > 0)
        })
    }

    /// 登録の初期化用: 台帳を空に戻す(全端末の登録取り消し。UI の
    /// 「すべての登録を初期化」とは別に、master 差し替え時の案内で使う)
    pub fn clear(&self) -> io::Result<()> {
        self.commit(|inner| {
            inner.devices.clear();
            (Ok(()), true)
        })
    }

    /// discover 応答側の部屋一覧。legacy を受付中なら room_id(master) を先頭に、
    /// 各端末の room_id(導出トークン) を加える(導入済み端末の発見を壊さないため)
    pub fn discover_rooms(&self) -> Vec<String> {
        let mut inner = self.lock_inner();
        if let Some((stamp, rooms)) = &inner.rooms {
            if Some(*stamp) == inner.stamp {
                return rooms.clone();
            }
        }
        let mut rooms: Vec<String> = Vec::new();
        if !inner.legacy_revoked {
            rooms.push(room_id(&self.master));
        }
        for (_, token) in self.device_tokens_locked(&inner) {
            let r = room_id(&token);
            if !rooms.contains(&r) {
                rooms.push(r);
            }
        }
        inner.rooms = inner.stamp.map(|s| (s, rooms.clone()));
        rooms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("knit-registry-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn master() -> String {
        "5f".repeat(32)
    }

    fn key_id(n: u8) -> DeviceKeyId {
        DeviceKeyId::parse(&format!("{:032x}", n as u32 * 0x01010101)).unwrap()
    }

    /// 導出トークンは常に64桁小文字hex で、既存 parse_key を通る
    #[test]
    fn device_token_is_64_hex_and_parse_key_passes() {
        let token = device_token(&master(), &key_id(1));
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(credentials::parse_key(&token).unwrap(), token);
    }

    /// 導出は決定論的で、master と key_id の両方に依存して変わる
    #[test]
    fn device_token_separates_master_and_key() {
        let m = master();
        let a = device_token(&m, &key_id(1));
        let b = device_token(&m, &key_id(2));
        assert_eq!(a, device_token(&m, &key_id(1)), "同じ入力は同じ値");
        assert_ne!(a, b, "key_id が違えば違う値");
        assert_ne!(a, device_token(&"3a".repeat(32), &key_id(1)), "master が違えば違う値");
    }

    /// key_id は hex32 以外を受け付けない(台帳の壊れた行はこれで弾く)
    #[test]
    fn key_id_parse_rejects_bad_shapes() {
        assert!(DeviceKeyId::parse("0123456789abcdef0123456789abcdef").is_some());
        assert!(DeviceKeyId::parse("0123456789ABCDEF0123456789ABCDEF").is_some(), "大文字は正規化して受ける");
        assert_eq!(
            DeviceKeyId::parse("0123456789ABCDEF0123456789ABCDEF").unwrap().as_str(),
            "0123456789abcdef0123456789abcdef"
        );
        assert!(DeviceKeyId::parse("zz").is_none(), "短い");
        assert!(DeviceKeyId::parse(&"g".repeat(32)).is_none(), "hex ではない");
        assert!(DeviceKeyId::parse(&"0".repeat(33)).is_none(), "長い");
        assert_ne!(DeviceKeyId::generate().unwrap(), DeviceKeyId::generate().unwrap(), "採番は毎回別");
    }

    /// enroll は行を追記し、token と略号が導出どおりに台帳へ載る
    #[test]
    fn enroll_appends_entries_and_writes_ledger() {
        let dir = temp_dir("enroll");
        let m = master();
        let reg = DeviceRegistry::open(&dir, &m).unwrap();
        let a = reg.enroll("事務室PC").unwrap();
        let b = reg.enroll("Re\u{202e}B").unwrap();
        let devices = reg.devices();
        assert_eq!(devices.len(), 2);
        assert_ne!(a.key_id, b.key_id);
        let token = device_token(&m, &a.key_id);
        assert_eq!(a.token, token);
        let entry = devices.iter().find(|d| d.key_id == a.key_id).unwrap();
        assert_eq!(entry.name, "事務室PC");
        let entry_b = devices.iter().find(|d| d.key_id == b.key_id).unwrap();
        assert_eq!(entry_b.name, "ReB", "表示名は safe_peer_name を通る(Bidi 文字を落とす)");
        assert_eq!(entry.fp, fingerprint(&token), "略号は導出トークンの fingerprint");
        assert!(dir.join(FILE).exists(), "台帳ファイルが書かれる");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 台帳ファイルに master も導出トークンも部分文字列として現れない
    #[test]
    fn ledger_contains_no_secrets() {
        let dir = temp_dir("secrets");
        let m = master();
        let reg = DeviceRegistry::open(&dir, &m).unwrap();
        let a = reg.enroll("A").unwrap();
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(!text.contains(&m), "master が台帳に漏れない");
        assert!(!text.contains(&a.token), "導出トークンが台帳に漏れない");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 失効は対象だけを消し、2回目は NotFound(台帳のバイト列は変わらない)
    #[test]
    fn revoke_removes_only_target_and_is_idempotent() {
        let dir = temp_dir("revoke");
        let m = master();
        let reg = DeviceRegistry::open(&dir, &m).unwrap();
        let a = reg.enroll("A").unwrap();
        let b = reg.enroll("B").unwrap();
        assert_eq!(reg.revoke(&a.key_id).unwrap(), RevokeOutcome::Revoked);
        let ids: Vec<_> = reg.key_ids();
        assert_eq!(ids, vec![b.key_id.clone()], "対象だけが消える");
        let bytes1 = std::fs::read(dir.join(FILE)).unwrap();
        assert_eq!(reg.revoke(&a.key_id).unwrap(), RevokeOutcome::NotFound);
        let bytes2 = std::fs::read(dir.join(FILE)).unwrap();
        assert_eq!(bytes1, bytes2, "2回目の失効は何も書かない(冪等)");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 失効は新しい登録を跨いで持続する(graft: 失効済み鍵の復活がない)
    #[test]
    fn revoke_survives_new_enrollments() {
        let dir = temp_dir("persist-revoke");
        let m = master();
        let reg = DeviceRegistry::open(&dir, &m).unwrap();
        let a = reg.enroll("A").unwrap();
        reg.revoke(&a.key_id).unwrap();
        let c = reg.enroll("C").unwrap();
        let ids: Vec<_> = reg.key_ids();
        assert!(!ids.contains(&a.key_id), "失効済みは新しい登録でも戻らない");
        assert!(ids.contains(&c.key_id));
        assert_ne!(a.token, c.token);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// revoke_legacy は永続し、再読込しても旧形式は受け付けない
    #[test]
    fn revoke_legacy_persists_across_reopen() {
        let dir = temp_dir("legacy");
        let m = master();
        let reg = DeviceRegistry::open(&dir, &m).unwrap();
        let a = reg.enroll("A").unwrap();
        assert!(reg.legacy_accepts());
        reg.revoke_legacy().unwrap();
        assert!(!reg.legacy_accepts());
        let reopened = DeviceRegistry::open(&dir, &m).unwrap();
        assert!(!reopened.legacy_accepts(), "再読込しても旧形式は閉じたまま");
        assert_eq!(reopened.key_ids(), vec![a.key_id.clone()], "端末は消えない");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 台帳は再読込しても内容が保たれる
    #[test]
    fn reopen_keeps_devices() {
        let dir = temp_dir("reopen");
        let m = master();
        let reg = DeviceRegistry::open(&dir, &m).unwrap();
        reg.enroll("A").unwrap();
        reg.enroll("B").unwrap();
        let before = reg.devices();
        let reopened = DeviceRegistry::open(&dir, &m).unwrap();
        assert_eq!(reopened.devices(), before);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 別プロセス(--registry CLI 等)の書き換えを、再オープンなしで拾う
    #[test]
    fn external_ledger_changes_are_seen_without_reopen() {
        let dir = temp_dir("external");
        let m = master();
        let app = DeviceRegistry::open(&dir, &m).unwrap();
        let cli = DeviceRegistry::open(&dir, &m).unwrap();
        let a = app.enroll("A").unwrap();
        // CLI 側での追加登録が本体から見える
        let b = cli.enroll("B").unwrap();
        assert_eq!(app.key_ids(), vec![a.key_id.clone(), b.key_id.clone()]);
        // CLI 側での失効が本体から見える(=accept_keys から消える)
        assert_eq!(cli.revoke(&b.key_id).unwrap(), RevokeOutcome::Revoked);
        assert_eq!(app.key_ids(), vec![a.key_id.clone()]);
        assert_eq!(app.accept_keys().unwrap().len(), 2, "legacy + A");
        // 本体側の書き込み(note_seen)も CLI 側から見える
        app.note_seen(&a.key_id, "A端末", 999).unwrap();
        assert_eq!(
            cli.devices().iter().find(|d| d.key_id == a.key_id).unwrap().last_seen,
            Some(999)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 読みと書きの間に別プロセスの書き込みが割り込んでも、どちらの変更も
    /// 失われない(楽観排他の回帰。レビュー指摘 H1/中 の固定)
    #[test]
    fn writes_retry_when_another_process_writes_in_between() {
        let dir = temp_dir("interleave");
        let m = master();
        let reg = DeviceRegistry::open(&dir, &m).unwrap();
        let a = reg.enroll("A").unwrap();
        // 外部プロセス(CLI)の書き込み結果を再現: A に加えて X が載った台帳
        let cli = DeviceRegistry::open(&dir, &m).unwrap();
        let x = cli.enroll("X").unwrap();
        let external = std::fs::read(dir.join(FILE)).unwrap();
        // reg の書き込み処理の途中で外部書き込みが割り込む状況を人為的に作る:
        // f の実行中にファイルを外部の内容で差し替える
        reg.commit(|inner| {
            let _ = std::fs::write(dir.join(FILE), &external);
            let entry = inner.devices.iter_mut().find(|d| d.key_id == a.key_id).unwrap();
            entry.last_seen = Some(777);
            (Ok(()), true)
        })
        .unwrap();
        // 外部の X も本体の記録も失われない
        let final_ = DeviceRegistry::open(&dir, &m).unwrap();
        let devices = final_.devices();
        assert_eq!(devices.len(), 2, "A と X の両方が残る: {:?}", devices.iter().map(|d| &d.name).collect::<Vec<_>>());
        let ea = devices.iter().find(|d| d.key_id == a.key_id).unwrap();
        assert_eq!(ea.last_seen, Some(777), "本体の記録も残る");
        assert!(devices.iter().any(|d| d.key_id == x.key_id), "外部の登録も残る");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 2つのハンドル(本体と CLI)の同時書き込みで、登録・記録が無音に
    /// 消えない(ロストアップデートのハンマーテスト)
    #[test]
    fn concurrent_registries_do_not_lose_each_others_writes() {
        let dir = temp_dir("hammer");
        let m = master();
        let app = DeviceRegistry::open(&dir, &m).unwrap();
        let cli = DeviceRegistry::open(&dir, &m).unwrap();
        let a = app.enroll("A").unwrap();
        let a_key = a.key_id.clone();
        std::thread::scope(|s| {
            s.spawn(move || {
                for i in 0..25 {
                    cli.enroll(&format!("C{i}")).unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            });
            for t in 0..25u64 {
                app.note_seen(&a_key, "A", 1000 + t).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });
        let final_ = DeviceRegistry::open(&dir, &m).unwrap();
        let devices = final_.devices();
        assert_eq!(devices.len(), 26, "登録も記録も失われない");
        assert!(devices.iter().any(|d| d.key_id == a_key && d.last_seen.is_some()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 壊れた台帳は退避されて空台帳で起動する(接続を止めない)
    #[test]
    fn corrupt_ledger_is_quarantined_and_starts_empty() {
        let dir = temp_dir("corrupt");
        std::fs::write(dir.join(FILE), b"{ broken").unwrap();
        let reg = DeviceRegistry::open(&dir, &master()).unwrap();
        assert!(reg.devices().is_empty());
        assert!(dir.join(format!("{FILE}.corrupt")).exists(), "元の内容は退避先へ残る");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// key_id の壊れた行だけをスキップし、他の行は生かす
    #[test]
    fn invalid_ledger_rows_are_skipped() {
        let dir = temp_dir("badrows");
        let bad_ledger = r#"{"v":1,"devices":[
                {"key_id":"not-hex!","name":"壊れた行","fp":"00000000","added_at":1},
                {"key_id":"0123456789abcdef0123456789abcdef","name":"正常な行","fp":"AB12CD34","added_at":2}
            ]}"#;
        std::fs::write(dir.join(FILE), bad_ledger).unwrap();
        let reg = DeviceRegistry::open(&dir, &master()).unwrap();
        let devices = reg.devices();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "正常な行");
        assert_eq!(devices[0].last_seen, None, "last_seen は省略可能(default)");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// note_seen は last_seen と表示名を更新する(台帳に無い鍵は何もしない)
    #[test]
    fn note_seen_updates_last_seen_and_name() {
        let dir = temp_dir("seen");
        let m = master();
        let reg = DeviceRegistry::open(&dir, &m).unwrap();
        let a = reg.enroll("旧い名前").unwrap();
        reg.note_seen(&a.key_id, "新しい名前", 12345).unwrap();
        let entry = reg.devices().into_iter().find(|d| d.key_id == a.key_id).unwrap();
        assert_eq!(entry.last_seen, Some(12345));
        assert_eq!(entry.name, "新しい名前");
        // 台帳に無い鍵(Legacy 相当)は何も記録しない
        let other = DeviceKeyId::parse(&"7".repeat(32)).unwrap();
        reg.note_seen(&other, "誰か", 1).unwrap();
        assert!(reg.devices().iter().all(|d| d.key_id != other));
        let reopened = DeviceRegistry::open(&dir, &m).unwrap();
        assert_eq!(
            reopened.devices().into_iter().find(|d| d.key_id == a.key_id).unwrap().last_seen,
            Some(12345)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 宛先トークンの解決は認証結果(Who)から行う(Legacy は master)
    #[test]
    fn token_for_follows_the_authenticated_identity() {
        let dir = temp_dir("token_for");
        let m = master();
        let reg = DeviceRegistry::open(&dir, &m).unwrap();
        assert_eq!(reg.token_for(&Who::Legacy), m, "Legacy は master");
        let a = reg.enroll("A").unwrap();
        assert_eq!(reg.token_for(&Who::Device(a.key_id.clone())), a.token);
        // 導出は決定論的(失効しても値は同じ。失効した鍵を受け付ける待受は
        // もう無い=accept_keys から消えていることは accept_keys_reflect で固定済み)
        reg.revoke(&a.key_id).unwrap();
        assert_eq!(reg.token_for(&Who::Device(a.key_id.clone())), a.token);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// discover の部屋一覧: legacy 受付中は master の部屋を含み、各端末の部屋も含む。
    /// revoke_legacy 後は master の部屋だけが消える
    #[test]
    fn discover_rooms_cover_master_and_devices() {
        let dir = temp_dir("rooms");
        let m = master();
        let reg = DeviceRegistry::open(&dir, &m).unwrap();
        let a = reg.enroll("A").unwrap();
        let rooms = reg.discover_rooms();
        assert!(rooms.contains(&room_id(&m)), "legacy 受付中は master の部屋を応答");
        assert!(rooms.contains(&room_id(&a.token)), "端末の部屋を応答");
        reg.revoke_legacy().unwrap();
        let rooms = reg.discover_rooms();
        assert!(!rooms.contains(&room_id(&m)), "旧形式を閉じたら master の部屋は消える");
        assert!(rooms.contains(&room_id(&a.token)), "端末の部屋は残る");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 台帳の上限。超過する登録は Err(既定 64)
    #[test]
    fn enroll_respects_cap() {
        let dir = temp_dir("cap");
        let m = master();
        let reg = DeviceRegistry::open(&dir, &m).unwrap();
        for i in 0..MAX_DEVICES {
            reg.enroll(&format!("PC{i}")).unwrap();
        }
        assert!(reg.enroll("65台目").is_err(), "上限を超える登録は失敗");
        assert_eq!(reg.devices().len(), MAX_DEVICES);
        // 失敗した登録は台帳に残さない
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(!text.contains("65台目"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// clear は台帳を空に戻す(全登録取り消し)
    #[test]
    fn clear_empties_ledger() {
        let dir = temp_dir("clear");
        let m = master();
        let reg = DeviceRegistry::open(&dir, &m).unwrap();
        reg.enroll("A").unwrap();
        reg.clear().unwrap();
        assert!(reg.devices().is_empty());
        assert_eq!(DeviceRegistry::open(&dir, &m).unwrap().devices(), vec![]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// prune_unseen は「最終接続(無ければ登録時刻)」が古い行だけを消す
    #[test]
    fn prune_removes_only_stale_entries() {
        let dir = temp_dir("prune");
        let m = master();
        let reg = DeviceRegistry::open(&dir, &m).unwrap();
        let a = reg.enroll("新しい端末").unwrap();
        let b = reg.enroll("古い端末").unwrap();
        // b の登録時刻を 40 日前へ書き換える(ファイル経由)
        let p = dir.join(FILE);
        let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        for d in v["devices"].as_array_mut().unwrap() {
            if d["key_id"] == b.key_id.as_str() {
                d["added_at"] = serde_json::json!(now - 40 * 86_400);
            }
        }
        std::fs::write(&p, serde_json::to_vec(&v).unwrap()).unwrap();
        let removed = reg.prune_unseen(30 * 86_400, now).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(reg.key_ids(), vec![a.key_id.clone()], "古い行だけ消える");
        assert_eq!(reg.prune_unseen(30 * 86_400, now).unwrap(), 0, "2回目は何もしない");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// accept_keys は台帳の状態を常に反映する(登録・失効・legacy 閉鎖)
    #[test]
    fn accept_keys_reflect_ledger_state() {
        let dir = temp_dir("acceptkeys");
        let m = master();
        let reg = DeviceRegistry::open(&dir, &m).unwrap();
        assert_eq!(reg.accept_keys().unwrap().len(), 1, "空台帳でも legacy 1本");
        let a = reg.enroll("A").unwrap();
        assert_eq!(reg.accept_keys().unwrap().len(), 2, "登録で1本増える");
        reg.revoke(&a.key_id).unwrap();
        assert_eq!(reg.accept_keys().unwrap().len(), 1, "失効で即座に消える");
        reg.revoke_legacy().unwrap();
        assert!(reg.accept_keys().is_err(), "legacy 閉鎖+0台は鍵束を作れない");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
