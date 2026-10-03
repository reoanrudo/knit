use crate::*;
use knit_common::proto::{compatible, decode, safe_peer_name, VERSION};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};

// ---------- 接続セッション(サーバ/クライアント両モード共通) ----------

/// 1行の長さ上限(画像base64 5MB 上限に対し余裕を持たせる。
/// 未認証helloを含む巨大行によるメモリ消費(DoS)対策)
const MAX_LINE: u64 = 8 * 1024 * 1024;

/// RTT 悪化の継続監視状態: (80ms 超が始まった時刻[単調 ms]、0=未発生,
/// 最後に案内通知を出した時刻)
static RTT_DEGRADE: std::sync::Mutex<(u64, u64)> = std::sync::Mutex::new((0, 0));

/// RTT 悪化の継続判定( pong 受信ごとに呼ぶ純関数。単体テストで境界を守る)。
/// 80ms 超が 10 秒以上続き、前回の案内から 10 分以上空いていれば案内する。
/// 戻り値は (案内するか, 更新後の状態)。rtt が閾値未満なら継続をリセットする
pub(crate) fn rtt_degraded_check(state: (u64, u64), rtt: u64, now: u64) -> (bool, (u64, u64)) {
    const THRESH_MS: u64 = 80;
    const SUSTAIN_MS: u64 = 10_000;
    const THROTTLE_MS: u64 = 600_000;
    let (over_since, last_notify) = state;
    if rtt < THRESH_MS {
        return (false, (0, last_notify));
    }
    // 初回の超過で始まった時刻を覚え、以降はそれを維持する
    let since = if over_since == 0 { now } else { over_since };
    // last_notify=0 は「一度も案内していない」なのでスロットルには掛からない
    let throttle_ok = last_notify == 0 || now.saturating_sub(last_notify) >= THROTTLE_MS;
    if now.saturating_sub(since) >= SUSTAIN_MS && throttle_ok {
        return (true, (since, now));
    }
    (false, (since, last_notify))
}

