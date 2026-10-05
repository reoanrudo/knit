use crate::*;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// 接続相手の名前(hello で受け取る)。通知へ出す
pub static PEER_NAME: Mutex<String> = Mutex::new(String::new());
/// 最前面アプリと Windows のアプリ列挙を名前で照合する(越境 App Handoff 用)。
/// 完全一致(大小無視)を優先し、次に Windows 側名前の部分一致(Mac 名が
/// 4 文字以上のときだけ。短い名前の誤爆を防ぐ下限)
pub(crate) fn app_handoff_match<'a>(
    mac_name: &str,
    win_apps: &'a [(String, String)],
) -> Option<&'a (String, String)> {
    let mac = mac_name.trim().to_lowercase();
    if mac.is_empty() {
        return None;
    }
    if let Some(hit) = win_apps
        .iter()
        .find(|(n, _)| n.trim().to_lowercase() == mac)
    {
        return Some(hit);
    }
    if mac.chars().count() >= 4 {
        if let Some(hit) = win_apps
            .iter()
            .find(|(n, _)| n.trim().to_lowercase().contains(&mac))
        {
            return Some(hit);
        }
    }
    None
}

/// Windows へ入る時の App Handoff: 最前面アプリを相手でも開く。
/// クリップボード同期とは独立した実験的機能
pub(crate) fn try_app_handoff() {
    if !APP_HANDOFF.load(Ordering::Relaxed) || !CONNECTED.load(Ordering::Relaxed) {
        return;
    }
    let name = with_pool(|| unsafe { mac_frontmost_app_name() }).unwrap_or_default();
    if name.is_empty() {
        return;
    }
    let win_apps = WIN_APPS.lock().map(|a| a.clone()).unwrap_or_default();
    if win_apps.is_empty() {
        // 列挙が未取得なら照合できない。要求だけ送って次回に備える
        eprintln!("[handoff] Windows のアプリ一覧が未取得のため照合を飛ばします");
        send_msg(&Msg::AppsQuery);
        return;
    }
    match app_handoff_match(&name, &win_apps) {
        Some((win_name, path)) => {
            eprintln!("[handoff] Mac 最前面「{name}」→ Windows「{win_name}」を起動します");
            let _ = send_msg_reported(&Msg::RunApp { path: path.clone() });
        }
        None => eprintln!("[handoff] Mac 最前面「{name}」に対応する Windows アプリはありません"),
    }
}
/// 本線がいま繋がっている Windows のアドレス(大容量経路の接続先・経路診断に使う)
pub(crate) static PEER_IP: Mutex<Option<std::net::IpAddr>> = Mutex::new(None);

// ---- 複数台接続(ビジョン: 台数制限の撤回)。Mac=サーバは複数の Windows を
// 同時に保持し、アクティブな 1 台へ入力を送る。切替はメニューバーの「接続先」から ----

/// 接続中の相手 1 台分。writer はアクティブ時のみ STREAM_SLOT へ貸し出し、
/// 非アクティブ時はここで待機する
pub(crate) struct PeerEntry {
    /// 端末識別子(hello の id。旧版相手は IP 由来の代替値)
    pub id: String,
    pub name: String,
    pub ip: std::net::IpAddr,
    /// 相手の代表画面サイズ(スケール算出用)
    pub screen: (f64, f64),
    /// 相手の全モニター構成(版 13 以降で自動交換。旧版相手は空)
    pub monitors: Vec<knit_common::proto::Monitor>,
    /// 非アクティブ時の送信口(アクティブ時は None: STREAM_SLOT が保持する)
    pub writer: Option<secure::Writer>,
    /// セッションの世代(同一端末の再接続で置き換えを判別する。大きいほど新しい)
    pub gen: u64,
    /// この端末を置く画面の辺(0=右/1=左/2=上/3=下)。端末ごとに独立して持ち、
    /// その辺へカーソルをやるとこの端末へ入る(複数端末を同時接続して住み分ける)
    pub side: u8,
    /// 対象のモニター番号(mac_displays() の並び。0=メイン)。None=全画面の端。
    /// 例: タブレットはメインモニターの左/外部モニターの右…を個別に選べる
    pub edge_monitor: Option<usize>,
    /// 相手のプロトコル版(hello で受信)。機能判定はこの端末の版で行う。
    /// 待機中の端末の版でグローバル(PEER_VERSION)を上書きしないために持つ
    pub ver: u32,
    /// 利用者が付けた表示名(エイリアス)。設定「接続」の「選択中の端末の名前」で
    /// 端末ごとに peer-sides.json へ保存し、表示・履歴ラベルでコンピュータ名(name)
    /// より優先する。None・空=設定なし(コンピュータ名を使う)
    pub alias: Option<String>,
}
pub(crate) static PEERS: Mutex<Vec<PeerEntry>> = Mutex::new(Vec::new());
/// アクティブなピアの添字(未接続は usize::MAX)
pub(crate) static ACTIVE_PEER: Mutex<usize> = Mutex::new(usize::MAX);
/// セッション世代の採番(同一端末への重複接続で、古い方を正しく破棄するため)
pub(crate) static PEER_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
// 接続の登録・切断・選択を直列化し、一覧の前詰めと送信口の所有者を一致させる。
pub(crate) static PEER_CHANGES: Mutex<()> = Mutex::new(());

