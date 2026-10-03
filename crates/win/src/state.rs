use crate::xfer::BULK_LINK;
use knit_common::proto::{encode, Msg};

pub(crate) static DEBUG_KEYS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// トレイ/バルーン表示用の接続状態(セッション確立で true)
pub(crate) static CONNECTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Mac から Cfg で同期される(KNIT_MUTE_SPK=0 で初期無効化)
/// 接続中の Windows スピーカーミュート(true=Mac のみ発音。既定 ON)。
pub(crate) static SPK_MUTE_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// Mac が測定した RTT(ms)。Mac から Stat で届く(ステータス窓の表示用)
pub(crate) static RTT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Windows 画面の位置(0=Macの右/1=左/2=上/3=下)。Mac から Cfg で同期
pub static SIDE_W: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
/// Windows 側で現在押下中のマウスボタン(後片付けの UP 注入を押下中のみに絞る。
/// 押されていないボタンへの UP は通常無害だが、一部アプリで意図しない
/// クリックとして扱われる懸念を排除する)
/// 送信チャネル(wtx)の共有: トレイの「Mac へ戻る」等から送るために
/// session の開始時に登録し、終了時に外す
pub static WTX: std::sync::Mutex<Option<std::sync::mpsc::Sender<String>>> =
    std::sync::Mutex::new(None);

/// 本線を外から張り直す合図(トレイ/昇格監視 → writer スレッド)。ワイヤには出ない
pub(crate) const MAIN_SHUTDOWN: &str = "\u{0}MAIN-SHUTDOWN";

/// 再接続の待機を外部イベント(スリープ復帰・電源状態変化)で割り込ませる
pub static WAKE: knit_common::retry::Wakeable = knit_common::retry::Wakeable::new();

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------- 接続の稼働計測(このプロセスの起動以降) ----------
// 診断の安定性表示の源。Mac 側(state.rs)と同じ方式で、ログの場所に依存しない
pub(crate) static METRIC_CONNECTS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);
pub(crate) static METRIC_DROPS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);
pub(crate) static METRIC_MAX_GAP_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub(crate) static METRIC_TOTAL_GAP_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
pub(crate) static BOOT_WALL_MS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// 起動時刻の記録(main から 1 回呼ぶ)
pub(crate) fn init_boot_wall() {
    BOOT_WALL_MS.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
}

/// 本線へ 1 行送る(トレイ・電源イベント・Esc 監視から使う)
pub(crate) fn send_main_msg(msg: &Msg) -> bool {
    WTX.lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|tx| tx.send(encode(msg)).is_ok())
}

/// 電源イベント(トレイの WM_POWERBROADCAST から)。resume=true はスリープ復帰。
/// 復帰後の再接続を read タイムアウト(最長 9 秒)待たずにすぐ始める
pub(crate) fn on_power_event(resume: bool) {
    println!(
        "[power] {}",
        if resume {
            "スリープ復帰を検出しました"
        } else {
            "スリープへ移行します"
        }
    );
    // クライアントループの待機を飛ばす
    WAKE.notify();
    // 古いセッションを能動的に切り、client_loop を次の試行へ進める。
    // MAIN_SHUTDOWN は writer スレッドが受け取り、read 側もエラーで終了する
    let _ = send_main_msg_raw(MAIN_SHUTDOWN);
    BULK_LINK.clear();
}

fn send_main_msg_raw(line: &str) -> bool {
    WTX.lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|tx| tx.send(line.to_string()).is_ok())
}

/// ログへ出してよい形へ整える(ピアが自由に送れる文字列の制御文字を置換。
/// ターミナルエスケープによる表示偽装=ログインジェクション防止)
pub(crate) fn log_safe(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// 「Mac へ戻る」用の Return 行(高さは画面中央相当)
pub fn proto_return() -> String {
    encode(&Msg::Return { ny: 0.5 })
}

// ---------- Mac の設定の遠隔操作(設定画面から) ----------
/// Mac から届いた設定一覧(接続中だけ有効)。キーは Mac の preferences.json と同じ
pub(crate) static MAC_PREFS: std::sync::Mutex<Option<serde_json::Value>> =
    std::sync::Mutex::new(None);

pub(crate) fn mac_pref(key: &str) -> Option<serde_json::Value> {
    MAC_PREFS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|v| v.get(key).cloned())
}

/// Mac へ設定一覧を要求する(応答は Msg::Prefs で届く)
pub(crate) fn request_mac_prefs() -> bool {
    send_main_msg(&Msg::PrefsGet)
}

/// Mac の設定を 1 項目変える。画面が戻りを待たずに追従するよう手元の一覧も先に更新し、
/// Mac が適用した結果の一覧(Msg::Prefs)で上書きされる
pub(crate) fn set_mac_pref(key: &str, value: serde_json::Value) {
    if let Some(obj) = MAC_PREFS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_mut()
        .and_then(|v| v.as_object_mut())
    {
        obj.insert(key.to_string(), value.clone());
    }
    let json = serde_json::json!({ key: value }).to_string();
    send_main_msg(&Msg::PrefsSet { json });
}