/// 認証済み接続の受信ループ(Return / Pong / Clip / Bye)。
/// 戻り値は true=相手の Bye による正常終了、false=切断・読み取りエラー
pub(crate) fn session_receive_loop(reader: &mut std::io::BufReader<secure::Reader>, my_id: &str, generation: u64) -> bool {
    use std::io::BufRead;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            // 切断理由(EOF 以外)を残す: pong 途絶の張替えか read エラーかの区別が
            // 「なぜ切れたか」の追跡に必要。不正行(decode 失敗)は既存どおり無視
            Err(e) => {
                eprintln!("[conn] read error: {e}(kind={:?})", e.kind());
                break;
            }
            Ok(_) if line.len() as u64 > MAX_LINE => {
                eprintln!("[conn] line too large. dropping connection");
                break;
            }
            Ok(_) => {
                if let Some(msg) = decode(&line) {
                    if !my_id.is_empty() && !PEERS.lock().unwrap_or_else(|e|e.into_inner()).iter().any(|p|p.id==my_id && p.gen==generation) {
                        continue;
                    }
                    if !is_active_peer(my_id, generation) && !matches!(msg, Msg::Bye | Msg::TabletInfo { .. } | Msg::Screen { .. } | Msg::Ping { .. }) {
                        continue;
                    }
                    match msg {
                        // 返信はアクティブな相手へ出るため、それ以外の相手からは受けない
                        // (相手側は押下の解放・時間切れで自ら取り消す)
                        Msg::DragOffer {
                            id,
                            count,
                            total,
                            position,
                        } if is_active_peer(my_id, generation) => {
                            incoming_drag::offer(id, count, total, position)
                        }
                        Msg::DragCommit { id } => incoming_drag::commit(id),
                        // 相手が自発的に止めた場合と、こちらの送信中ドラッグの中止要求
                        Msg::DragCancel { id } => {
                            incoming_drag::cancel(id);
                            // 中止要求は自発信の転送(Mac 生成 id=最上位ビット1)にだけ掛ける。
                            // 相手発信の id を request すると、消費する送信スレッドが
                            // 存在せず中止要求が一覧に溜まり続ける
                            if id >> 63 == 1 {
                                knit_common::xfer::request(id);
                            }
                        }
                        Msg::AppsReply { apps } => {
                            // 越境 App Handoff の照合用: Windows 側のアプリ候補を保持する
                            *WIN_APPS.lock().unwrap_or_else(|e| e.into_inner()) = apps.clone();
                            eprintln!("[handoff] Windows アプリ {} 件を受信", apps.len());
                        }
                        Msg::Role { host } => {
                            // 適用済みを相手へ知らせてから再起動へ移る。
                            // 相手はこの確認を待って再起動する(版 15 以降。
                            // 旧側は無視するため、旧側相手では従来どおり時間頼み)
                            let _ = send_msg_reported(&Msg::RoleAck);
                            std::thread::spawn(move || gui::apply_peer_role(host));
                        }
                        Msg::RoleAck => {
                            // 相手の役割切替適用が済んだ。再起動を待っていた
                            // GUI へ知らせる
                            gui::note_role_ack();
                        }
                        Msg::XferAck {
                            accepted,
                            rejected,
                            scope,
                        } => {
                            // 送信したファイルの受理結果(版 15 以降の相手が返す)。
                            // 送信スレッドの通知分岐に使う
                            note_xfer_ack(accepted, rejected, scope);
                        }
                        Msg::PrefsGet => {
                            send_msg(&Msg::Prefs { json: gui::prefs_snapshot_json() });
                        }
                        Msg::PrefsSet { json } => {
                            gui::apply_remote_prefs(&json);
                            send_cfg();
                            send_msg(&Msg::Prefs { json: gui::prefs_snapshot_json() });
                        }
                        Msg::Return { ny } => {
                            if HOTKEY_ONLY.load(Ordering::Relaxed) {
                                // hotkey モードでは Windows 側の左端到達を無視し、
                                // F13 で戻すまで Windows のまま(ロック状態)
                                continue;
                            }
                            // abs-left 復帰後に遅延到着した Return で二重に leave
                            // され再ワープされるのを防ぐ(既に Mac の場合は無視)
                            if !WIN_MODE.load(Ordering::Relaxed) {
                                continue;
                            }
                            WIN_MODE.store(false, Ordering::Relaxed);
                            eprintln!("[mode] MAC (return)");
                            leave_win_mode_cursor_unlock(Some(ny));
                        }
                        Msg::Clip { text } => {
                            if !CLIP_SHARE.load(Ordering::Relaxed) || !knit_common::share::allow_clip() {
                                continue;
                            }
                            if text.len() > CLIP_MAX_BYTES {
                                // 巨大コピーの連投で通知が洪水にならないよう 60 秒に間引く
                                static BIG_CLIP_NOTIFY_MS: AtomicU64 = AtomicU64::new(0);
                                let now = now_ms();
                                if now
                                    .saturating_sub(BIG_CLIP_NOTIFY_MS.swap(now, Ordering::Relaxed))
                                    >= 60_000
                                {
                                    notify(
                                        "Knit",
                                        &knit_common::history::too_large_clip_message(&text),
                                    );
                                }
                                continue;
                            }
                            {
                                // Windows の CRLF は Mac 向けに LF へ正規化
                                let text = if text.contains("\r\n") {
                                    text.replace("\r\n", "\n")
                                } else {
                                    text
                                };
                                *LAST_RECV_CLIP.lock().unwrap_or_else(|e| e.into_inner()) =
                                    Some(text.clone());
                                with_pool(|| unsafe { mac_set_clipboard(&text) });
                                LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);
                                history_push(&text, history_device_for_peer(my_id));
                                eprintln!("[clip] win->mac {} bytes", text.len());
                            }
                        }
                        Msg::Pong { ts } => {
                            // アクティブな相手の pong だけ生存時刻・RTT に反映する
                            // (非アクティブ peers への keepalive 応答で上書めないように)
                            if is_active_peer(my_id, generation) {
                                let now = now_ms();
                                LAST_PONG_MS.store(now, Ordering::Relaxed);
                                // ping/pong の往復時間を RTT として保持し、Windows 側の
                                // ステータス窓表示にも回す(接続品質の見える化)
                                let rtt = now.saturating_sub(ts).min(60_000);
                                RTT_MS.store(rtt, Ordering::Relaxed);
                                if rtt >= 80 {
                                    eprintln!("[rtt] {rtt}ms(相手 {my_id}。カクつきが Wi-Fi の遅れによるかの確認用)");
                                }
                                // RTT 悪化が続いたら一度だけ有線直結を案内する
                                // ([rtt] ログだけでは気づけないため。 pong は毎秒来るが
                                // スロットルで 10 分に 1 回まで)
                                let mut degrade = RTT_DEGRADE.lock().unwrap_or_else(|e| e.into_inner());
                                let (guide, next) = rtt_degraded_check(*degrade, rtt, now);
                                *degrade = next;
                                drop(degrade);
                                if guide {
                                    notify(
                                        "Knit",
                                        "遅延が続いています(80ms超)。Wi-Fi の場合は有線直結を検討してください。経路は設定「接続」で確認できます",
                                    );
                                }
                                send_msg(&Msg::Stat { rtt });
                            }
                        }
                        Msg::Ping { ts } => {
                            // アクティブなら既定の送信経路。非アクティブな相手からの
                            // keepalive には、そのセッション自身の writer へ直接返す
                            if is_active_peer(my_id, generation) {
                                send_msg(&Msg::Pong { ts });
                            } else {
                                use std::io::Write as _;
                                // writer を取り出してからロックを解放し、
                                // タイムアウト付き書込みの間に PEERS を掴まない
                                let mut taken = {
                                    let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                                    peers
                                        .iter_mut()
                                        .find(|p| p.id == my_id && p.gen == generation)
                                        .and_then(|p| p.writer.take())
                                };
                                if let Some(w) = taken.as_mut() {
                                    let wire = encode(&Msg::Pong { ts });
                                    let ok = w.write_all(wire.as_bytes())
                                        .and_then(|_| w.flush())
                                        .is_ok();
                                    let mut peers =
                                        PEERS.lock().unwrap_or_else(|e| e.into_inner());
                                    if let Some(p) = peers
                                        .iter_mut()
                                        .find(|p| p.id == my_id && p.gen == generation)
                                    {
                                        if ok {
                                            p.writer = taken;
                                        }
                                    }
                                }
                            }
                        }
                        Msg::Rel { on } => {
                            if GAME_REL.swap(on, Ordering::Relaxed) != on {
                                eprintln!(
                                    "[game] ゲームモード -> {}",
                                    if on {
                                        "ON(相対移動)"
                                    } else {
                                        "OFF(絶対位置)"
                                    }
                                );
                            }
                        }
                        Msg::TabletInfo { width_mm, height_mm, control, keyboard, japanese }
                            if my_id.starts_with("android-app-") => {
                            if [width_mm,height_mm].iter().all(|n| n.is_finite() && (30.0..=1500.0).contains(n)) {
                                android::display::remember(my_id, Some(android::display::PhysicalSize {width_mm,height_mm}));
                            }
                            android::app::remember(my_id, control, keyboard, japanese);
                            // 「接続したら操作対象に自動で切り替える」: タブレットの Knit アプリが
                            // 明示的に接続してきた=操作してほしい合図として扱う(製品方針の
                            // 「黙って操作しない」は「近くにいるだけで操作」を禁じる趣旨で、
                            // アプリの「接続を開始」は明示的な同意)。Windows 操作中は奪わない
                            if !is_active_peer(my_id, generation) && !WIN_MODE.load(Ordering::Relaxed) {
                                activate_peer_by_id(my_id, "タブレット接続");
                            }
                            if !control && is_active_peer(my_id, generation) && WIN_MODE.load(Ordering::Relaxed) {
                                leave_win_mode_cursor_unlock(None);
                            }
                        }
                        Msg::Screen { w, h } if (1..=32768).contains(&w) && (1..=32768).contains(&h) => {
                            {
                                let mut peers = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                                if let Some(peer) = peers.iter_mut().find(|p| p.id==my_id && p.gen==generation) {
                                    peer.screen=(w as f64,h as f64);
                                    if peer.id.starts_with("android-app-") { peer.monitors=vec![knit_common::proto::Monitor {x:0,y:0,w,h,name:String::new()}]; }
                                }
                            }
                            if !is_active_peer(my_id,generation) { continue; }
                            // WIN_SCREEN と WIN_CUR はこの順で保持する(他箇所は
                            // 同時保持しないため順序固定でデッドロックなし)
                            let mut wc = WIN_CUR.lock().unwrap_or_else(|e| e.into_inner());
                            let mut ws = WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner());
                            let old = *ws;
                            *ws = (w as f64, h as f64);
                            *wc = rescale_win_cur(*wc, old, *ws);
                            eprintln!("[info] win screen changed {w}x{h}");
                        }
                        Msg::Lock => {
                            // Windows 側でロックされた(Win+L 等)。既定で Mac も連動して
                            // ロックする(KNIT_LOCK_SYNC=0 で無効)
                            if crate::LOCK_SYNC.load(Ordering::Relaxed) {
                                eprintln!("[lock] Windows がロックされたため Mac もロックします");
                                crate::lock_this_mac();
                            }
                        }
                        Msg::Bye => return true,
                        _ => {}
                    }
                }
            }
        }
    }
    // Bye 以外(EOF・読み取りエラー・行長超過)での終了
    false
}