/// my_id がいまアクティブなピアか(pong の反映先判定などに使う)。
/// 空 id はクライアントモード(単一接続)を表し、常にアクティブ扱い
pub(crate) fn is_active_peer(my_id: &str, generation: u64) -> bool {
    if my_id.is_empty() {
        return true;
    }
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    peers.get(act).is_some_and(|p| p.id == my_id && p.gen == generation)
}

/// アクティブな接続先が Android タブレットか(設定画面の文言・機能の表示分け。
/// Android 中継の hello は id が "android-" で始まる)
pub(crate) fn active_android_app_permissions() -> Option<(bool,bool)> {
    let peers=PEERS.lock().unwrap_or_else(|e|e.into_inner());
    let act=*ACTIVE_PEER.lock().unwrap_or_else(|e|e.into_inner());
    let id=&peers.get(act)?.id;
    if !id.starts_with("android-app-") { return None; }
    Some(android::app::get(id).unwrap_or((false,false)))
}

pub(crate) fn active_peer_is_android_app() -> bool {
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    peers.get(act).is_some_and(|p| p.id.starts_with("android-app-"))
}

pub(crate) fn active_peer_is_android() -> bool {
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    peers.get(act).is_some_and(|p| p.id.starts_with("android-"))
}

/// 端末の表示名の選択(純粋関数)。エイリアス(利用者が付けた名前)があれば
/// それ、無ければ hello で受け取ったコンピュータ名
pub(crate) fn peer_label_of<'a>(alias: Option<&'a str>, name: &'a str) -> &'a str {
    alias.filter(|a| !a.is_empty()).unwrap_or(name)
}

/// 端末の表示名。エイリアス優先(peer_label_of と同じ規則)
pub(crate) fn peer_label(p: &PeerEntry) -> &str {
    peer_label_of(p.alias.as_deref(), &p.name)
}

/// ポップアップ・配置エディタ向けの表示ラベル「名前 (IP)」。同じコンピュータ名の
/// 端末が複数台あるとき IP で区別できるようにする(純粋関数・単体テストで守る)
pub(crate) fn peer_display_label(alias: Option<&str>, name: &str, ip: &str) -> String {
    format!("{} ({})", peer_label_of(alias, name), ip)
}

/// 相手から受信したもののクリップボード履歴ラベル。接続中の端末は
/// エイリアス/コンピュータ名で載せる(同じラベルで複数台を区別できない
/// ため)。Android アプリはエイリアスが無い間は従来どおり「タブレット」、
/// 未接続・未知の id は "端末"(name 受信前の汎用表示)。旧履歴の "Windows"・
/// 「タブレット」の値はそのまま表示互換
pub(crate) fn history_device_for_peer(peer_id: &str) -> String {
    if peer_id.is_empty() {
        return "端末".into();
    }
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = peers.iter().find(|p| p.id == peer_id) {
        if let Some(a) = p.alias.as_deref().filter(|a| !a.is_empty()) {
            return a.to_string();
        }
        if !peer_id.starts_with("android-app-") {
            return p.name.clone();
        }
        return "タブレット".into();
    }
    if peer_id.starts_with("android-app-") {
        "タブレット".into()
    } else {
        "端末".into()
    }
}

/// 大容量経路(ファイル・画像)は選択中の端末から届くため、アクティブな
/// 接続先の id で判別する(ロック順は STREAM_SLOT → PEERS → ACTIVE_PEER)。
/// id だけ先に取り出して PEERS を離してから history_device_for_peer へ渡す:
/// こちらは PEERS を保持したまま呼ぶと、先方が内部で PEERS を再取得して
/// 自己デッドロックする(std::sync::Mutex は再入不可能なため)
pub(crate) fn history_device_for_active() -> String {
    let id = {
        let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        peers
            .get(act)
            .map(|p| p.id.clone())
            .unwrap_or_default()
    };
    history_device_for_peer(&id)
}

/// bulk 受信バッチの開始時点の履歴ラベル。通知時点でアクティブ端末を読むと、
/// 受信中の端末切替でラベルが「完了時のアクティブ端末」にすり替わるため、
/// バッチの開始時(on_batch_begin)で固定して使う
static BULK_RX_DEVICE: Mutex<String> = Mutex::new(String::new());

/// bulk 受信バッチの開始時点の履歴ラベルを現在のアクティブ端末で記録する
///(common の Endpoint.on_batch_begin から呼ばれる)
pub(crate) fn note_bulk_rx_device() {
    *BULK_RX_DEVICE.lock().unwrap_or_else(|e| e.into_inner()) = history_device_for_active();
}

#[cfg(test)]
/// PEERS を入れ替えるテスト(conn::tests・gui::prefs::layout_values_tests)の
/// 直列化ロック。プロセス共有の static を複数テストが並行で入れ替えると
/// 互いに他人の PEERS を覗いて間欠失敗するため、置換中の区間を排他する
pub(crate) static PEERS_TEST_SERIAL: Mutex<()> = Mutex::new(());

/// 記録した bulk 受信バッチ開始時点の履歴ラベル(未記録の初回は "端末")
pub(crate) fn bulk_rx_device() -> String {
    let label = BULK_RX_DEVICE.lock().unwrap_or_else(|e| e.into_inner());
    if label.is_empty() {
        "端末".into()
    } else {
        label.clone()
    }
}

