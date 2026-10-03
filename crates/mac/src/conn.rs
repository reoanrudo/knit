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

/// 相手から受信したもののクリップボード履歴ラベル。Android アプリからの
/// 受信は「タブレット」。それ以外(Windows 接続・未接続)は従来どおり
/// "Windows"。本線メッセージ(clip)は送り主のピア id で判別する
pub(crate) fn history_device_for_peer(peer_id: &str) -> &'static str {
    if peer_id.starts_with("android-app-") {
        "タブレット"
    } else {
        "Windows"
    }
}

/// 大容量経路(ファイル・画像)は選択中の端末から届くため、アクティブな
/// 接続先の id で判別する(ロック順は STREAM_SLOT → PEERS → ACTIVE_PEER)
pub(crate) fn history_device_for_active() -> &'static str {
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    history_device_for_peer(peers.get(act).map(|p| p.id.as_str()).unwrap_or(""))
}

/// アクティブな接続先の表示名(Windows 接続・未接続は "Windows")
pub(crate) fn active_peer_label() -> String {
    let peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let act = *ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    peers
        .get(act)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| {
            let name = PEER_NAME.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if name.is_empty() { "Windows".into() } else { name }
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

fn activate_peer_locked(new: usize, reason: &str) {
    let slot_arc = STREAM_SLOT.get().unwrap();
    let mut slot = slot_arc.lock().unwrap_or_else(|e| e.into_inner());
    let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    let mut act = ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
    if new >= peers.len() || (*act == new && slot.is_some()) || peers[new].writer.is_none() {
        return;
    }
    OUTBOUND_GENERATION.fetch_add(1, Ordering::SeqCst);
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
        peers[new].name.clone(),
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
    // 旧相手のファイル転送経路を切る(接続側は本線の接続先へ追従して張り直す)
    BULK_LINK.clear();
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
                let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(p) = peers
                    .iter_mut()
                    .find(|p| p.id == id && p.gen == generation)
                {
                    if ok {
                        p.writer = taken;
                    } else {
                        let mut active = ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
                        peers::detach(&mut peers, &mut active, &id, generation);
                        eprintln!("[conn] 待機中の端末(id={id})が応答しないため一覧から外しました");
                    }
                }
            }
        }
    });
}

pub(crate) fn allow_bulk_peer(ip: std::net::IpAddr) -> bool {
    knit_common::net::is_allowed(ip) && *PEER_IP.lock().unwrap_or_else(|e|e.into_inner())==Some(ip)
}

/// 接続経路の短い表示(メニューバー用)。LAN 内なら "LAN 直"、100.x なら "Tailscale"
pub fn route_label() -> &'static str {
    match *PEER_IP.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(ip) if knit_common::net::is_tailscale(ip) => "Tailscale",
        // Android の中継は同じ Mac の中から繋ぐ(端末とは adb のワイヤレスデバッグで結ぶ)
        Some(ip) if ip.is_loopback() => "adb",
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
/// 最終接続時刻(unix ms)。メニュー表示と「長い断の後の再接続」通知に使う
static LAST_CONNECTED_MS: AtomicU64 = AtomicU64::new(0);
/// 断を検知した時刻(0=接続中)。長い断の後の再接続だけ通知する
pub(crate) static DISCONNECTED_SINCE_MS: AtomicU64 = AtomicU64::new(0);
/// 次の再接続試行の時刻(unix ms・0=待ちなし)。クライアントモード
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
        0..=4 => "今しがた".to_string(),
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
        notify("Knit", "Windows と再接続しました");
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
        }
    }

    /// PEERS/ACTIVE_PEER はプロセス共有の状態。入れ替えて必ず元へ戻す
    ///(ロック順は PEERS → ACTIVE_PEER。STREAM_SLOT は触らない)
    fn with_peers(peers: Vec<PeerEntry>, active: usize, f: impl FnOnce()) {
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

    /// 受信データの履歴ラベル: Android アプリ(id が android-app-*)なら
    /// 「タブレット」、Windows 接続・未接続は従来どおり "Windows"
    #[test]
    fn history_device_labels_android_app_and_windows() {
        assert_eq!(history_device_for_peer("android-app-abc"), "タブレット");
        assert_eq!(history_device_for_peer("android-adb1"), "Windows");
        assert_eq!(history_device_for_peer(""), "Windows");
        // bulk 経路(ファイル)はアクティブな接続先で判別する。未接続は "Windows"
        with_peers(vec![], usize::MAX, || {
            assert_eq!(history_device_for_active(), "Windows");
        });
        with_peers(vec![entry("win-1"), entry("android-app-abc")], 1, || {
            assert_eq!(history_device_for_active(), "タブレット");
        });
        with_peers(vec![entry("win-1"), entry("android-app-abc")], 0, || {
            assert_eq!(history_device_for_active(), "Windows");
        });
    }
}
