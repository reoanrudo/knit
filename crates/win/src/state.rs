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
/// Mac の Cfg が指定する Windows 側の接続区間。None は旧版互換の全辺。
/// 2端点は同じロックで読み書きし、不正な Some も保持して「越境不可」にする。
static TARGET_RANGE_W: std::sync::Mutex<Option<[f64; 2]>> = std::sync::Mutex::new(None);

pub(crate) fn set_target_range(range: Option<[f64; 2]>) {
    *TARGET_RANGE_W.lock().unwrap_or_else(|e| e.into_inner()) = range;
}

/// Cfg更新の途中に、新旧のside/targetを混ぜてOLE側へ見せない。
pub(crate) fn set_connection_edge(side: u8, range: Option<[f64; 2]>) {
    let mut target = TARGET_RANGE_W.lock().unwrap_or_else(|e| e.into_inner());
    SIDE_W.store(side.min(7), std::sync::atomic::Ordering::Relaxed);
    *target = range;
}

pub(crate) fn connection_edge() -> (u8, Option<[f64; 2]>) {
    let target = TARGET_RANGE_W.lock().unwrap_or_else(|e| e.into_inner());
    (SIDE_W.load(std::sync::atomic::Ordering::Relaxed), *target)
}

/// 未指定だけ全辺にする。逆順・空・非有限・0..1外は安全側(接続区間なし)。
pub(crate) fn effective_target_range(range: Option<[f64; 2]>) -> Option<[f64; 2]> {
    let [start, end] = range.unwrap_or([0.0, 1.0]);
    (start.is_finite() && end.is_finite() && 0.0 <= start && start < end && end <= 1.0)
        .then_some([start, end])
}

pub(crate) fn target_range_contains(range: Option<[f64; 2]>, position: f64) -> bool {
    effective_target_range(range).is_some_and(|[start, end]| {
        position.is_finite() && start <= position && position <= end
    })
}

/// 辺に沿う既存の正規化座標。左右辺は上→下、上下辺は左→右(反転なし)。
/// 既存 Return/DragOffer と同じく長さ w/h で割る。画面外や無効な画面は拒否。
pub(crate) fn edge_position(side: u8, px: i32, py: i32, w: i32, h: i32) -> Option<f64> {
    if w <= 0 || h <= 0 || px < 0 || px >= w || py < 0 || py >= h {
        return None;
    }
    Some(if matches!(side, 2 | 3) {
        px as f64 / w as f64
    } else {
        py as f64 / h as f64
    })
}

/// 区間内にある実ピクセルの半開区間。包含端点と position=pixel/extent に合わせ、
/// 内側へ丸めて範囲外のピクセルを OLE の帯に含めない。
pub(crate) fn target_pixel_span(range: Option<[f64; 2]>, extent: i32) -> Option<(i32, i32)> {
    if extent <= 0 {
        return None;
    }
    let [start, end] = effective_target_range(range)?;
    let mut first = (start * extent as f64).ceil() as i32;
    let mut after = ((end * extent as f64).floor() as i64 + 1).min(extent as i64) as i32;
    // 浮動小数点の乗算が整数境界の反対側へ丸まった場合も、contains と一致させる。
    if first > 0 && target_range_contains(range, (first - 1) as f64 / extent as f64) {
        first -= 1;
    }
    if first < extent && !target_range_contains(range, first as f64 / extent as f64) {
        first += 1;
    }
    if after < extent && target_range_contains(range, after as f64 / extent as f64) {
        after += 1;
    }
    if after > 0 && !target_range_contains(range, (after - 1) as f64 / extent as f64) {
        after -= 1;
    }
    (first < after).then_some((first, after))
}
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

/// 単調時計の ms(プロセス起動からの経過)。経過時間・間引き・期限判定(再試行予定・
/// 通知の間引き・ドクターの連発制限)に使う。壁時計は NTP 補正やスリープ復帰で
/// 飛び・巻き戻るため、内部の比較には使わない(Mac 側と同じ原則の共通実装)
pub(crate) fn now_ms() -> u64 {
    knit_common::clock::mono_now_ms()
}