/// 端末切替で bulk 経路を張り替えた(clear した)時刻(単調時計の ms・0=未発生)。
/// 接続側は 2 秒毎に張り直すため、切替直後の未接続はまもなく回復する見込みが
/// ある。掴みドラッグの offer がこの窓を猶予するかの判定に使う
pub(crate) static BULK_SWITCHED_AT_MS: AtomicU64 = AtomicU64::new(0);

/// 端末切替時の bulk 張替延期の保留(世代ガード付き)。受信中に切替があると
/// clear を遅らせるが、旧実装は切替のたびに監視スレッドを spawn していたため、
/// 期限後に「新しく張った経路」を後から切る競合が起きた。保留は切替時点の
/// 本線世代(OUTBOUND_GENERATION)で持ち、周期処理(keepalive スレッド)が
/// 世代が一致するときだけ clear する(0=予約なし)
static BULK_DEFER_GEN: AtomicU64 = AtomicU64::new(0);
/// 保留の期限(単調時計の ms。この時刻を過ぎたら受信中でも切る)
static BULK_DEFER_DEADLINE_MS: AtomicU64 = AtomicU64::new(0);

/// bulk 張替延期の保留判定(周期処理から呼ぶ純関数。単体テストで守る)。
/// 予約世代が現在も使われているときだけ clear を許し、別の切替で世代が
/// 増えていれば古い予約として破棄する(新しい経路を後から切らない)
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BulkDeferAction {
    /// 予約なし、または受信が続いて期限内。次の周期まで待つ
    Pending,
    /// 受信が終わったか期限切れ。予約した経路を切ってよい
    Clear,
    /// 世代が置き換わった(新しい切替が起きた)。予約を捨てる
    Stale,
}

pub(crate) fn bulk_defer_clear_action(
    booked_gen: u64,
    current_gen: u64,
    deadline_ms: u64,
    now_ms: u64,
    rx_active: bool,
) -> BulkDeferAction {
    if booked_gen == 0 {
        return BulkDeferAction::Pending;
    }
    if current_gen != booked_gen {
        return BulkDeferAction::Stale;
    }
    if !rx_active || now_ms >= deadline_ms {
        return BulkDeferAction::Clear;
    }
    BulkDeferAction::Pending
}

/// アクティブな接続先の表示名(エイリアス優先。未接続・name 受信前は "端末")
pub(crate) fn active_peer_label() -> String {
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    peers
        .get(act)
        .map(|p| peer_label(p).to_string())
        .unwrap_or_else(|| {
            let name = PEER_NAME.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if name.is_empty() { "端末".into() } else { name }
        })
}

/// ピアをアクティブへ切り替える(送信口・画面・設定同期・通知)。
/// ロック順は STREAM_SLOT → PEERS → ACTIVE_PEER で統一し、逆順で取得しないこと
pub(crate) fn activate_peer(new: usize, reason: &str) {
    let _change = PEER_CHANGES.lock().unwrap_or_else(|e| e.into_inner());
    activate_peer_locked(new, reason);
}

pub(crate) fn activate_peer_by_id(id: &str, reason: &str) {
    let _change = PEER_CHANGES.lock().unwrap_or_else(|e| e.into_inner());
    activate_peer_id_locked(id, reason);
}

pub(crate) fn activate_peer_id_locked(id: &str, reason: &str) {
    let index = PEERS.lock().unwrap_or_else(|e| e.into_inner()).iter().position(|p| p.id == id);
    if let Some(index) = index { activate_peer_locked(index, reason); }
}

/// 切り替え要求に対する今の状態。writer の一時不在は戻りを待つ価値があり、
/// 対象が無い・既にアクティブなら従来どおり何もしない
enum PeerSwitchReadiness {
    /// writer がある。切り替えを実行できる
    Ready,
    /// writer が一時的に取り出されている(ping 送信中)。戻りを待つ
    Wait,
    /// 対象が一覧に無い・既にアクティブ。何もしない(従来どおり)
    Stop,
}

/// ロックは取るたびに離す(戻し入れ側が PEERS ロックを必要とするため、
/// 掴んだまま待つと deadlock する。ロック順は slot → PEERS → ACTIVE_PEER)
fn peer_switch_readiness(new: usize) -> PeerSwitchReadiness {
    let Some(slot_arc) = STREAM_SLOT.get() else {
        return PeerSwitchReadiness::Stop;
    };
    let slot = slot_arc.lock().unwrap_or_else(|e| e.into_inner());
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    if new >= peers.len() {
        return PeerSwitchReadiness::Stop;
    }
    if peers[new].writer.is_some() {
        return PeerSwitchReadiness::Ready;
    }
    if act == new && slot.is_some() {
        return PeerSwitchReadiness::Stop;
    }
    PeerSwitchReadiness::Wait
}