/// セッション終了の共通後処理(スロット解除・切断通知・WIN 中なら正規 leave)。
/// auto_fallback=true は待機中の別端末へ自動で切り替わる見込みがあるため、
/// 切断通知を出さない(直後の切替通知と二重になる)
pub(crate) fn on_disconnect(auto_fallback: bool) {
    incoming_drag::reset();
    {
        let mut guard = STREAM_SLOT
            .get()
            .unwrap()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = None;
        OUTBOUND_GENERATION.fetch_add(1, Ordering::SeqCst);
    }
    CONNECTED.store(false, Ordering::Relaxed);
    DISCONNECTED_SINCE_MS
        .compare_exchange(0, now_ms(), Ordering::Relaxed, Ordering::Relaxed)
        .ok();
    // 旧セッションの RTT が再接続直後に「前の接続の値」として表示されるのを防ぐ
    RTT_MS.store(0, Ordering::Relaxed);
    TS_PATH.store(0, Ordering::Relaxed);
    *PEER_NAME.lock().unwrap_or_else(|e| e.into_inner()) = String::new();
    *PEER_IP.lock().unwrap_or_else(|e| e.into_inner()) = None;
    // 旧接続先のアプリ列挙キャッシュを消す: 空でない限り次の接続先の列挙を
    // 取得しに行かない構造のため、残すと別の端末へ再接続した後に handoff の
    // 照合が永久に旧端末のパスで行われる
    WIN_APPS.lock().unwrap_or_else(|e| e.into_inner()).clear();
    // 接続スコープの状態を戻す: 相手の版(次に繋がる端末は別の版もあり得る)と
    // ゲームモード相対移動(次の相手は全画面ゲーム中とは限らない)
    PEER_VERSION.store(0, Ordering::Relaxed);
    if GAME_REL.swap(false, Ordering::Relaxed) {
        eprintln!("[game] 切断のため相対移動を解除しました");
    }
    // WIN モード中の切断は正規の leave 経由で復帰させる(カーソル表示・
    // EDGE_GUARD・CUR_POS 整合を自己修復スレッドの「たまたま」に任せない)。
    // BULK_LINK.clear() より先に行う: 転送中に bulk の書き込みが詰まっていると
    // clear が長く止まり得るため、ユーザー操作の復帰と切断通知を後回しにしない
    if WIN_MODE.swap(false, Ordering::Relaxed) {
        eprintln!("[return] -> mac (disconnect)");
        leave_win_mode_cursor_unlock(None);
    }
    eprintln!("[conn] lost. waiting for reconnect...");
    eprintln!("[conn-metric] lost unix_ms={}", crate::state::wall_ms());
    // ネットワークのフラップで通知が連打されるのを防ぐ(再接続通知と同じ
    // 60 秒の間引き)。自動切替が決まっているときは切替通知だけを出す
    static LAST_DISCONNECT_NOTIFY_MS: AtomicU64 = AtomicU64::new(0);
    if !auto_fallback {
        let now = now_ms();
        if now.saturating_sub(LAST_DISCONNECT_NOTIFY_MS.load(Ordering::Relaxed)) >= 60_000 {
            LAST_DISCONNECT_NOTIFY_MS.store(now, Ordering::Relaxed);
            notify("Knit", "切断しました(自動で再接続します)");
        }
    }
    BULK_LINK.clear();
}