/// 壁時計の unix ms。ログのメトリクス行など、絶対時刻を表示する用途だけに使う。
/// 時間の長さは測らない(Mac 側 state::wall_ms と同じ使い分け)
pub(crate) fn wall_ms() -> u64 {
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

/// 起動時刻の記録(main から 1 回呼ぶ)。壁時計の絶対時刻で記録する
pub(crate) fn init_boot_wall() {
    BOOT_WALL_MS.store(wall_ms(), std::sync::atomic::Ordering::Relaxed);
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

#[cfg(test)]
mod target_range_tests {
    use super::{edge_position, effective_target_range, target_pixel_span, target_range_contains};

    #[test]
    fn missing_range_is_full_edge_and_invalid_ranges_fail_closed() {
        assert_eq!(effective_target_range(None), Some([0.0, 1.0]));
        for position in [0.0, 0.5, 1.0] {
            assert!(target_range_contains(None, position));
        }
        for range in [
            [-0.1, 0.5], [0.5, 1.1], [0.5, 0.5], [0.8, 0.2],
            [f64::NAN, 0.5], [0.2, f64::INFINITY], [f64::NEG_INFINITY, 0.8],
        ] {
            assert_eq!(effective_target_range(Some(range)), None);
            assert!(!target_range_contains(Some(range), 0.5));
            assert_eq!(target_pixel_span(Some(range), 1000), None);
        }
        for position in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
            assert!(!target_range_contains(None, position));
        }
        assert!(target_range_contains(Some([0.2, 0.8]), 0.2));
        assert!(target_range_contains(Some([0.2, 0.8]), 0.8));
        assert!(!target_range_contains(Some([0.2, 0.8]), 0.19));
        assert!(!target_range_contains(Some([0.2, 0.8]), 0.81));
    }

    #[test]
    fn edge_coordinates_keep_top_down_and_left_right_for_every_side() {
        for side in [0, 1, 4, 5, 6, 7] {
            assert_eq!(edge_position(side, 100, 250, 1000, 1000), Some(0.25));
            assert_eq!(edge_position(side, 900, 750, 1000, 1000), Some(0.75));
        }
        for side in [2, 3] {
            assert_eq!(edge_position(side, 250, 100, 1000, 1000), Some(0.25));
            assert_eq!(edge_position(side, 750, 900, 1000, 1000), Some(0.75));
        }
        for (px, py, w, h) in [(-1, 0, 10, 10), (0, 10, 10, 10), (10, 0, 10, 10),
            (0, 0, 0, 10), (0, 0, 10, -1)] {
            assert_eq!(edge_position(0, px, py, w, h), None);
        }
    }

    #[test]
    fn pixel_spans_include_exact_endpoints_and_never_expand_the_range() {
        assert_eq!(target_pixel_span(None, 1000), Some((0, 1000)));
        assert_eq!(target_pixel_span(Some([0.25, 0.75]), 1000), Some((250, 751)));
        assert_eq!(target_pixel_span(Some([0.251, 0.749]), 100), Some((26, 75)));
        assert_eq!(target_pixel_span(Some([0.251, 0.259]), 100), None);
        assert_eq!(target_pixel_span(Some([0.0, 0.5]), 1), Some((0, 1)));
        assert_eq!(target_pixel_span(Some([0.5, 1.0]), 1), None);
        assert_eq!(target_pixel_span(None, 0), None);
        assert_eq!(target_pixel_span(None, -1), None);
        // 端点がピクセルの比率と一致する/しない両方で、帯とreturn判定の同値性を検証。
        for extent in [1, 2, 3, 7, 100, 1080, 1920] {
            for start in 0..10 {
                for end in start + 1..=10 {
                    let range = Some([start as f64 / 10.0, end as f64 / 10.0]);
                    let span = target_pixel_span(range, extent);
                    for pixel in 0..extent {
                        assert_eq!(
                            span.is_some_and(|(first, after)| first <= pixel && pixel < after),
                            target_range_contains(range, pixel as f64 / extent as f64),
                            "extent={extent}, range={range:?}, pixel={pixel}"
                        );
                    }
                }
            }
        }
    }
}