fn activate_peer_locked(new: usize, reason: &str) {
    // 待機ピアへの ping は writer を一時的に取り出す(keepalive_inactive_peers)。
    // ちょうど取り出し中に切り替え要求が来ると、従来は writer.is_none() で
    // 無言で return していた=「選んだのに何も起きない」。ping の書き込みは通常
    // 一瞬で終わるため、短い再取得待ちを入れる(合計 2 秒・50ms 間隔。これでも
    // 戻らなければ相手は半開き等で応答不能=一覧から外れる)
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        match peer_switch_readiness(new) {
            PeerSwitchReadiness::Ready => break,
            PeerSwitchReadiness::Stop => return,
            PeerSwitchReadiness::Wait => {}
        }
        if std::time::Instant::now() >= deadline {
            eprintln!("[conn] 接続先へ切り替えられませんでした(writer が戻りません idx={new})");
            // 自動系の切り替えは頻発し得るため、明示的な選択の失敗だけ通知する。
            // この失敗は相手が半開き等で応答不能=待っても直らないため、相手側の
            // 起動確認と、この端末が一覧から外れること(再選択は無意味)を伝える
            if !reason.contains("再接続") && !reason.contains("自動切替") {
                notify(
                    "Knit",
                    "切り替えられませんでした(相手が応答しません)。相手側アプリの起動を確認してください。この端末はまもなく一覧から消えます",
                );
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let slot_arc = STREAM_SLOT.get().unwrap();
    let mut slot = slot_arc.lock().unwrap_or_else(|e| e.into_inner());
    let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let mut act = ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    // 待ちの間に writer が再び取り出された場合の最終防護(従来と同じ無言 return)
    if new >= peers.len() || (*act == new && slot.is_some()) || peers[new].writer.is_none() {
        return;
    }
    OUTBOUND_GENERATION.fetch_add(1, Ordering::SeqCst);
    // この切替の世代(延期予約のガードに使う。以降の切替・切断で増える)
    let generation = OUTBOUND_GENERATION.load(Ordering::SeqCst);
    if *act < peers.len() {
        if let Some(mut w) = slot.take() {
            // 古いキューを捨てても、旧端末に押下を残さない。
            // 本線のロック内でLeaveを送り終えてから待機端末へ戻す。
            use std::io::Write as _;
            if peers[*act].id.starts_with("android-app-") {
                let _=w.write_all(encode(&Msg::Selected {on:false}).as_bytes());
            }
            if w.write_all(encode(&Msg::Leave).as_bytes()).and_then(|_| w.flush()).is_ok() {
                peers[*act].writer = Some(w);
            } else {
                w.shutdown();
            }
        }
    }
    let Some(w) = peers[new].writer.take() else {
        return;
    };
    *slot = Some(w);
    let (name, ip, screen, ver, side) = (
        peer_label(&peers[new]).to_string(),
        peers[new].ip,
        peers[new].screen,
        peers[new].ver,
        peers[new].side,
    );
    *act = new;
    drop(act);
    drop(peers);
    drop(slot);
    // アクティブな相手の版だけを機能判定に使う(待機端末の版で壊さない)。
    // 相対移動(ゲームモード)は前の相手の状態を引き継がない
    PEER_VERSION.store(ver, Ordering::Relaxed);
    if GAME_REL.swap(false, Ordering::Relaxed) {
        eprintln!("[game] 接続先を切り替えたため相対移動を解除しました");
    }
    // この端末の配置(斜めなら辺の半分)を境界判定へ反映する
    *LAY_RANGE.lock().unwrap_or_else(|e| e.into_inner()) = crate::tap::side_lay_range(side);
    *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner()) = screen;
    *PEER_NAME.lock().unwrap_or_else(|e| e.into_inner()) = name.clone();
    *PEER_IP.lock().unwrap_or_else(|e| e.into_inner()) = Some(ip);
    mark_connected();
    LAST_PONG_MS.store(now_ms(), Ordering::Relaxed);
    RTT_MS.store(0, Ordering::Relaxed);
    // 旧相手のファイル転送経路を切る(接続側は本線の接続先へ追従して張り直す)。
    // 受信バッチの進行中は完了まで延期する: 入力の切替は即時、bulk 経路の張替は
    // 受信が終わってから(進行中に切ると FILE_END までの分しか残らず、途中で
    // 切れた旨の部分報告だけが残る)。張替の猶予を付けないと、掴みドラッグも
    // 切替直後は必ず未接続で拒否されることと同じ構造になる
    BULK_SWITCHED_AT_MS.store(now_ms(), Ordering::Relaxed);
    if knit_common::bulk::rx_active() {
        eprintln!("[conn] 受信中のため bulk 経路の張替えを受信完了後へ延期します");
        // 保留は切替時点の本線世代で記録し、keepalive の周期処理(1 秒毎)で判定
        // して clear する。旧実装のスレッド spawn は切替のたびに並走し、期限後に
        // 新しく張った経路を後から切る競合があった(世代が一致するときだけ切る)
        BULK_DEFER_GEN.store(generation, Ordering::SeqCst);
        BULK_DEFER_DEADLINE_MS.store(now_ms() + 60_000, Ordering::Relaxed);
    } else {
        BULK_LINK.clear();
    }
    send_cfg();
    if active_peer_is_android_app() { send_msg(&Msg::Selected {on:true}); }
    eprintln!("[conn] 接続先を {name} へ切り替え({reason})");
    // 再接続・経路昇格の置き換えは頻発するため通知は出さない(明示的な切替だけ知らせる)
    if !reason.contains("再接続") {
        notify("Knit", &format!("{name} へ切り替えました({reason})"));
    }
}

/// 非アクティブピアへの間欠 ping(生存確認)。書けなくなった相手は一覧から外す。
/// アクティブ 1 台の旧構成では「繋いでいない相手が黙って消える」検知が無かった。
/// 間隔は 1 秒: 相手側は「9 秒無通信で経路断」とみなすため、余裕を大きく取る
/// (3 秒間隔でも OS の送信遅延が重なると 9 秒に届かず切れることがあった)
pub(crate) fn keepalive_inactive_peers() {
    std::thread::spawn(|| {
        use std::io::Write as _;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            // bulk 張替延期の保留判定(受信中に切り替えた端末の clear 予約)。
            // 世代ガードで「予約した時の切替」が現在も使われているときだけ
            // clear する。判定自体は純関数(bulk_defer_clear_action)
            let booked = BULK_DEFER_GEN.load(Ordering::SeqCst);
            if booked != 0 {
                let current = OUTBOUND_GENERATION.load(Ordering::SeqCst);
                let deadline = BULK_DEFER_DEADLINE_MS.load(Ordering::Relaxed);
                match bulk_defer_clear_action(
                    booked,
                    current,
                    deadline,
                    now_ms(),
                    knit_common::bulk::rx_active(),
                ) {
                    BulkDeferAction::Clear => {
                        BULK_DEFER_GEN.store(0, Ordering::SeqCst);
                        BULK_LINK.clear();
                        eprintln!("[conn] 受信が落ち着いたため bulk 経路を張り替えます(遅延していた切替)");
                    }
                    BulkDeferAction::Stale => {
                        BULK_DEFER_GEN.store(0, Ordering::SeqCst);
                    }
                    BulkDeferAction::Pending => {}
                }
            }
            // 書込み(タイムアウト5秒/台)の間はロックを保持しない。半開きの相手への
            // ping で3本のロックを掴み続けると、切替UI・新規接続の受け付け・
            // 各セッションの受信ループが待ち側まで止まる
            let candidates: Vec<(String, u64)> = {
                let _change = PEER_CHANGES.lock().unwrap_or_else(|e| e.into_inner());
                let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                let active = ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
                peers
                    .iter()
                    .enumerate()
                    .filter(|(i, p)| *i != *active && p.writer.is_some())
                    .map(|(_, p)| (p.id.clone(), p.gen))
                    .collect()
            };
            for (id, generation) in candidates {
                // writer を一時的に取り出して書く(書いている間は他の送信は None で skip)
                let mut taken = {
                    let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                    peers
                        .iter_mut()
                        .find(|p| p.id == id && p.gen == generation)
                        .and_then(|p| p.writer.take())
                };
                let Some(w) = taken.as_mut() else { continue };
                let wire = encode(&Msg::Ping { ts: now_ms() });
                let ok = w.write_all(wire.as_bytes()).and_then(|_| w.flush()).is_ok();
                // 戻し入れは id+gen が一致する時だけ(取り出し後に退場した端末は触らない)
                let mut save_sides = false;
                {
                    let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(p) = peers
                        .iter_mut()
                        .find(|p| p.id == id && p.gen == generation)
                    {
                        if ok {
                            p.writer = taken;
                        } else {
                            let mut active = ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
                            let departure = peers::detach(&mut peers, &mut active, &id, generation);
                            save_sides = peers::should_save_sides_after_detach(&peers, &departure);
                            eprintln!("[conn] 待機中の端末(id={id})が応答しないため一覧から外しました");
                        }
                    }
                }
                // 全端末がいなくなった時だけ配置(peer-sides)を整理する(セッション
                // 終了時と同じ条件。保存は PEERS ロックの外で行う)
                if save_sides {
                    crate::tap::save_peer_sides();
                    eprintln!("[conn] 全ての端末がいなくなったため配置設定を整理しました");
                }
            }
        }
    });
}