/// 待受モード(既定): Windows からの接続を受け入れる
/// (本環境では Mac 発コネクションが不通なため、Windows 発に限定した設計)
/// 進行中のハンドシェイク数(黙る相手の接続で待ちが積み上がらないよう上限を設ける)
static HANDSHAKE_PENDING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// ハンドシェイク+hello 検証までを1接続ぶんだけ行う。accept ループは別スレッド+
/// 絶対期限(15秒)でこの関数を待つため、接続だけして黙る相手が待ち受けを塞がない
#[allow(clippy::type_complexity)]
fn handshake_and_hello(
    stream: TcpStream,
    token: &str,
    peer: std::net::SocketAddr,
) -> Result<
    (
        std::io::BufReader<secure::Reader>,
        secure::Writer,
        String,
        String,
        Vec<knit_common::proto::Monitor>,
        f64,
        f64,
        u32,
    ),
    String,
> {
    use std::io::{BufRead, Read};
    stream.set_nodelay(true).ok();
    stream.set_read_timeout(Some(Duration::from_secs(12))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let (r, hw) = secure::accept(stream, token, b"knit-main")
        .map_err(|e| format!("暗号化ハンドシェイク失敗 ({peer}): {e}(トークン不一致の可能性)"))?;
    let mut reader = std::io::BufReader::new(r);
    let mut line = String::new();
    match (&mut reader).take(MAX_LINE + 1).read_line(&mut line) {
        Ok(0) | Err(_) => return Err("closed before hello".into()),
        Ok(_) if line.len() as u64 > MAX_LINE => return Err("hello too large. dropped".into()),
        Ok(_) => {}
    }
    match decode(&line) {
        Some(Msg::Hello {
            ver,
            w,
            h,
            name,
            id,
            monitors,
            ..
        }) if compatible(ver) => {
            // ここではグローバルの PEER_VERSION を書かない: 待機中の端末を含む
            // 全ハンドシェイクで上書きすると、後から繋がった旧端末がアクティブ
            // 端末の機能判定を壊す。版は PeerEntry へ格納し、アクティブ化時に
            // PEER_VERSION へ反映する
            // 表示名は制御文字・Bidi オーバーライドを除去してから載せる
            let disp = {
                let n = safe_peer_name(name.trim());
                if n.is_empty() {
                    "Windows".to_string()
                } else {
                    n
                }
            };
            // 端末 id は再接続の紐付けに使う。旧版(版 12 以前)は空=IP 由来の代替
            let dev = if id.is_empty() {
                format!("legacy-{}", peer.ip())
            } else {
                id
            };
            Ok((reader, hw, disp, dev, monitors, w.max(1) as f64, h.max(1) as f64, ver))
        }
        _ => Err("invalid hello".into()),
    }
}

pub(crate) fn server_thread(port: u16, token: String, screen_w: f64, screen_h: f64) {
    // 待受アドレス: 既定は全インターフェース(LAN 直を受け入れる)。
    // 防御は is_allowed(接続元絞り)+ Noise ハンドシェイクが担う
    let bind_ip = envutil::get("KNIT_BIND").unwrap_or_else(|| "0.0.0.0".to_string());
    let listener = match std::net::TcpListener::bind((bind_ip.as_str(), port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[fatal] listen {bind_ip}:{port} failed: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("[info] server mode. listening on {bind_ip}:{port}");
    let mut accept_errs: u32 = 0;
    // 認証に失敗し続ける接続の連打を鈍らせる(正規の接続が成功すれば即回復)
    let mut throttle = secure::FailThrottle::new();
    // 非アクティブピアへの生存確認(複数台保持のために一度だけ起こす)
    keepalive_inactive_peers();
    loop {
        let (stream, peer) = match listener.accept() {
            Ok(x) => {
                accept_errs = 0;
                knit_common::net::tune_tcp(&x.0);
                x
            }
            Err(e) => {
                // fd 枯渇等で失敗が続くと 500ms 毎の洪水になるため、最初と
                // その後 20 回毎(≒10 秒)だけ出す
                accept_errs += 1;
                if accept_errs == 1 || accept_errs.is_multiple_of(20) {
                    eprintln!("[conn] accept error: {e}(連続 {accept_errs} 回目)");
                }
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
        };
        eprintln!("[conn] accepted from {peer}");
        // 接続元の制限(既定は LAN・有線直結のみ。Tailscale は KNIT_ALLOW_TS=1、
        // インターネット側は KNIT_ALLOW_ANY=1 で許可)。認証は暗号化ハンドシェイクで行う
        if !knit_common::net::is_allowed(peer.ip()) {
            eprintln!(
                "[conn] rejected: {peer} は許可範囲外です(Tailscale は KNIT_ALLOW_TS=1、他ネットワークは KNIT_ALLOW_ANY=1 で許可)"
            );
            continue;
        }
        // ハンドシェイク+hello に絶対期限を切る(接続だけして黙る相手が待ち受けを塞ぐと、
        // 正規の再接続まで受け付けられなくなるため)。同時に待つハンドシェイクも少量に絞る
        if HANDSHAKE_PENDING.load(Ordering::Relaxed) >= 4 {
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        HANDSHAKE_PENDING.fetch_add(1, Ordering::Relaxed);
        let (hs_tx, hs_rx) = std::sync::mpsc::channel();
        {
            let token = token.clone();
            let peer2 = peer;
            std::thread::spawn(move || {
                let out = handshake_and_hello(stream, &token, peer2);
                HANDSHAKE_PENDING.fetch_sub(1, Ordering::Relaxed);
                let _ = hs_tx.send(out);
            });
        }
        let (mut reader, mut w, disp, dev, mons, win_w, win_h, peer_ver) = match hs_rx
            .recv_timeout(Duration::from_secs(15))
        {
            Ok(Ok(x)) => x,
            Ok(Err(e)) => {
                eprintln!("[conn] {e}");
                std::thread::sleep(throttle.fail());
                continue;
            }
            Err(_) => {
                eprintln!("[conn] ハンドシェイクが15秒で完了しないため切断({peer})");
                std::thread::sleep(throttle.fail());
                continue;
            }
        };
        throttle.success();
        eprintln!(
            "[conn] established: {disp} (id={dev}) 画面 {}x{} モニター: {}",
            win_w as i32,
            win_h as i32,
            knit_common::proto::Monitor::summary(&mons)
        );
        // セッションはスレッドへ分離し、accept 側は次の接続を待つ(複数台の同時保持)
        std::thread::spawn(move || {
            use std::io::Write as _;
            // hello_ok はこのセッションの writer へ直接返す(アクティブ化前でも届くように)
            let ok = encode(&Msg::HelloOk {
                name: hostname_label(),
                w: screen_w as i32,
                h: screen_h as i32,
                ver: VERSION,
                id: knit_common::proto::device_id(),
                monitors: mac_monitors(),
            });
            if w.write_all(ok.as_bytes()).and_then(|_| w.flush()).is_err() {
                eprintln!("[conn] hello_ok 送信に失敗しました");
                return;
            }
            let my_gen = PEER_GEN.fetch_add(1, Ordering::Relaxed) + 1;
            {
                let _change = PEER_CHANGES.lock().unwrap_or_else(|e| e.into_inner());
                let mut slot = STREAM_SLOT.get().unwrap().lock().unwrap_or_else(|e| e.into_inner());
                let mut entries = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                let mut active = ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
                let previous = *active;
                let first = previous >= entries.len();
                let replacing = entries.get(previous).is_some_and(|p| p.id == dev);
                let taken = entries.iter().map(|p| p.side).collect::<Vec<_>>();
                let (side, edge_monitor) = assign_side_for_new_peer(&dev, &taken);
                let select = peers::insert(&mut entries, previous, PeerEntry {
                    id: dev.clone(), name: disp.clone(), ip: peer.ip(),
                    screen: (win_w, win_h), monitors: mons, writer: Some(w),
                    gen: my_gen, side, edge_monitor, ver: peer_ver,
                });
                if replacing {
                    if let Some(old) = slot.take() { old.shutdown(); }
                    *active = usize::MAX;
                }
                drop(active);
                drop(entries);
                drop(slot);
                if select {
                    activate_peer_id_locked(&dev, if first { "初回接続" } else { "再接続" });
                }
            }
            let _ = session_receive_loop(&mut reader, &dev, my_gen);
            let _change = PEER_CHANGES.lock().unwrap_or_else(|e| e.into_inner());
            let departure = {
                let mut slot = STREAM_SLOT.get().unwrap().lock().unwrap_or_else(|e| e.into_inner());
                let mut entries = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                let mut active = ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
                let departure = peers::detach(&mut entries, &mut active, &dev, my_gen);
                if matches!(departure, peers::Departure::Active { .. }) {
                    if let Some(old) = slot.take() { old.shutdown(); }
                    CONNECTED.store(false, Ordering::Relaxed);
                }
                departure
            };
            if let peers::Departure::Active { next } = departure {
                // 待機端末へ自動で切り替わる場合は切断通知を出さない(二重になる)
                on_disconnect(next.is_some());
                if let Some(id) = next { activate_peer_id_locked(&id, "自動切替"); }
            }

        });
    }
}

/// 接続モードの 1 セッション分(ハンドシェイク+本体)。Result はリトライ理由。
/// 確立後の切断は Err で返す: 確立したのにすぐ切れる相手への高頻度再試行を
/// 抑えるため、呼び出し側のバックオフはリセットしない(Windows 側と同じ条件)
pub(crate) fn client_attempt(s: TcpStream, token: &str, screen_w: f64, screen_h: f64) -> Result<(), String> {
    use std::io::{BufRead, Read, Write};
    s.set_read_timeout(Some(Duration::from_secs(12))).ok();
    eprintln!("[conn] connected");
    s.set_nodelay(true).ok();
    s.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let (r, mut hw) = secure::connect(s, token, b"knit-main")
        .map_err(|e| format!("暗号化ハンドシェイク失敗: {e}(トークン不一致の可能性)"))?;
    // hello(自画面サイズを相手へ伝える。相手は hello_ok で自画面を返す)
    let hello = encode(&Msg::Hello {
        ver: VERSION,
        name: hostname_label(),
        token: String::new(),
        w: screen_w as i32,
        h: screen_h as i32,
        id: knit_common::proto::device_id(),
        monitors: mac_monitors(),
    });
    hw.write_all(hello.as_bytes())
        .and_then(|_| hw.flush())
        .map_err(|_| "hello send failed".to_string())?;
    // hello_ok を待つ(行長制限付き)
    let mut reader = std::io::BufReader::new(r);
    let mut line = String::new();
    (&mut reader)
        .take(MAX_LINE + 1)
        .read_line(&mut line)
        .map_err(|_| "hello_ok read failed".to_string())?;
    if line.len() as u64 > MAX_LINE {
        return Err("hello_ok too large".into());
    }
    match decode(&line) {
        Some(Msg::HelloOk {
            w: mw,
            h: mh,
            name,
            ver,
            monitors,
            ..
        }) if mw > 0 && mh > 0 && compatible(ver) => {
            PEER_VERSION.store(ver, Ordering::Relaxed);
            *WIN_SCREEN.lock().unwrap_or_else(|e| e.into_inner()) = (mw as f64, mh as f64);
            *PEER_NAME.lock().unwrap_or_else(|e| e.into_inner()) = {
                let n = safe_peer_name(name.trim());
                if n.is_empty() {
                    "Windows".into()
                } else {
                    n
                }
            };
            eprintln!(
                "[info] win screen {mw}x{mh} モニター: {}",
                knit_common::proto::Monitor::summary(&monitors)
            );
        }
        _ => return Err("invalid hello_ok".into()),
    }
    LAST_PONG_MS.store(now_ms(), Ordering::Relaxed);
    {
        let mut guard = STREAM_SLOT
            .get()
            .unwrap()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *guard = Some(hw);
        OUTBOUND_GENERATION.fetch_add(1, Ordering::SeqCst);
    }
    mark_connected();
    LAST_PONG_MS.store(now_ms(), Ordering::Relaxed);
    // 現在の ⌘キー設定を同期(クライアントモードの確立時)
    send_cfg();
    eprintln!("[conn] established");
    let peer = PEER_NAME.lock().map(|n| n.clone()).unwrap_or_default();
    notify(
        "Knit",
        &format!(
            "{} と接続しました",
            if peer.is_empty() {
                "Windows".into()
            } else {
                peer
            }
        ),
    );
    // クライアントモードは単一接続のため my_id 空(=常にアクティブ扱い)
    let bye = session_receive_loop(&mut reader, "", 0);
    on_disconnect(false);
    if bye {
        // 相手の Bye による正常終了(役割切替の再起動等)。Ok として返し、
        // 呼び出し側のバックオフをリセットさせる(異常切断だけ待ち時間を進める)
        return Ok(());
    }
    // ここに来た=確立後に切れた(確立前の失敗はすべて上で return している)。
    // Err として返し、呼び出し側のバックオフを進ませたままにする
    Err("connection lost after established".into())
}

/// 接続モード(KNIT_ROLE=client): Windows(サーバ)へ接続し続ける。
/// 待機は指数バックオフ+フルジッター。スリープ復帰・ネットワーク変化の
/// WAKE 通知で待機を飛ばしてすぐ再試行する
pub(crate) fn client_thread(host: Option<String>, port: u16, token: String, screen_w: f64, screen_h: f64) {
    let mut backoff = knit_common::retry::Backoff::new(
        Duration::from_millis(500),
        Duration::from_secs(10),
    );
    loop {
        let addrs = knit_common::connect::resolve(host.as_deref(), port, &token);
        let delay = if addrs.is_empty() {
            // 候補が空なら first_reachable を呼ばない(Windows 側と同じ: 3.5 秒の空待ち防止)
            eprintln!("[conn] 接続先が見つかりません");
            backoff.next_delay()
        } else {
            let t0 = std::time::Instant::now();
            match knit_common::connect::first_reachable(&addrs, Duration::from_secs(3)) {
                Some((s, a)) => {
                    eprintln!("[conn] connected ({a}) in {}ms", t0.elapsed().as_millis());
                    *PEER_IP.lock().unwrap_or_else(|e| e.into_inner()) = Some(a.ip());
                    match client_attempt(s, &token, screen_w, screen_h) {
                        Ok(()) => backoff.reset(),
                        Err(e) => eprintln!("[conn] {e}"),
                    }
                }
                None => eprintln!("[conn] どの接続先にも繋がりません: {:?}", host),
            }
            backoff.next_delay()
        };
        // 次の試行時刻を状態表示へ(未接続が続くときの「あと何秒」の見える化。
        // WAKE で待機を飛ばした場合は実際より長く出るが、過剰表示で害は無い)
        NEXT_RETRY_AT_MS.store(
            now_ms() + delay.as_millis() as u64,
            Ordering::Relaxed,
        );
        WAKE.sleep(delay);
    }
}

#[cfg(test)]
mod rtt_degrade_tests {
    use super::rtt_degraded_check;

    #[test]
    fn 短い超過では案内しない() {
        // 80ms 超が始まって 9 秒ではまだ案内しない(境界: 継続 10 秒未満)
        let (guide, st) = rtt_degraded_check((0, 0), 120, 1_000);
        assert!(!guide);
        let (guide, st) = rtt_degraded_check(st, 120, 10_900);
        assert!(!guide);
        // 10 秒に達した時点で案内(初回は last_notify=0 → スロットル空き)
        let (guide, _) = rtt_degraded_check(st, 120, 11_000);
        assert!(guide);
    }

    #[test]
    fn 閾値未満に戻ったら継続をリセット() {
        let (_, st) = rtt_degraded_check((0, 0), 120, 1_000);
        let (guide, st) = rtt_degraded_check(st, 30, 5_000);
        assert!(!guide);
        assert_eq!(st.0, 0, "RTT が正常に戻ったら継続時刻はリセット");
        // 戻ってからまた超え始めたら、継続は最初から数え直し
        let (guide, st) = rtt_degraded_check(st, 120, 6_000);
        assert!(!guide);
        let (guide, _) = rtt_degraded_check(st, 120, 15_900);
        assert!(!guide, "再超過から 9.9 秒なのでまだ案内しない");
    }

    #[test]
    fn 案内後は10分間のスロットル() {
        let (_, st) = rtt_degraded_check((0, 0), 120, 1_000);
        let (guide, st) = rtt_degraded_check(st, 120, 11_000);
        assert!(guide);
        // 案内直後(継続は続いている)はスロットル内なので出さない
        let (guide, st) = rtt_degraded_check(st, 120, 30_000);
        assert!(!guide);
        // 10 分経てば再び案内(継続がまだ続いている想定)
        let (guide, _) = rtt_degraded_check(st, 120, 611_000);
        assert!(guide);
    }

    #[test]
    fn 閾値ちょうどは超過扱い() {
        // rtt=80 は「80ms 超」のログと同じ条件(>= 80)で超過扱い
        let (guide, st) = rtt_degraded_check((0, 0), 80, 1_000);
        assert!(!guide);
        let (guide, _) = rtt_degraded_check(st, 80, 11_000);
        assert!(guide);
        let (_, st) = rtt_degraded_check((0, 0), 79, 1_000);
        assert_eq!(st.0, 0, "79ms は正常扱いで開始もしない");
    }
}
