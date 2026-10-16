//! 端末ごとのポインター感度と、独立した送出元/送出先の接続範囲。
//! 範囲の端点は左右の辺では上→下、上下の辺では左→右。通信設定とは独立し、
//! writer・接続世代・暗号状態には一切触れない。
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PeerSettings {
    pub source: (f64, f64),
    pub target: (f64, f64),
    pub sensitivity: f64,
}

impl PeerSettings {
    /// 未保存の端末は旧共通感度と斜め配置の範囲を継承する。相手側は従来どおり全辺。
    pub fn for_side(side: u8) -> Self {
        let scale = crate::mouse_scale();
        Self {
            source: crate::tap::side_lay_range(side),
            target: (0.0, 1.0),
            sensitivity: if scale.is_finite() {
                scale.clamp(0.2, 3.0)
            } else {
                1.0
            },
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, range) in [("source", self.source), ("target", self.target)] {
            if !valid_range(range) {
                return Err(format!("{name}: 0 ≤ 開始 < 終了 ≤ 1 の有限値が必要です"));
            }
        }
        if !self.sensitivity.is_finite() || !(0.2..=3.0).contains(&self.sensitivity) {
            return Err("感度は0.2〜3.0の有限値で指定してください".into());
        }
        Ok(())
    }
}

pub fn valid_range(range: (f64, f64)) -> bool {
    range.0.is_finite()
        && range.1.is_finite()
        && 0.0 <= range.0
        && range.0 < range.1
        && range.1 <= 1.0
}

pub fn contains(range: (f64, f64), position: f64) -> bool {
    valid_range(range) && position.is_finite() && position >= range.0 && position <= range.1
}

/// 範囲外は壁(None)。画面の辺による反転はしない。
pub fn map_range(position: f64, from: (f64, f64), to: (f64, f64)) -> Option<f64> {
    if !contains(from, position) || !valid_range(to) {
        return None;
    }
    let progress = (position - from.0) / (from.1 - from.0);
    Some((to.0 + progress * (to.1 - to.0)).clamp(to.0, to.1))
}

pub fn map_source(settings: PeerSettings, position: f64) -> Option<f64> {
    map_range(position, settings.source, settings.target)
}

pub fn map_target(settings: PeerSettings, position: f64) -> Option<f64> {
    map_range(position, settings.target, settings.source)
}

#[derive(Default)]
struct Store {
    peers: BTreeMap<String, PeerSettings>,
    // 壊れた/読めないファイルを、空の設定として黙って上書きしない。
    load_error: Option<String>,
}

fn memory_only() -> bool {
    cfg!(test) || crate::gui::UI_PREVIEW.load(Ordering::Relaxed)
}

fn store_for(preview: bool) -> &'static Mutex<Store> {
    static LIVE: OnceLock<Mutex<Store>> = OnceLock::new();
    static PREVIEW: OnceLock<Mutex<Store>> = OnceLock::new();
    if preview {
        // config_dir() は旧ディレクトリの移行を書き込む場合があるので呼ばない。
        PREVIEW.get_or_init(|| Mutex::new(Store::default()))
    } else {
        LIVE.get_or_init(|| Mutex::new(load()))
    }
}

fn valid_id(id: &str) -> bool {
    !id.trim().is_empty() && !id.chars().any(char::is_control)
}

fn parse_entry(v: &Value) -> Result<PeerSettings, String> {
    let range = |name: &str| -> Result<(f64, f64), String> {
        let a = v[name]
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or_else(|| format!("{name}: 2つの数値が必要です"))?;
        Ok((
            a[0].as_f64()
                .ok_or_else(|| format!("{name}: 数値ではありません"))?,
            a[1].as_f64()
                .ok_or_else(|| format!("{name}: 数値ではありません"))?,
        ))
    };
    let settings = PeerSettings {
        source: range("source")?,
        target: range("target")?,
        sensitivity: v["sensitivity"]
            .as_f64()
            .ok_or("感度が数値ではありません")?,
    };
    settings.validate()?;
    Ok(settings)
}

fn parse(v: &Value) -> Result<BTreeMap<String, PeerSettings>, String> {
    if v["version"].as_u64() != Some(1) {
        return Err("未対応の画面設定ファイルの版です".into());
    }
    let peers = v["peers"]
        .as_object()
        .ok_or("端末の画面設定がオブジェクトではありません")?;
    peers
        .iter()
        .map(|(id, value)| {
            if !valid_id(id) {
                return Err("端末IDが不正です".into());
            }
            parse_entry(value)
                .map(|settings| (id.clone(), settings))
                .map_err(|e| format!("{id}: {e}"))
        })
        .collect()
}