pub(crate) fn allow_bulk_peer(ip: std::net::IpAddr) -> bool {
    knit_common::net::is_allowed(ip) && *PEER_IP.lock().unwrap_or_else(|e|e.into_inner())==Some(ip)
}

/// 接続経路の短い表示(メニューバー用)。LAN 内なら "LAN 直"、100.x なら "Tailscale"。
/// ループバック(Android 中継経由)は画面の他箇所と同じ「タブレット」と呼ぶ
pub fn route_label() -> &'static str {
    match *PEER_IP.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(ip) if knit_common::net::is_tailscale(ip) => "Tailscale",
        // Android の中継は同じ Mac の中から繋ぐ(端末とは adb のワイヤレスデバッグで結ぶ)。
        // 経路名は開発ツール名ではなく、ユーザーが接続相手を読める呼び方で出す
        Some(ip) if ip.is_loopback() => "タブレット",
        Some(_) => "LAN 直",
        None => "",
    }
}

/// Tailscale の経路状態(0=不明/Tailscale 外, 1=直結, 2=中継(DERP))。
/// 中継は遅延が数倍になるため、切り替わった時に知らせる
pub static TS_PATH: AtomicU8 = AtomicU8::new(0);

/// 本線を確実に切る(スロットから外すだけでは受信スレッドが読み出しを待ち続ける)
pub(crate) fn drop_stream(reason: &str) {
    let taken = STREAM_SLOT
        .get()
        .and_then(|s| {
            let mut slot = s.lock().unwrap_or_else(|e| e.into_inner());
            OUTBOUND_GENERATION.fetch_add(1, Ordering::SeqCst);
            slot.take()
        });
    if let Some(s) = taken {
        eprintln!("[conn] {reason}。接続を張り直します");
        s.shutdown();
    }
}

