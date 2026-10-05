use crate::*;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

// ---------- 共有状態 ----------
pub(crate) static WIN_MODE: AtomicBool = AtomicBool::new(false);
pub(crate) static CONNECTED: AtomicBool = AtomicBool::new(false);
/// ⌘キーのマップ先(false=Ctrl 既定 / true=Alt)。トグル時に Windows へ Cfg で同期
pub(crate) static CMD_ALT: AtomicBool = AtomicBool::new(false);
/// 接続中の Windows スピーカーミュート(true=Mac のみ発音。既定 ON)。
/// トグル時に Windows へ Cfg で同期(KNIT_MUTE_SPK=0 で初期無効化)
pub(crate) static SPK_MUTE: AtomicBool = AtomicBool::new(true);
/// スクロール方向の反転(既定 false=Windows 標準の指の動きに合わせてある)
/// スクロール方向の手動上書き(true=Windows 標準。false 既定=Mac の設定に合わせる)
pub(crate) static SCROLL_FLIP: AtomicBool = AtomicBool::new(false);
/// macOS の「自然スクロール」設定(起動時に取得。true=トラックパッドのコンテンツ追従)
pub(crate) static NATURAL_SCROLL: AtomicBool = AtomicBool::new(true);

/// macOS のスクロール方向設定を読む(失敗時は出荷既定の自然スクロール扱い)。
/// ユーザーが Mac で使っている向きへ Windows 側も自動で合わせるために使う
pub(crate) fn detect_natural_scroll() -> bool {
    std::process::Command::new("defaults")
        .args(["read", "-g", "com.apple.swipescrolledirection"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() != "0")
        .unwrap_or(true)
}
/// Windows との RTT(ms)。ping/pong 往復で測定(メニュー状態行の表示用)
pub(crate) static RTT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 相手(Windows)のプロトコル版。hello/hello_ok で更新(版による機能のON/OFFに使う)。
/// 複数台接続では PeerEntry の版が正であり、ここには「アクティブな相手」の版だけを
/// 反映する(activate_peer_locked / on_disconnect で管理)
pub(crate) static PEER_VERSION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// 接続先の登録(ペアリング)が済んでいるか。未接続時の案内を
/// 「初回登録へ導く」か「再接続を待つ」かに分けるために使う(main で立つ)
pub(crate) static PAIRED: AtomicBool = AtomicBool::new(false);
/// 再接続の待機を外部イベント(スリープ復帰・ネットワーク変化)で割り込ませる
pub static WAKE: knit_common::retry::Wakeable = knit_common::retry::Wakeable::new();

/// 壁時計の unix ms。ログのメトリクス行など、プロセス再起動を跨いで時刻を
/// 比較する用途に使う。内部のダブルタップ判定・pong 監視は単調時計の
/// now_ms(main.rs)を使い、ここは分けておく(スリープ復帰で壁時計は飛ぶ)
pub(crate) fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------- 接続の稼働計測(このプロセスの起動以降) ----------
// 診断・安定性表示の源。過去・長期の集計はログ解析(stability-report.sh)が担い、
// ここは「今の稼働」を実測する。ログ解析はログの場所や保持に依存するため、
// Windows 側(mac とは異なり exe 自身がログの場所を知らない)とも共通の方式にする
pub(crate) static METRIC_CONNECTS: AtomicU32 = AtomicU32::new(0);
pub(crate) static METRIC_DROPS: AtomicU32 = AtomicU32::new(0);
pub(crate) static METRIC_MAX_GAP_MS: AtomicU64 = AtomicU64::new(0);
pub(crate) static METRIC_TOTAL_GAP_MS: AtomicU64 = AtomicU64::new(0);
/// プロセス起動時刻(壁時計)。稼働計測の窓の表示に使う(main から初期化)
pub(crate) static BOOT_WALL_MS: AtomicU64 = AtomicU64::new(0);

/// 起動時刻の記録(main から 1 回呼ぶ)
pub(crate) fn init_boot_wall() {
    BOOT_WALL_MS.store(wall_ms(), Ordering::Relaxed);
}
pub(crate) static TX: OnceLock<Sender<outgoing::Queued>> = OnceLock::new();
pub(crate) static OUTBOUND_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static STREAM_SLOT: OnceLock<Arc<Mutex<Option<secure::Writer>>>> = OnceLock::new();
pub(crate) static TAP_PORT: OnceLock<usize> = OnceLock::new();
pub(crate) static DIAG_MOVE_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static DIAG_KEY_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static DIAG_SEND_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static DIAG_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) static DIAG_WARP_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static DIAG_MODE_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static DIAG_SCROLL_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static DIAG_ABS_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static DIAG_SELF_HEAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static TAP_REARM_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// ウォッチドッグ用: 最終タップ受信時刻と最終 abs 送信時刻(ms)。
/// 「ユーザーが操作中なのに Windows へ届いていない」状態を検知する
pub(crate) static LAST_EVENT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static LAST_ABS_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 境界ダブルタップ切替(Deskflow switchDoubleTap 相当)。
/// KNIT_EDGE_TAPS(既定2)= 境界に連続で2回当てた時だけ切替。1回の到達では
/// 切替しないため、境界付近での日常作業と Windows への移動が分離される。
/// GUI から実行中に切り替え可能なため AtomicU32(初期値は起動時に store)
pub(crate) static EDGE_TAPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(2);
pub(crate) static EDGE_AT_EDGE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) static EDGE_LAST_HIT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
// ---- Deskflow 標準オプション(画面位置 links / switchDelay / switchDoubleTap /
// ---- switchCorners / clipboardSharing)----
/// Windows 画面の位置(0=右/1=左/2=上/3=下)。Deskflow links 相当
pub static SIDE: AtomicU8 = AtomicU8::new(0);
/// 端に N ms 滞ってから切替(switchDelay。0=無効でダブルタップ/即時判定)
pub static SWITCH_DELAY_MS: AtomicU64 = AtomicU64::new(0);
/// ダブルタップの判定窓 ms(switchDoubleTap)
pub static DOUBLE_TAP_MS: AtomicU64 = AtomicU64::new(700);
/// 四隅の切替禁止サイズ px(switchCornerSize。0=無効)
pub static CORNER_PX: AtomicU64 = AtomicU64::new(0);
/// クリップボード共有(clipboardSharing)
pub static CLIP_SHARE: AtomicBool = AtomicBool::new(true);
/// スクロール互換モード(KNIT_SCROLL_COMPAT=1 / 設定「操作」のチェック): 120 未満の
/// ホイール量を無視する古い設計のアプリ向けに 1 ノッチ(120)単位で送る。
/// 既定 OFF=高解像度(0.05 ノッチ刻み)で滑らかに
pub static SCROLL_COMPAT: AtomicBool = AtomicBool::new(false);
/// 端到達の開始時刻(switchDelay の滞在計測用)
pub(crate) static EDGE_STAY_SINCE_MS: AtomicU64 = AtomicU64::new(0);
/// 境界到達の許容誤差(px)。OS はカーソルを画面端で止めるため、越えは
/// 「境界そのもの」(距離 0)でのみ発火させる。これは小数の丸め誤差ぶんだけの許容
pub(crate) const EDGE_BOUNDARY_TOL: f64 = 0.5;
/// 横スワイプ(戻る/進む)の状態: (累積 dx, 最終イベント時刻, 最終発火時刻)
pub(crate) static SWIPE_ACC: std::sync::Mutex<(f64, u64, u64)> = std::sync::Mutex::new((0.0, 0, 0));
/// ドラッグ中切替(KNIT_DRAG_SWITCH=1): 押したまま境界を越えられる
pub static DRAG_SWITCH: AtomicBool = AtomicBool::new(false);
/// 速度越境(Crossing Intelligence・ビジョン§24): 境界への速度が十分大きい越えは
/// 滞在待ち/ダブルタップをスキップする。`KNIT_FAST_EDGE=0` で無効化
pub static FAST_EDGE: AtomicBool = AtomicBool::new(true);
/// Mac 流ショートカット翻訳(KNIT_MAC_KEYS=0 で無効)。タップ内で毎イベント
/// 設定を引かないよう起動時にキャッシュする
pub(crate) static MAC_KEYS: AtomicBool = AtomicBool::new(true);
/// 2本指横スワイプ→戻る/進む(KNIT_SWIPE_NAV=0 で横ホイールのまま)
pub(crate) static SWIPE_NAV: AtomicBool = AtomicBool::new(true);
/// 現在押下中のマウスボタン(0=左,1=右,2=中)。切替時の持ち込み再送に使う
pub(crate) static BTN_DOWN: [AtomicBool; 3] = [
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
];
/// 接続する境界の範囲(境界に沿った位置の割合 0..1)。Mac の「ディスプレイ配置」
/// と同じ発想: Windows 画面が Mac の端のどの範囲に接しているか。
/// 斜め(4-7)は半分、それ以外は全域。配置エディタのドラッグで更に細かく決まる
pub static LAY_RANGE: std::sync::Mutex<(f64, f64)> = std::sync::Mutex::new((0.0, 1.0));
pub(crate) static LAST_PONG_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 復帰直後は右端判定を一定時間無効化する(再突入チャタリング防止)
pub(crate) static EDGE_GUARD_UNTIL_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 別端末へ入った直後の時刻(0=なし)。入り直後の過剰な移動で即座に Mac へ
/// 戻されてしまう(「押し戻される」報告)のを防ぐ、戻り判定の抑制に使う
/// 境界から出てくる位置を境界ちょうどにしたため、入った(戻った)直後の小さな動きで
/// すぐ逆向きに切り替わらないよう、境界から一定距離離れるまで切替を受け付けない。
/// 入った直後は Windows 側の戻り判定、戻った直後は Mac 側の再突入判定を止める
pub(crate) static RETURN_ARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
pub(crate) static REENTRY_ARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// 境界から離れたとみなす距離(px)
pub(crate) const EDGE_REARM_PX: f64 = 24.0;
pub(crate) static ENTER_GUARD_UNTIL_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 前回の権限チェック時刻(watchdog の権限喪失保険用)
pub(crate) static PERM_CHECK_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// 自前管理のカーソル位置(delta 積算)。タップ内での毎イベント CGEventCreate は
/// 負荷としてカクつきに効くため、積算+間欠同期(Deskflow の m_xCursor 方式)にする。
pub(crate) static CUR_POS: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
/// 直近 100ms の移動量(px)と窓の開始時刻(ms)。切替時の速度計装用
///(§24 Crossing Intelligence: 実機の速い/遅い到達の分布を見てから誤 Cross
/// 判定のしきい値を設計する。現状は判定には使わない)
pub(crate) static RECENT_PX: Mutex<(f64, u64)> = Mutex::new((0.0, 0));