fn load() -> Store {
    let result = (|| -> Result<BTreeMap<String, PeerSettings>, String> {
        let path = knit_common::envutil::config_dir()
            .ok_or("設定フォルダを取得できません")?
            .join("screen-settings.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(format!("画面設定を読み込めません: {e}")),
        };
        let v =
            serde_json::from_str(&text).map_err(|e| format!("画面設定のJSONが不正です: {e}"))?;
        parse(&v)
    })();
    match result {
        Ok(peers) => Store {
            peers,
            load_error: None,
        },
        Err(error) => {
            eprintln!("[screen-settings] {error} (元ファイルを保持し、既定値を使用します)");
            Store {
                peers: BTreeMap::new(),
                load_error: Some(error),
            }
        }
    }
}

pub fn get(id: &str, side: u8) -> PeerSettings {
    store_for(memory_only())
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .peers
        .get(id)
        .copied()
        .unwrap_or_else(|| PeerSettings::for_side(side))
}

fn encode(peers: &BTreeMap<String, PeerSettings>) -> Value {
    let peers: BTreeMap<_, _> = peers
        .iter()
        .map(|(id, s)| {
            (
                id,
                json!({
                    "source": s.source, "target": s.target, "sensitivity": s.sensitivity,
                }),
            )
        })
        .collect();
    json!({"version": 1, "peers": peers})
}

/// 保存済みの端末別設定だけを返す。接続状態や旧共通の配置には触れない。
/// Test/Preview は専用メモリを参照し、config_dir() も呼ばない。
pub fn snapshot_json() -> Value {
    let current = store_for(memory_only())
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    encode(&current.peers)
}

/// 副作用なしの事前検証。アーカイブ全体の検証で、保存前に呼ぶ。
pub(crate) fn validate_json(value: &Value) -> Result<(), String> {
    parse(value).map(|_| ())
}

/// 全エントリーを検証してから、一括保存する。未記載の端末は保持する。
pub fn import_json(value: &Value) -> Result<(), String> {
    let updates: Vec<_> = parse(value)?.into_iter().collect();
    set_many(&updates)
}

/// アーカイブの失敗時専用。mergeでは新規端末を消せないため、保存済みsnapshotへ
/// 完全に戻す。通常のimportには使わない。読込エラー時の上書き禁止は維持する。
pub(crate) fn restore_json(value: &Value) -> Result<(), String> {
    let peers = parse(value)?;
    update_store(|next| *next = peers)
}

pub fn set(id: &str, settings: PeerSettings) -> Result<(), String> {
    set_many(&[(id.to_string(), settings)])
}

/// UIの一括Apply用。列挙されていない端末は保持し、全検証→一度だけatomic保存→
/// メモリ反映の順に確定する。失敗時は既存のファイル/実効設定を変更しない。
/// PEERS等を取得しないため、呼び出し元の接続ロックとの逆順/再入を作らない。
pub fn set_many(updates: &[(String, PeerSettings)]) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    for (id, settings) in updates {
        if !valid_id(id) {
            return Err("端末IDが不正です".into());
        }
        if !ids.insert(id.as_str()) {
            return Err(format!("端末IDが重複しています: {id}"));
        }
        settings.validate().map_err(|e| format!("{id}: {e}"))?;
    }
    if updates.is_empty() {
        return Ok(());
    }
    update_store(|next| next.extend(updates.iter().cloned()))
}

fn update_store(update: impl FnOnce(&mut BTreeMap<String, PeerSettings>)) -> Result<(), String> {
    // 保存同士だけ直列化する。fsync中に入力スレッドのgetまで止めない。
    static APPLY: Mutex<()> = Mutex::new(());
    let _apply = APPLY.lock().unwrap_or_else(|e| e.into_inner());
    let preview = memory_only();
    let store = store_for(preview);
    let mut next = {
        let current = store.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(error) = &current.load_error {
            return Err(error.clone());
        }
        current.peers.clone()
    };
    update(&mut next);
    if !preview && !memory_only() {
        let path = knit_common::envutil::config_dir()
            .ok_or("設定フォルダを取得できません")?
            .join("screen-settings.json");
        let bytes = serde_json::to_vec_pretty(&encode(&next))
            .map_err(|e| format!("画面設定を変換できません: {e}"))?;
        write_atomic(&path, &bytes).map_err(|e| format!("画面設定を保存できません: {e}"))?;
    }
    store.lock().unwrap_or_else(|e| e.into_inner()).peers = next;
    Ok(())
}