/// `tailscale status --json` から相手への経路が直結か中継かを調べる
pub(crate) fn tailscale_path(peer: std::net::IpAddr) -> Option<u8> {
    let out = [
        "tailscale",
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
    ]
    .iter()
    .find_map(|bin| {
        std::process::Command::new(bin)
            .args(["status", "--json"])
            .output()
            .ok()
    })
    .filter(|o| o.status.success())?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let ip = peer.to_string();
    v["Peer"].as_object()?.values().find_map(|p| {
        let has = p["TailscaleIPs"]
            .as_array()?
            .iter()
            .any(|x| x.as_str() == Some(ip.as_str()));
        has.then(|| {
            if p["CurAddr"].as_str().unwrap_or("").is_empty() {
                2
            } else {
                1
            }
        })
    })
}
/// 大容量経路(ファイル・画像)。本線とは別の TCP 接続
pub(crate) static BULK_LINK: bulk::Link = bulk::Link::new();
pub(crate) static BULK: OnceLock<bulk::Endpoint> = OnceLock::new();
/// Windows 側のアプリ候補(AppsReply で受け取る)。越境 App Handoff(§12)の照合に使う
pub static WIN_APPS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
/// 越境 App Handoff(ビジョン§12 の第一歩・実験的): Windows へ切り替えたとき
/// Mac の最前面アプリと同じアプリを Windows で起動する。勝手にアプリが
/// 開く驚きを避けるため既定は無効(`KNIT_APP_HANDOFF=1` で有効)
pub(crate) static APP_HANDOFF: AtomicBool = AtomicBool::new(false);
/// 最終接続時刻(単調時計の ms)。メニュー表示と「長い断の後の再接続」通知に使う
static LAST_CONNECTED_MS: AtomicU64 = AtomicU64::new(0);
/// 断を検知した時刻(0=接続中)。長い断の後の再接続だけ通知する
pub(crate) static DISCONNECTED_SINCE_MS: AtomicU64 = AtomicU64::new(0);
/// 次の再接続試行の時刻(単調時計の ms・0=待ちなし)。クライアントモード
/// (Windows をホストにする)のバックオフ待機の前だけ立つ
pub(crate) static NEXT_RETRY_AT_MS: AtomicU64 = AtomicU64::new(0);
/// 最終接続の表示(相対時間)。未接続時の目安にする
pub fn last_connected_line() -> Option<String> {
    let ms = LAST_CONNECTED_MS.load(Ordering::Relaxed);
    if ms == 0 {
        return None;
    }
    let ago = now_ms().saturating_sub(ms) / 1000;
    let text = match ago {
        0..=4 => "たった今".to_string(),
        5..=59 => format!("{ago}秒前"),
        60..=3599 => format!("{}分前", ago / 60),
        _ => format!("{}時間前", ago / 3600),
    };
    Some(format!("最終接続 {text}"))
}

/// 次の再接続試行までの表示(未接続が続くときの見える化)。文言は common で
/// Windows 側と共用。サーバモード(既定)は再試行の待機が無いため常に None
pub fn next_retry_line() -> Option<String> {
    knit_common::diagnose::next_retry_line(
        NEXT_RETRY_AT_MS.load(Ordering::Relaxed),
        CONNECTED.load(Ordering::Relaxed),
        now_ms(),
    )
}

/// 相手を見つけられない状態の継続表示(1 分を超えた断だけ出す)。
/// Windows 側のステータス窓と同じ見える化(ログでしか分からない問題をメニューへ)
pub fn not_found_line() -> Option<String> {
    knit_common::diagnose::not_found_line(
        DISCONNECTED_SINCE_MS.load(Ordering::Relaxed),
        CONNECTED.load(Ordering::Relaxed),
        now_ms(),
    )
}