/// 100ms 窓の移動量から速度(px/秒)を引く(計装ログと単体テストで使う)
pub(crate) fn px_per_sec(px: f64, window_ms: u64) -> f64 {
    if window_ms == 0 {
        return 0.0;
    }
    px * 1000.0 / window_ms as f64
}
/// 接続相手(Windows)の画面サイズ(px)。hello で受信しスケール自動算出に使う
pub(crate) static WIN_SCREEN: Mutex<(f64, f64)> = Mutex::new((1920.0, 1080.0));
/// WIN モード中の Windows 仮想カーソル位置(px)。絶対位置送信モードで使う
pub(crate) static WIN_CUR: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));

/// Windows の解像度が変わった(Screen)とき、仮想カーソルを旧画面内の比率の
/// 位置へ写し直す。補正しないと旧サイズの絶対 px のまま残り、縮小時は境界へ
/// 張り付いて side=1(右配置)の誤帰還、拡大時は位置が飛ぶ
pub(crate) fn rescale_win_cur(wc: (f64, f64), old: (f64, f64), new: (f64, f64)) -> (f64, f64) {
    if old.0 <= 0.0 || old.1 <= 0.0 || new.0 <= 0.0 || new.1 <= 0.0 {
        return wc;
    }
    (wc.0 * new.0 / old.0, wc.1 * new.1 / old.1)
}
/// 絶対位置送信モード(既定ON。KNIT_MOUSE_MODE=rel で旧・相対移動に戻す)
pub(crate) static MOUSE_ABS_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// 切替方式: false=境界+ホットキー(既定)/ true=ホットキー(F13)のみで切替、
/// 切替後は境界を超えても戻らないロック状態になる(KNIT_SWITCH_MODE=hotkey)
pub(crate) static HOTKEY_ONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) static CUR_SYNC_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// ライブカーソル位置を取得(CFRelease まで面倒を見る)
pub(crate) unsafe fn live_cursor() -> Option<CGPoint> {
    let probe = CGEventCreate(std::ptr::null_mut());
    if probe.is_null() {
        return None;
    }
    let loc = CGEventGetLocation(probe);
    CFRelease(probe);
    Some(loc)
}
/// WIN モード中のカーソル固定位置(右端内側, y)。漏れ移動を warp で巻き戻す基準。
pub(crate) static LOCK_POS: Mutex<Option<(f64, f64)>> = Mutex::new(None);
/// スクロール変換の累積残高(dx, dy)[ノッチ]。除数を大きくしても細かい動きを失わないための仕組み。
pub(crate) static SCROLL_ACC: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
/// スクロール速度除数(ピクセル→ノッチ変換。大きいほど遅い)。設定ウィンドウの
/// スライダーからも可変(KNIT_SCROLL_DIV は初期値)。f64 を AtomicU64 ビットで保持
pub(crate) static SCROLL_DIV: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(60.0f64.to_bits());

/// 現在のスクロール除数を f64 で読む
pub fn scroll_div() -> f64 {
    f64::from_bits(SCROLL_DIV.load(Ordering::Relaxed))
}

/// スクロール除数を設定(20..240 にクランプ)。設定ウィンドウから呼ばれる
pub fn set_scroll_div(v: f64) {
    let clamped = v.clamp(20.0, 240.0);
    SCROLL_DIV.store(clamped.to_bits(), Ordering::Relaxed);
}
/// マウス移動の倍率(Mac の加速済み delta に Windows の加速が重なる調整用)。
/// カーソル速度倍率(abs 座標系)。設定「操作」のカーソル速度スライダーから可変。
/// f64 を AtomicU64 ビットで保持(スクロール除数と同じ方式)
pub(crate) static MOUSE_SCALE: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1.0f64.to_bits());

/// 現在のカーソル速度倍率
pub fn mouse_scale() -> f64 {
    f64::from_bits(MOUSE_SCALE.load(Ordering::Relaxed))
}

/// カーソル速度倍率を設定(0.2..3.0 にクランプ)
pub fn set_mouse_scale(v: f64) {
    MOUSE_SCALE.store(v.clamp(0.2, 3.0).to_bits(), Ordering::Relaxed);
}