/// create_newで一時ファイルを所有し、既存のリンクや固定名のtempを追従しない。
/// 外部のディレクトリ権限は変更しない。失敗時は自分のtempだけを片付ける。
fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::atomic::AtomicU64;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("設定フォルダがありません"))?;
    std::fs::create_dir_all(parent)?;
    match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
            return Err(std::io::Error::other(
                "画面設定の保存先が通常ファイルではありません",
            ));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("設定ファイル名がありません"))?;
    let mut temporary = std::ffi::OsString::from(".");
    temporary.push(name);
    temporary.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let temporary = parent.join(temporary);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    // create_newが成功したpathだけを削除する。rename成功後はすでに存在しない。
    let _ = std::fs::remove_file(&temporary);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> PeerSettings {
        PeerSettings {
            source: (0.2, 0.6),
            target: (0.4, 0.9),
            sensitivity: 1.5,
        }
    }

    #[test]
    fn ranges_map_both_directions_without_flipping() {
        let s = settings();
        for source in [0.2, 0.3, 0.4, 0.6] {
            let target = map_source(s, source).unwrap();
            assert!((map_target(s, target).unwrap() - source).abs() < 1e-12);
        }
        assert_eq!(map_source(s, 0.2), Some(0.4));
        assert_eq!(map_source(s, 0.6), Some(0.9));
        assert_eq!(map_source(s, 0.1), None);
        assert_eq!(map_target(s, 0.3), None);
        assert_eq!(map_target(s, f64::NAN), None);
    }

    #[test]
    fn unsaved_peer_inherits_legacy_sensitivity_and_diagonal_source() {
        let missing = "screen-settings-test-never-saved";
        let settings = get(missing, 5);
        assert_eq!(settings.source, (0.5, 1.0));
        assert_eq!(settings.target, (0.0, 1.0));
        assert_eq!(settings.sensitivity, PeerSettings::for_side(0).sensitivity);
    }

    #[test]
    fn validation_rejects_nonfinite_and_out_of_bounds() {
        let s = settings();
        for range in [
            (-0.1, 0.8),
            (0.3, 0.3),
            (0.8, 0.3),
            (0.1, 1.1),
            (f64::NAN, 1.0),
            (0.0, f64::INFINITY),
        ] {
            assert!(PeerSettings { source: range, ..s }.validate().is_err());
            assert!(PeerSettings { target: range, ..s }.validate().is_err());
        }
        for sensitivity in [0.19, 3.01, f64::NAN, f64::INFINITY] {
            assert!(PeerSettings { sensitivity, ..s }.validate().is_err());
        }
        for sensitivity in [0.2, 3.0] {
            assert!(PeerSettings { sensitivity, ..s }.validate().is_ok());
        }
    }

    #[test]
    fn batch_is_validated_before_any_setting_changes() {
        let first = "screen-settings-test-first".to_string();
        let second = "screen-settings-test-second".to_string();
        let before = settings();
        set(&first, before).unwrap();
        let changed = PeerSettings {
            sensitivity: 2.0,
            ..before
        };
        let invalid = PeerSettings {
            target: (0.7, 0.6),
            ..before
        };
        assert!(set_many(&[(first.clone(), changed), (second.clone(), invalid)]).is_err());
        assert_eq!(get(&first, 0), before);
        assert!(set_many(&[(first.clone(), changed), (first.clone(), before)]).is_err());
        set_many(&[(first.clone(), changed), (second.clone(), before)]).unwrap();
        assert_eq!(get(&first, 0), changed);
        assert_eq!(get(&second, 0), before);
        assert!(memory_only());
    }

    #[test]
    fn json_import_merges_only_after_full_validation() {
        let first = "screen-settings-json-first";
        let second = "screen-settings-json-second";
        set(first, settings()).unwrap();
        let next = json!({"version": 1, "peers": {second: {
            "source": [0.1, 0.5], "target": [0.3, 0.7], "sensitivity": 2.0
        }}});
        validate_json(&next).unwrap();
        import_json(&next).unwrap();
        assert_eq!(get(first, 0), settings(), "未記載peerは保持する");
        assert_eq!(snapshot_json()["version"], 1);
        assert_eq!(snapshot_json()["peers"][second], next["peers"][second]);
        let before = get(first, 0);
        let mut invalid = next.clone();
        invalid["peers"][first] = json!({
            "source": [0.8, 0.2], "target": [0.0, 1.0], "sensitivity": 1.0
        });
        assert!(import_json(&invalid).is_err());
        assert!(restore_json(&invalid).is_err());
        assert_eq!(get(first, 0), before);
        assert_eq!(snapshot_json()["peers"][second], next["peers"][second]);
        assert!(memory_only());
    }

    #[test]
    fn atomic_screen_write_uses_owner_only_file_and_rejects_symlink() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir =
            std::env::temp_dir().join(format!("knit-screen-atomic-test-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let path = dir.join("screen.json");
        write_atomic(&path, b"first").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        write_atomic(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        let link = dir.join("link.json");
        symlink(&path, &link).unwrap();
        assert!(write_atomic(&link, b"changed").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        // この試験でcreate_dir/canonicalizeしたディレクトリだけを片付ける。
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn malformed_saved_entries_are_rejected() {
        let v = json!({"version": 1, "peers": {"peer": {
            "source": [0.2, 0.6], "target": [0.4, 0.9], "sensitivity": 1.5
        }}});
        assert_eq!(parse(&v).unwrap()["peer"], settings());
        let mut invalid = v.clone();
        invalid["peers"]["peer"]["target"] = json!([0.9, 0.4]);
        assert!(parse(&invalid).is_err());
        invalid = v.clone();
        invalid["version"] = json!(2);
        assert!(parse(&invalid).is_err());
    }
}