/// 接続が確立した時の共通処理(状態・時刻の記録と、長い断の後の再接続通知)
pub(crate) fn mark_connected() {
    CONNECTED.store(true, Ordering::Relaxed);
    NEXT_RETRY_AT_MS.store(0, Ordering::Relaxed);
    let now = now_ms();
    let since = DISCONNECTED_SINCE_MS.swap(0, Ordering::Relaxed);
    LAST_CONNECTED_MS.store(now, Ordering::Relaxed);
    // 接続安定性の実測(scripts/stability-report.sh の集計源)。since=0 は断なしの接続。
    // 時刻は壁時計(プロセス再起動を跨ぐ集計のため)。gap の内部比較は単調時計
    let gap = if since == 0 { 0 } else { now.saturating_sub(since) };
    eprintln!(
        "[conn-metric] connected unix_ms={} gap_ms={gap}",
        crate::state::wall_ms()
    );
    // 稼働計測(診断の安定性表示の源)。断=再接続が成功した回数のみ数える
    if gap > 0 {
        METRIC_DROPS.fetch_add(1, Ordering::Relaxed);
        METRIC_TOTAL_GAP_MS.fetch_add(gap, Ordering::Relaxed);
        METRIC_MAX_GAP_MS.fetch_max(gap, Ordering::Relaxed);
    }
    METRIC_CONNECTS.fetch_add(1, Ordering::Relaxed);
    if since != 0 && now.saturating_sub(since) > 60_000 {
        // 再接続した相手の実名(hello の name。未受信の間は汎用の「端末」)で知らせる
        let peer = PEER_NAME.lock().unwrap_or_else(|e| e.into_inner()).clone();
        notify(
            "Knit",
            &format!("{} と再接続しました", if peer.is_empty() { "端末".to_string() } else { peer }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> PeerEntry {
        PeerEntry {
            id: id.into(),
            name: "test".into(),
            ip: "127.0.0.1".parse().unwrap(),
            screen: (800.0, 600.0),
            monitors: vec![],
            writer: None,
            gen: 1,
            side: 0,
            edge_monitor: None,
            ver: 13,
            alias: None,
        }
    }

    /// PEERS/ACTIVE_PEER はプロセス共有の状態。入れ替えて必ず元へ戻す
    ///(ロック順は PEERS → ACTIVE_PEER。STREAM_SLOT は触らない)。
    /// 置換中の区間は crate の PEERS_TEST_SERIAL で直列化する
    ///(prefs::layout_values_tests も同じ PEERS を入れ替えるため共通で使う)
    fn with_peers(peers: Vec<PeerEntry>, active: usize, f: impl FnOnce()) {
        let _serial = crate::conn::PEERS_TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (saved_p, saved_a) = {
            let mut p = PEERS.lock().unwrap_or_else(|e| e.into_inner());
            let mut a = ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
            (std::mem::replace(&mut *p, peers), std::mem::replace(&mut *a, active))
        };
        f();
        let mut p = PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let mut a = ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        *p = saved_p;
        *a = saved_a;
    }

    /// activate_peer_locked の待ち判断: ping 送信中(writer 一時不在)だけ待ち、
    /// 一覧外は即諦め、writer が戻れば実行可能、アクティブ中の再選択は何もしない。
    /// Ready の判定には本物の Writer が要るためループバックで暗号化ペアを作る
    #[test]
    fn peer_switch_readiness_waits_only_for_a_missing_writer() {
        // STREAM_SLOT は main で初期化される。テストでは空のスロットを据える
        //(既に他のテストが据えていればそのまま使う)
        let _ = STREAM_SLOT.set(std::sync::Arc::new(std::sync::Mutex::new(None)));
        let slot_arc = STREAM_SLOT.get().unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let (_r, _w) = secure::accept(sock, "switch-test", b"knit-main").unwrap();
            std::thread::sleep(Duration::from_secs(30));
        });
        let socket = std::net::TcpStream::connect(addr).unwrap();
        let (_reader, writer) = secure::connect(socket, "switch-test", b"knit-main").unwrap();

        with_peers(vec![entry("idle")], usize::MAX, || {
            // ping 送信中: writer が一時的に取り出されている
            assert!(
                matches!(peer_switch_readiness(0), PeerSwitchReadiness::Wait),
                "writer 一時不在は待ち状態"
            );
            // 一覧外: 待たずに諦める(従来の無言 return と同じ扱い)
            assert!(
                matches!(peer_switch_readiness(5), PeerSwitchReadiness::Stop),
                "一覧外は即諦め"
            );
            // writer が戻れば実行可能
            PEERS.lock().unwrap_or_else(|e| e.into_inner())[0].writer = Some(writer);
            assert!(
                matches!(peer_switch_readiness(0), PeerSwitchReadiness::Ready),
                "writer があれば実行可能"
            );
            // アクティブな相手の再選択(writer はスロットへ貸し出し中): 何もしない
            let lent = PEERS
                .lock()
                .unwrap_or_else(|e| e.into_inner())[0]
                .writer
                .take()
                .unwrap();
            *slot_arc.lock().unwrap_or_else(|e| e.into_inner()) = Some(lent);
            *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner()) = 0;
            assert!(
                matches!(peer_switch_readiness(0), PeerSwitchReadiness::Stop),
                "アクティブ中の再選択は何もしない"
            );
            // 後始末: スロットを空へ戻す(状態を他のテストへ残さない)
            *slot_arc.lock().unwrap_or_else(|e| e.into_inner()) = None;
        });
    }

    /// 受信データの履歴ラベル: 接続中の端末はコンピュータ名(エイリアスが
    /// あれば優先)で載る。Android アプリはエイリアスが無い間は「タブレット」、
    /// 未接続・未知の id は汎用の "端末"
    #[test]
    fn history_device_labels_follow_peer_name_alias_and_fallbacks() {
        assert_eq!(history_device_for_peer("android-app-abc"), "タブレット");
        assert_eq!(history_device_for_peer("android-adb1"), "端末");
        assert_eq!(history_device_for_peer(""), "端末");
        // bulk 経路(ファイル)はアクティブな接続先で判別する。未接続は "端末"
        with_peers(vec![], usize::MAX, || {
            assert_eq!(history_device_for_active(), "端末");
        });
        // Android アプリ選択中: エイリアスが無ければ従来どおり「タブレット」
        with_peers(vec![entry("win-1"), entry("android-app-abc")], 1, || {
            assert_eq!(history_device_for_active(), "タブレット");
        });
        // Windows 選択中: コンピュータ名(name)で履歴へ載る
        with_peers(vec![entry("win-1"), entry("android-app-abc")], 0, || {
            assert_eq!(history_device_for_active(), "test");
            // 本線(clip)のラベルも送り主のピア id から同じ名前を引く
            assert_eq!(history_device_for_peer("win-1"), "test");
        });
        // エイリアスがあれば優先(利用者が付けた名前で複数台を区別できる)
        let mut aliased = entry("win-2");
        aliased.alias = Some("事務室のPC".into());
        with_peers(vec![aliased], 0, || {
            assert_eq!(history_device_for_active(), "事務室のPC");
            assert_eq!(history_device_for_peer("win-2"), "事務室のPC");
        });
    }

    /// 表示ラベル「名前 (IP)」: エイリアス優先・空のエイリアスは無い扱い。
    /// 同じコンピュータ名の複数台を IP で区別するための形式
    #[test]
    fn peer_display_label_prefers_alias_and_appends_ip() {
        assert_eq!(
            peer_display_label(None, "DESKTOP-A", "192.168.1.5"),
            "DESKTOP-A (192.168.1.5)"
        );
        assert_eq!(
            peer_display_label(Some("事務室"), "DESKTOP-A", "192.168.1.5"),
            "事務室 (192.168.1.5)"
        );
        // 空のエイリアスは「コンピュータ名へ戻す」扱いのため無いものと同じ
        assert_eq!(
            peer_display_label(Some(""), "DESKTOP-A", "10.0.0.2"),
            "DESKTOP-A (10.0.0.2)"
        );
    }

    /// アクティブな接続先の表示名もエイリアス優先(設定の接続ページ・通知で使う)。
    /// 未接続・name 未受信のフォールバックは汎用の "端末"
    #[test]
    fn active_peer_label_prefers_alias() {
        with_peers(vec![], usize::MAX, || {
            *PEER_NAME.lock().unwrap_or_else(|e| e.into_inner()) = String::new();
            assert_eq!(active_peer_label(), "端末");
        });
        let mut named = entry("win-1");
        named.name = "DESKTOP-X".into();
        with_peers(vec![named], 0, || {
            assert_eq!(active_peer_label(), "DESKTOP-X");
        });
        let mut aliased = entry("win-1");
        aliased.name = "DESKTOP-X".into();
        aliased.alias = Some("自席".into());
        with_peers(vec![aliased], 0, || {
            assert_eq!(active_peer_label(), "自席");
        });
    }

    /// bulk 受信バッチの履歴ラベルは「開始時点のアクティブ端末」で固定される。
    /// 通知時点で active を読むと、受信中の切替でラベルが替わる(指摘の回帰)
    #[test]
    fn bulk_rx_device_is_fixed_at_batch_begin() {
        // バッチ開始時点は Windows(win-1 / name=test)がアクティブ
        with_peers(vec![entry("win-1"), entry("android-app-abc")], 0, || {
            note_bulk_rx_device();
            assert_eq!(bulk_rx_device(), "test");
        });
        // 受信中にアクティブをタブレットへ切り替えても、開始時のラベルのまま
        with_peers(vec![entry("win-1"), entry("android-app-abc")], 1, || {
            assert_eq!(
                bulk_rx_device(),
                "test",
                "開始後の切替でラベルが変わってはいけない"
            );
            // 次のバッチは切り替え後の端末で始まる
            note_bulk_rx_device();
            assert_eq!(bulk_rx_device(), "タブレット");
        });
        // 未接続で始まったバッチのラベル(既定)
        with_peers(vec![], usize::MAX, || {
            note_bulk_rx_device();
            assert_eq!(bulk_rx_device(), "端末");
        });
    }

    /// bulk 張替延期の世代ガード: 予約した世代が現在も使われているときだけ
    /// clear を許す。切替が重なって世代が増えていれば古い予約は何も切らない
    ///(旧実装は期限後に新しい経路を後から切る競合があった)
    #[test]
    fn bulk_defer_clears_only_when_the_booked_generation_is_current() {
        // 予約なし(0)は何もしない
        assert_eq!(
            bulk_defer_clear_action(0, 5, 0, now_ms(), true),
            BulkDeferAction::Pending
        );
        // 世代一致・受信中・期限内: 待つ
        assert_eq!(
            bulk_defer_clear_action(5, 5, 10_060_000, 10_000_000, true),
            BulkDeferAction::Pending
        );
        // 世代一致・受信完了: clear(期限前でも受信が終われば切ってよい)
        assert_eq!(
            bulk_defer_clear_action(5, 5, 10_060_000, 10_000_000, false),
            BulkDeferAction::Clear
        );
        // 世代一致・受信中でも期限切れ: clear(無期限の遅延を許さない)
        assert_eq!(
            bulk_defer_clear_action(5, 5, 10_060_000, 10_060_000, true),
            BulkDeferAction::Clear
        );
        // 世代不一致(予約後に別の切替が起きた): 何も切らず予約を捨てる
        assert_eq!(
            bulk_defer_clear_action(5, 6, 10_060_000, 10_000_000, false),
            BulkDeferAction::Stale,
            "世代が置き換わった予約が clear してはいけない"
        );
        assert_eq!(
            bulk_defer_clear_action(5, 6, 10_060_000, 10_060_000, true),
            BulkDeferAction::Stale
        );
    }
}
