use crate::*;
use knit_common::drag;
use knit_common::notice;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// ファイル群の指紋(パス+合計サイズ)。形式は common::files::key に統一
pub(crate) fn mac_files_key(paths: &[std::path::PathBuf]) -> String {
    let v: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    bulk::files_key(&v)
}

/// send_files_to_win の結果。呼び出し元は Busy と Denied を区別して扱う
///(Busy は同期済みの印を戻して再試行、Denied は通知済みなので印を戻さない)
pub enum SendFilesOutcome {
    /// 送信スレッドを開始した
    Started,
    /// 他の転送中で開始できなかった(自動再試行の余地がある)
    Busy,
    /// この Mac の共有設定でファイル共有が許可されていない(通知済み)
    Denied,
}

/// ファイル群を Windows へ送る(FileBegin → FileChunk… → FileEnd)。
/// GUI メニュー(NSOpenPanel)と Finder の ⌘C 検出の両方から呼ぶ。別スレッド実行。
/// 掴んだまま境界を越える場合は offer_drag_to_win を使う
pub fn send_files_to_win(paths: Vec<std::path::PathBuf>) -> SendFilesOutcome {
    if !knit_common::share::allow_files() {
        notify("Knit", "このPCではファイルの共有が許可されていません(KNIT_SHARE)");
        return SendFilesOutcome::Denied;
    }
    if FILE_TX_BUSY.swap(true, Ordering::Relaxed) {
        eprintln!("[file] 送信中のため要求を無視しました");
        return SendFilesOutcome::Busy;
    }
    std::thread::spawn(move || {
        let guard = TxGuard::new();
        // 相手の版が持つ機能(フォルダ=版 14 以降・空フォルダ=版 15 以降)は
        // proto::peer_features に集約した判定を使う
        let f = knit_common::proto::peer_features(PEER_VERSION.load(Ordering::Relaxed));
        let cr = match bulk::collect_with_skips(&paths, false, f.dirs, f.empty_dirs) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[file] 送信拒否: {e}");
                // ラベル側に実際の値と上限(例: 「展開後 640 件以上(上限 512 件)」)が
                // 入るため、ここでは上限の再掲をしない
                notify(
                    "Knit",
                    &format!(
                        "ファイルを送信できません({})",
                        knit_common::bulk::send_error_label(&e)
                    ),
                );
                return;
            }
        };
        let entries = cr.entries;
        if entries.is_empty() {
            eprintln!("[file] 送れるものがありません");
            notify("Knit", "送れるものがありません(フォルダが空か、読み取りできない項目です)");
            return;
        }
        let total = bulk::entries_total(&entries);
        let id = (1 << 63) | NEXT_TX_ID.fetch_add(1, Ordering::Relaxed);
        guard.arm(id);
        let t0 = std::time::Instant::now();
        eprintln!(
            "[file] 送信開始: {} 件 / 合計 {}KB",
            entries.len(),
            total / 1024
        );
        // 大容量(1GiB 以上)は開始時にも規模を知らせる。相手側の空き容量を
        // 送る前に確認する手がかりにする(小さい転送で通知を増やさない閾値)
        if total >= 1024 * 1024 * 1024 {
            let peer = crate::active_peer_label();
            notify(
                "Knit",
                &format!(
                    "{} 件・合計 {} を {peer} へ送信します(相手の保存先 Downloads/Knitの空き容量をご確認ください)",
                    entries.len(),
                    human_bytes(total)
                ),
            );
        }
        begin_xfer(id, total, &entries[0].name);
        clear_xfer_ack();
        let send = || {
            BULK_LINK.send(|w| {
                bulk::send_entries(
                    w,
                    &entries,
                    false,
                    None,
                    &mut || knit_common::xfer::take(id),
                    &mut |sent, total, name| update_xfer(id, sent, total, name),
                )
            })
        };
        // 未接続(NotConnected)は本線再接続直後の bulk 張り直しの窓で起きる。
        // 中止(Esc・メニュー)は接続の切断として伝播する設計のため、その直後の
        // 再送もこの窓に入りやすい。接続の回復を最大60秒待って再試行し、待ちの
        // 間に中止要求が来たら即座に中断する(送信枠の占有が長引かないようにする)
        let mut r = send();
        {
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            let mut attempt = 0;
            while r.as_ref().is_err_and(|e| e.kind() == std::io::ErrorKind::NotConnected)
                && std::time::Instant::now() < deadline
            {
                if knit_common::xfer::take(id) {
                    r = Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "cancelled while waiting for the transfer link",
                    ));
                    break;
                }
                attempt += 1;
                eprintln!("[file] bulk 経路の再接続を待って再試行します({attempt})");
                std::thread::sleep(Duration::from_millis(2500));
                r = send();
            }
        }
        let peer = crate::active_peer_label();
        match &r {
            Ok(rep) => {
                let secs = t0.elapsed().as_secs_f64().max(0.001);
                let n = rep.sent;
                eprintln!(
                    "[file] 送信完了({n} 件, {:.1}MB/s)",
                    total as f64 / 1024.0 / 1024.0 / secs
                );
                // 成功したら直前の失敗の記録を消す(メニューの「もう一度送る」が
                // 残り続けて、同じファイルを二重に送ってしまうのを防ぐ)
                note_outcome(
                    &notice::DeliveryOutcome {
                        peer: peer.clone(),
                        count: n,
                        result: notice::DeliveryResult::Placed,
                    },
                    &paths,
                );
                // collect 時(フォルダ走査)と送信時(open 失敗)の両方の読み取り不可を通知に反映
                let mut all_skipped = cr.skipped.clone();
                all_skipped.extend(rep.skipped.iter().cloned());
                let skip_note = skip_note(&all_skipped);
                // 相手の受理結果を待つ(版 15 以降の相手だけ返す。旧版はタイムアウト)。
                // scope=true は設定による拒否、rejected>0 は相手側での保存失敗
                let ack = wait_xfer_ack(Duration::from_millis(2500));
                match ack {
                    Some((_, _, true)) => {
                        notify(
                            "Knit",
                            &format!("{peer} の設定でファイルの受け取りが拒否されています(相手側アプリの「共有」設定を確認してください){skip_note}"),
                        );
                    }
                    Some((_, rejected, false)) if rejected > 0 => {
                        notify(
                            "Knit",
                            &format!("{n} 件を {peer} へ送信しましたが、{rejected} 件は相手側で保存できませんでした(相手側の通知と受信フォルダを確認してください){skip_note}"),
                        );
                    }
                    _ => {
                        // 読取スキップや受理未確認（旧版・Ack待ち切れ）は常時通知。
                        // 相手が全件保存したと確認できた成功だけを抑制対象にする。
                        let notify_done = if all_skipped.is_empty()
                            && ack.is_some_and(|(accepted, rejected, scope)| {
                                accepted == n && rejected == 0 && !scope
                            })
                        {
                            notify_transfer
                        } else {
                            notify
                        };
                        notify_done(
                            "Knit",
                            &format!(
                                "{n} 件({})を {peer} へ送信しました({:.1}MB/s・Ctrl+V で貼り付け){skip_note}",
                                human_bytes(total),
                                total as f64 / 1024.0 / 1024.0 / secs
                            ),
                        );
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                eprintln!("[file] 送信を中止しました");
                notify("Knit", "ファイル転送を中止しました");
            }
            Err(e) => {
                eprintln!("[file] 送信失敗: {e}");
                // 通常送信(⌘C・メニュー)の失敗も回復動線へ記録する(I6)
                let outcome = notice::DeliveryOutcome {
                    peer: peer.clone(),
                    count: entries.len(),
                    result: notice::failed_result(e, 0),
                };
                note_outcome(&outcome, &paths);
                notify(
                    "Knit",
                    &format!(
                        "{peer} へファイルを送れませんでした({}: {e})",
                        knit_common::bulk::send_error_label(e)
                    ),
                );
            }
        }
        // 履歴は実際に送信に回った分だけ(読めずスキップした項目は載せない)。
        // 判定は展開後の src で行う: ユーザー指定パス(フォルダ)のままだと、
        // フォルダ内の1ファイルが読めなくてもフォルダ全体が履歴に載ってしまう。
        // skipped は collect 段(走査)と送信段(open)の両方
        let mut skipped = cr.skipped.clone();
        skipped.extend(r.as_ref().map(|rep| rep.skipped.clone()).unwrap_or_default());
        let recorded: Vec<std::path::PathBuf> = entries
            .iter()
            .filter(|e| !skipped.contains(&e.src))
            .map(|e| e.src.clone())
            .collect();
        // 履歴判定は成否のみ(map_err で io::Error のまま渡す)
        let r_ok: Result<(), std::io::Error> = r.as_ref().map(|_| ()).map_err(|e| std::io::Error::other(format!("{e}")));
        push_files_history_on_result(&r_ok, &recorded);
    });
    SendFilesOutcome::Started
}

/// スキップ(読めなかった項目・シンボリックリンク)の通知用の追記部分。
/// 無ければ空文字列
fn skip_note(skipped: &[std::path::PathBuf]) -> String {
    if skipped.is_empty() {
        return String::new();
    }
    let name = skipped[0]
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "?".into());
    let extra = skipped.len().saturating_sub(1);
    let more = if extra > 0 { format!(" ほか{extra}件") } else { String::new() };
    format!(" ※読めない・リンク等の {} 件をスキップ: {name}{more}", skipped.len())
}

/// 相手の転送受理結果(本線の XferAck で受信ループから届く)
static XFER_ACK: Mutex<Option<(usize, usize, bool)>> = Mutex::new(None);
static XFER_ACK_CV: std::sync::Condvar = std::sync::Condvar::new();

/// 本線受信ループから呼ぶ: 相手の受理結果を記録し、待っている送信スレッドへ通知
pub(crate) fn note_xfer_ack(accepted: usize, rejected: usize, scope: bool) {
    let mut g = XFER_ACK.lock().unwrap_or_else(|e| e.into_inner());
    *g = Some((accepted, rejected, scope));
    XFER_ACK_CV.notify_all();
}

/// 送信開始前に前回の受理結果を捨てる(誤って前回分を読まないように)
pub(crate) fn clear_xfer_ack() {
    *XFER_ACK.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// 相手の受理結果を待つ。旧版相手は応答しないためタイムアウトで None
fn wait_xfer_ack(timeout: Duration) -> Option<(usize, usize, bool)> {
    let mut g = XFER_ACK.lock().unwrap_or_else(|e| e.into_inner());
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(v) = g.take() {
            return Some(v);
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return None;
        }
        let (ng, _) = XFER_ACK_CV
            .wait_timeout(g, deadline - now)
            .unwrap_or_else(|e| e.into_inner());
        g = ng;
    }
}

/// 掴みドラッグの offer に必要な bulk 経路の猶予窓(ms)。端末切替で経路を clear
/// した直後は、接続側(2 秒毎の張り直し)がまもなく新ピアへ張り替えるため、
/// この窓の内の未接続は offer を通して送信側の再試行に委ねる
pub(crate) const DRAG_LINK_REBUILD_WINDOW_MS: u64 = 10_000;

/// 掴みドラッグの offer を通してよいか。経路が張れていれば通す。張れていなくても
/// 端末切替で張り替えた直後(切替の時刻から猶予窓の内)なら通す: ここで拒否すると
/// 切替直後の掴み渡しが構造的に 100% 失敗し、入力先だけ切り替わった不一致が残る。
/// 送信側(send_drag_files_to_win)には未接続を待って再試行する既存の仕組みがある
pub(crate) fn drag_offer_link_ok(link_up: bool, ms_since_switch: Option<u64>) -> bool {
    if link_up {
        return true;
    }
    matches!(ms_since_switch, Some(ms) if ms < DRAG_LINK_REBUILD_WINDOW_MS)
}

/// 掴み渡しの拒否を出したときのログ行(ユーザーには見えないが診断レポートに出る)。
/// 文言は現行のまま。どの理由にどの行を出すかだけをここで決める
fn refusal_log(reason: &drag::Refusal, raw: Option<&str>) -> Option<String> {
    use drag::Refusal::*;
    Some(match reason {
        // 現行も通知のみでログ行を出さない(0 件も同じ)
        TooMany | EmptyGrab => return None,
        LinkDown => "[drag] mac->win 対象外: ファイル経路が未接続".to_string(),
        CollectStale => "[drag] mac->win 対象外: 集計条件が変わっています".to_string(),
        CollectTimeout => "[drag] mac->win 対象外: 集計結果の準備が間に合っていません".to_string(),
        CollectFailed { label } => {
            // 現行は生のエラーを出していた(通知は label)。生が無い場合だけ label へ落とす
            let e = raw.unwrap_or(label.as_str());
            format!("[drag] mac->win 対象外: {e}")
        }
        NothingToSend => "[drag] mac->win 対象外: 渡れるものが無い".to_string(),
        Busy => "[drag] mac->win 送信中のため受け付けません".to_string(),
    })
}

/// 掴み渡しを断る(ログ → 通知の順は現行のまま)。本文は値(`Refusal`)が持つ。
/// 戻り値は常に None(呼び出し元は境界を越えず、Mac 側のドラッグを続ける)
fn refuse_drag(reason: drag::Refusal, raw: Option<&str>) -> Option<u64> {
    if let Some(line) = refusal_log(&reason, raw) {
        eprintln!("{line}");
    }
    // 理由を**カーソルの近く**にも出す(I3)。通知はフルスクリーン等で出ない場合の保険として残す
    crate::hud::note_refusal(&reason);
    notify("Knit", &reason.notice_body());
    None
}

/// 掴んだまま境界を越えたファイルを Windows へ渡す準備。本線の予告(DragOffer)と
/// ファイル転送に同じ受け渡しIDを付け、Windows は越えてきた押下が続いている間に
/// 届いた場合だけ OLE ドラッグを始める(離した後の転送を別の操作に混ぜない)。
/// 予告は押下より前に本線へ積む。受け付けなければ None を返し、呼び出し元は
/// 境界を越えない。tap スレッドから呼ぶため、ファイル情報の読み出しは 1 回にする
pub(crate) fn offer_drag_to_win(paths: &[std::path::PathBuf]) -> Option<u64> {
    // 件数と経路は集計を待つ前に断る(順序も文言も現行のまま。I1 で値にした)
    let mut link_ok = true;
    // 越境を確定する前にファイル経路が張れていることを確かめる(M7)。
    // 張り直し中に越えると元のドラッグは合成 Up で終わるのに転送が届かず、
    // 「どこにも渡らない」結果になるため、拒んで Mac 側のドラッグを続けさせる。
    // is_up_fast はロックを待たない(転送中の slot ロック待ちで tap が止まらない)。
    // ただし端末切替(activate_peer)で経路を張り替えた直後は必然的に未接続に
    // なるため、接続側の張り直しが間に合う猶予窓の内だけ通す(切替直後の掴み
    // 渡しが構造的に失敗するのを防ぐ。実際の送信は送信スレッドの再試行に委ねる)
    if !BULK_LINK.is_up_fast() {
        let switched_at = BULK_SWITCHED_AT_MS.load(Ordering::Relaxed);
        let ms_since_switch = (switched_at > 0).then(|| now_ms().saturating_sub(switched_at));
        link_ok = drag_offer_link_ok(false, ms_since_switch);
        if link_ok {
            eprintln!("[drag] 経路張替直後のため、再接続を待って転送します(切替から約{}ms)",
                ms_since_switch.unwrap_or(0));
        }
    }
    if let drag::Verdict::Refused(reason) = drag::verdict_for(&drag::Signals {
        path_count: paths.len(),
        link_ok,
        busy: false,
        collect: drag::Collect::Pending,
    }) {
        return refuse_drag(reason, None);
    }
    // 相手の版が持つ機能(フォルダ=版 14 以降・空フォルダ=版 15 以降)
    let f = knit_common::proto::peer_features(PEER_VERSION.load(Ordering::Relaxed));
    let allow_dirs = f.dirs;
    let keep_empty_dirs = f.empty_dirs;
    // 掴み検出時にポーリングスレッドで集計済みの結果が使えるなら使い、
    // この tap スレッドでのフォルダ走査を省く(M6)。版が変わって集計条件が
    // 異なる結果は使い回せない(版 14 相手に版 15 用の空フォルダ入りが流れると
    // サイズ 0 の空ファイルとして実体化してしまう)
    let mut cached: Option<bulk::CollectResult> = None;
    // 失敗したときのログ行に使う生のエラー文言(通知は label を使う)
    let mut collect_raw_err: Option<String> = None;
    let mut collect = drag::Collect::Pending;
    for attempt in 0..15u32 {
        let got = DRAG_ENTRIES_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        match got {
            Some(c) if c.paths.as_slice() == paths
                && c.allow_dirs == allow_dirs
                && c.keep_empty_dirs == keep_empty_dirs =>
            {
                match c.result {
                    Ok(cr) => {
                        collect = drag::Collect::Done {
                            entry_count: cr.entries.len(),
                        };
                        cached = Some(cr);
                    }
                    Err(e) => {
                        collect_raw_err = Some(e.to_string());
                        collect = drag::Collect::Failed {
                            label: knit_common::bulk::send_error_label(&e),
                        };
                    }
                }
                break;
            }
            Some(_) => {
                // 保存済みだが条件が違う(接続先の版が変わった等)。ここで走査し直すと
                // tap スレッドを長く止めるため、越境を断って掴み直してもらう
                collect = drag::Collect::Stale;
                break;
            }
            None => {
                // 集計スレッドがまだ書いていない: 少しだけ待つ(合計 150ms 未満。
                // ここで同期走査に落とすと深いフォルダで CGEventTap がタイムアウト
                // し、Mac 全体の入力が瞬断する)。**待つのはここ(mac 側)**に残す
                if attempt < 14 {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
    if matches!(collect, drag::Collect::Pending) {
        collect = drag::Collect::Expired;
    }
    // リンク等で送れない項目は展開後の件数に含まれない。件数が合っているように
    // 見えて黙って欠けるのを防ぐため、skipped があれば越境のタイミングで通知する。
    // **これは拒否ではない**(通知したうえで渡せる分を渡す)ので、判定より先に出す
    if let Some(cr) = &cached {
        let skip_note = skip_note(&cr.skipped);
        if !skip_note.is_empty() {
            // 連結は common の1箇所(区切りを足しても golden list では検出できないため)
            notify("Knit", &drag::skipped_notice(&skip_note));
        }
    }
    let verdict = drag::verdict_for(&drag::Signals {
        path_count: paths.len(),
        link_ok,
        // 予約は下の swap が行う(load は「今どう見えるか」だけ。競合窓を作らない)
        busy: FILE_TX_BUSY.load(Ordering::Relaxed),
        collect,
    });
    let cr = match verdict {
        drag::Verdict::Refused(reason) => return refuse_drag(reason, collect_raw_err.as_deref()),
        // 集計中のまま戻ることは無い(上のループで Expired にする)。念のため断る
        drag::Verdict::Wait => return refuse_drag(drag::Refusal::CollectTimeout, None),
        drag::Verdict::Ready { count } => match cached {
            Some(cr) if cr.entries.len() == count => cr,
            // 判定と集計結果が食い違う(設計上あり得ない)。無言にせず同じ理由で断る
            _ => return refuse_drag(drag::Refusal::CollectTimeout, None),
        },
    };
    let entries = cr.entries;
    // 送信枠の取得は値ではなく swap で行う(load してから swap するまでの間に
    // 別の転送が始まっても二重送信にしない。現行と同じ atomic の使い方)
    if FILE_TX_BUSY.swap(true, Ordering::Relaxed) {
        return refuse_drag(drag::Refusal::Busy, None);
    }
    // Windows 側が自分で採番する受け渡しIDと重ならないよう最上位ビットを立てる。
    // 採番は NEXT_TX_ID に一本化(独立カウンタだと通常送信と同じ ID を振り得る)
    let id = DRAG_ID_BIT | NEXT_TX_ID.fetch_add(1, Ordering::Relaxed);
    // 新しい掴みが始まったので、前回の記録を消して進行中を立てる(I5)。
    // begin_xfer より前(指紋計算・集計の間)に離される窓も「未完」に含める
    DRAG_RELEASED_EARLY.store(false, Ordering::Relaxed);
    LAZY_ACCEPTED_ID.store(0, Ordering::Relaxed); // 前の受理を引き継がない
    MANIFEST_SENT_ID.store(0, Ordering::Relaxed); // 前の目録を引き継がない
    *DRAG_DONE.lock().unwrap_or_else(|e| e.into_inner()) = None; // 前の結果も引き継がない
    DRAG_IN_FLIGHT.store(true, Ordering::Relaxed);
    DRAG_UNFINISHED.store(true, Ordering::Relaxed);
    let count = entries.len();
    let total = bulk::entries_total(&entries);
    // 遅延提供(版 17)の目録は、entries を DRAG_TX へ move する前に作る
    // (中身は読まない。名前・サイズ・種別だけ)
    let manifest = peer_features_lazy().then(|| bulk::manifest_of(&entries));
    // 展開結果をそのまま送る(予告の件数・合計と厳密に一致させる)
    *DRAG_TX.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, entries));
    send_msg(&Msg::DragOffer {
        id,
        count,
        total,
        position: 0.0,
    });
    eprintln!(
        "[drag] mac->win offer {id}: {count} 件 / 合計 {}KB",
        total / 1024
    );
    // 遅延提供(版 17)が使える相手には、**データを流す前に目録を渡す**。
    // 相手は目録だけで OLE ドラッグを開始でき、押下保持が転送量の関数にならない
    // (実測: Wi-Fi 実効 30MB/s で 1GB=34秒・10GB=5分41秒 を押し続ける問題への答え)
    if let Some(files) = manifest {
        send_msg(&Msg::Manifest {
            id,
            count,
            total,
            files,
        });
        MANIFEST_SENT_ID.store(id, Ordering::Relaxed);
        eprintln!("[drag] mac->win manifest {id}: {count} 件(遅延提供)");
    }
    Some(id)
}

/// 予告済みの掴みドラッグのファイルを転送する。押下を本線へ積んだ後に呼ぶ
///(小さいファイルの完了が押下より先に Windows へ着くのを避ける)
pub(crate) fn send_drag_files_to_win(paths: Vec<std::path::PathBuf>, id: u64) {
    let stash = DRAG_TX
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .filter(|(stashed, _)| *stashed == id);
    std::thread::spawn(move || {
        // 掴みドラッグ用の後始末: 進行中フラグを必ず下ろす(早期 return でも Drop が走る)
        let guard = TxGuard::new_drag();
        let Some((id, entries)) = stash else {
            eprintln!("[drag] mac->win {id}: 予告が取り消されているため送信しません");
            return;
        };
        // ⌘C ポーリング経由の再送を指紋で抜く(通常は載らないが保険)。
        // 指紋は件数分の metadata 参照を伴うため、tap スレッドではなく
        // この転送スレッドで行う(M6)
        let key = mac_files_key(&paths);
        // 指紋は転送が成功したときだけ記録する(失敗した転送を同期済み扱いにすると、
        // 同内容の ⌘C が黙って止まり、二度と同期されない: 9/28 レビュー M4 の再発)
        let t0 = std::time::Instant::now();
        let total = bulk::entries_total(&entries);
        // 失敗の通知に件数を出す(I5)。entries はこの後も参照で使う
        let count = entries.len();
        begin_xfer(id, total, &entries[0].name);
        guard.arm(id);
        let peer = crate::active_peer_label();
        let send = || {
            BULK_LINK.send(|w| {
                bulk::send_entries(
                    w,
                    &entries,
                    true,
                    Some(id),
                    &mut || knit_common::xfer::take(id),
                    &mut |sent, total, name| update_xfer(id, sent, total, name),
                )
            })
        };
        let mut r = send();
        // 通常送信と同じ待ち戦略: 接続回復を最大60秒待つ(中止は即効く)
        {
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            let mut attempt = 0;
            while r.as_ref().is_err_and(|e| e.kind() == std::io::ErrorKind::NotConnected)
                && std::time::Instant::now() < deadline
            {
                if knit_common::xfer::take(id) {
                    r = Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "cancelled while waiting for the transfer link",
                    ));
                    break;
                }
                attempt += 1;
                eprintln!("[drag] bulk 経路の再接続を待って再試行します({attempt})");
                std::thread::sleep(Duration::from_millis(2500));
                r = send();
            }
        }
        match &r {
            Ok(rep) => {
                *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) = key;
                eprintln!(
                "[drag] mac->win {id} 転送完了({} 件, {:.1}MB/s)",
                rep.sent,
                total as f64 / 1024.0 / 1024.0 / t0.elapsed().as_secs_f64().max(0.001)
                );
                // 結果を3状態で言い切る(I5)。判断は common の純関数にあり、ここは
                // 「離した順序」を渡すだけ(配線にロジックを置かない)
                DRAG_UNFINISHED.store(false, Ordering::Relaxed);
                // 遅延提供で**受理済み**なら、相手のドロップ結果を少し待って**事実**で言い切る
                // (相手は受理の時点でドラッグを始めており、置けたかは相手しか知らない)。
                // 結果が来なければ従来どおり「離した順序」から慎重に判断する
                let accepted = wait_lazy_accepted(id, Duration::from_millis(1000));
                let done = accepted.then(|| wait_drag_done(id, Duration::from_millis(1200)));
                let result = match done {
                    Some(Some(true)) => notice::DeliveryResult::PlacedOnScreen,
                    Some(Some(false)) => notice::DeliveryResult::SavedOnly {
                        why: notice::SaveWhy::PeerDidNotPlace,
                    },
                    // 受理済みだが結果が来なかった/受理していない: 離した順序で判断する
                    _ => {
                        let early = DRAG_RELEASED_EARLY.load(Ordering::Relaxed) && !accepted;
                        notice::placed_or_saved(early)
                    }
                };
                let outcome = notice::DeliveryOutcome {
                    peer: peer.clone(),
                    count: rep.sent,
                    result,
                };
                let n = notice::compose_notice(&outcome);
                // メニューの回復動線へ記録する(成功なら消える)
                note_outcome(&outcome, &paths);
                match n.kind {
                    notice::NoticeKind::Always => notify(&n.title, &n.body),
                    notice::NoticeKind::Suppressible => notify_transfer(&n.title, &n.body),
                }
            }
            Err(e) => {
                eprintln!("[drag] mac->win {id} 転送失敗: {e}");
                // 相手側の待ち状態(Pending・Carried)を確実に解く
                send_msg(&Msg::DragCancel { id });
                if e.kind() == std::io::ErrorKind::Interrupted {
                    // 利用者の中止は「結果」ではないので、現行の文言のまま出す
                    notify("Knit", "ファイル転送を中止しました");
                } else {
                    let outcome = notice::DeliveryOutcome {
                        peer: peer.clone(),
                        count,
                        result: notice::failed_result(e, 0),
                    };
                    let n = notice::compose_notice(&outcome);
                    note_outcome(&outcome, &paths);
                    notify(&n.title, &n.body);
                }
            }
        }
        let r_ok: Result<(), std::io::Error> = r.as_ref().map(|_| ()).map_err(|e| std::io::Error::other(format!("{e}")));
        push_files_history_on_result(&r_ok, &paths);
    });
}

/// 大容量経路の受信完了(Windows からのファイル・画像)。
/// 受け取った結果は XferAck で相手へも知らせる(版 15 以降の相手は
/// 「送ったのに届いていない」ことに気づける)
pub(crate) fn mac_on_bulk(e: bulk::Event) {
    with_pool(|| match e {
        bulk::Event::Files {
            paths,
            drag_id: Some(id),
            failed,
            denied,
            ..
        } => {
            report_failed_files(&failed);
            send_msg(&Msg::XferAck {
                accepted: paths.len(),
                rejected: failed.len(),
                scope: denied > 0,
            });
            incoming_drag::receive(id, paths);
        }
        bulk::Event::Files {
            paths,
            failed,
            renamed,
            denied,
            ..
        } => {
            report_failed_files(&failed);
            send_msg(&Msg::XferAck {
                accepted: paths.len(),
                rejected: failed.len(),
                scope: denied > 0,
            });
            if paths.is_empty() {
                return;
            }
            let n = paths.len();
            let ok = unsafe { mac_clipboard_write_files(&paths) };
            // 自分が載せたファイルを Windows へ送り返さない
            *LAST_SENT_FILES.lock().unwrap_or_else(|e| e.into_inner()) = mac_files_key(&paths);
            LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);
            // 保存済み設定ではなく、今回のEndpointが実際に使っている起動時の場所。
            let place = desktop_settings::receive_dir().display().to_string();
            // 同名衝突で「名前 (n)」へ保存した分を通知に添える(黙って別名になるのを防ぐ)
            let rename_note = rename_note(renamed);
            if ok {
                eprintln!("[file] win->mac 受信: {n} 件(⌘V で貼り付け可)");
                // 履歴ラベルはバッチ開始時点の端末で固定(受信中の切替で替えない)
                history_push_files(&paths, &bulk_rx_device());
                let total = knit_common::bulk::total_size(&paths);
                // 保存失敗・拒否・同名変更の注意がある場合は設定にかかわらず通知。
                let notify_done = if failed.is_empty() && denied == 0 && renamed == 0 {
                    notify_transfer
                } else {
                    notify
                };
                notify_done(
                    "Knit",
                    &format!(
                        "ファイルを受信: {n} 件({}){rename_note}(⌘V で貼り付け可)。実体は {place}",
                        human_bytes(total),
                    ),
                );
                // 設定が有効なら受信フォルダをFinderで開く(失敗は静かに諦める)
                if desktop_settings::snapshot().open_receive_folder {
                    if let Err(e) = std::process::Command::new("/usr/bin/open")
                        .arg(desktop_settings::receive_dir())
                        .spawn()
                    {
                        eprintln!("[file] 受信フォルダを開けませんでした: {e}");
                    }
                }
            } else {
                // 実体は保存済みだが ⌘V に載らなかった。黙っていると
                // 「受信したのに貼り付けられない」というトラブルに見えるため伝える
                eprintln!("[file] win->mac 受信: {n} 件(クリップボード載せ失敗)");
                history_push_files(&paths, &bulk_rx_device());
                notify(
                    "Knit",
                    &format!(
                        "ファイルを受信: {n} 件{rename_note}。実体は {place} に保存されています(クリップボードが他アプリで使用中のため ⌘V には載りませんでした)"
                    ),
                );
            }
        }
        bulk::Event::Denied { files } => {
            // この Mac の共有設定で受け取りを拒否した。相手が誤って
            // 「送信しました」と扱わないよう、結果を知らせる
            eprintln!("[file] この Mac の設定で {files} 件の受信を拒否しました");
            send_msg(&Msg::XferAck {
                accepted: 0,
                rejected: files,
                scope: true,
            });
        }
        bulk::Event::Interrupted { saved } => {
            // バッチの途中で切断・中止(⌘C 送信の切断・Esc 中止の伝播)。
            // 保存済みの分が通知なしで受信フォルダに残るのを黙らせない
            eprintln!("[file] 転送が中断されました(保存済み {saved} 件)");
            notify(
                "Knit",
                &format!("転送の途中で中断されました(受信済みの {saved} 件は保存されています。残りは接続が戻ってから再送してください)"),
            );
        }
        bulk::Event::Image(dib) => {
            if !CLIP_SHARE.load(Ordering::Relaxed) {
                return;
            }
            LAST_IMG_RX_MS.store(now_ms(), Ordering::Relaxed);
            let bmp = dib_to_bmp(&dib);
            let ok = unsafe { mac_set_clipboard_image_bmp(&bmp) };
            LAST_SYNC_COUNT.store(clipboard_change_count(), Ordering::Relaxed);
            if ok {
                // 履歴ラベルは他の bulk 受信(ファイル)と同じくバッチ開始時点の
                // アクティブ端末(エイリアス優先。未受信の間は汎用の「端末」)
                // ここは既に BMP を作っているので BMP 版を使う(DIB 版に渡すと
                // ワーカー側で二重変換になり壊れた実体を保存する。レビューで判明)
                history_push_image_bmp(&bmp, &bulk_rx_device());
            }
            eprintln!(
                "[clip] win->mac image {}KB {}",
                dib.len() / 1024,
                if ok { "ok" } else { "FAILED" }
            );
        }
    })
}

/// 受信側で保存できなかったファイルを通知する(黙って欠けるのを防ぐ)
fn report_failed_files(failed: &[(String, String)]) {
    if failed.is_empty() {
        return;
    }
    let (name, reason) = &failed[0];
    let extra = failed.len().saturating_sub(1);
    let more = if extra > 0 { format!(" ほか{extra}件") } else { String::new() };
    notify(
        "Knit",
        &format!(
            "{}件を保存できませんでした({reason}: {name}{more})。保存先({})の空き容量を確認してください",
            failed.len(),
            desktop_settings::receive_dir().display()
        ),
    );
    eprintln!("[file] 保存失敗 {failed:?}");
}

/// 同名衝突で「名前 (n)」として保存した件数の通知用追記。無ければ空文字列
fn rename_note(renamed: usize) -> String {
    if renamed == 0 {
        return String::new();
    }
    format!("。同名の {renamed} 件は「名前 (n)」として保存しました")
}
/// ファイル送信中(多重送信の抑制)
pub(crate) static FILE_TX_BUSY: AtomicBool = AtomicBool::new(false);
/// 送信スレッドの異常終了(panic)でもフラグ・進捗表示・中止要求を解くためのガード。
/// パニック1回で Busy が永久残留すると、以後の全転送が再起動まで拒否される
struct TxGuard {
    id: std::cell::Cell<Option<u64>>,
    /// 掴みドラッグの転送か(DRAG_IN_FLIGHT / DRAG_UNFINISHED を下ろす責任を持つ)
    drag: bool,
}
impl TxGuard {
    fn new() -> Self {
        TxGuard {
            id: std::cell::Cell::new(None),
            drag: false,
        }
    }
    /// 掴みドラッグ用。offer が立てた進行中フラグを、この後始末で必ず下ろす
    /// (早期 return でも Drop が走る)
    fn new_drag() -> Self {
        TxGuard {
            id: std::cell::Cell::new(None),
            drag: true,
        }
    }
    fn arm(&self, id: u64) {
        self.id.set(Some(id));
    }
}
impl Drop for TxGuard {
    fn drop(&mut self) {
        FILE_TX_BUSY.store(false, Ordering::Relaxed);
        if self.drag {
            DRAG_IN_FLIGHT.store(false, Ordering::Relaxed);
            DRAG_UNFINISHED.store(false, Ordering::Relaxed);
        }
        if let Some(id) = self.id.get() {
            end_xfer(id);
            knit_common::xfer::discard(id);
        }
    }
}
/// 進行中のファイル送信(メニューバーの進捗表示・キャンセルで使う)
struct Xfer {
    id: u64,
    sent: u64,
    total: u64,
    label: String,
    began: std::time::Instant,
}
static XFER: Mutex<Option<Xfer>> = Mutex::new(None);
/// 直前の受け渡し結果(I6)。通知バナーは数秒で消えるうえ操作もできないため、
/// **メニューに「理由の1行」と押せる導線**を残す。成功したら消す(溜めない)
struct LastOutcome {
    /// メニューに出す短い1行(通知本文は長すぎる)
    line: String,
    /// もう一度送るための対象(空なら再送できない)
    paths: Vec<std::path::PathBuf>,
    recovery: notice::Recovery,
}
static LAST_OUTCOME: Mutex<Option<LastOutcome>> = Mutex::new(None);

/// メニュー用の短い1行(長い通知本文はメニューに入らない)
fn menu_line(result: &notice::DeliveryResult) -> String {
    match result {
        notice::DeliveryResult::Placed => "渡しました".to_string(),
        notice::DeliveryResult::PlacedOnScreen => "相手の画面に置きました".to_string(),
        notice::DeliveryResult::SavedOnly { .. } => {
            "置けず、相手の受信フォルダに保存されました".to_string()
        }
        notice::DeliveryResult::Failed { reason, .. } => {
            format!("渡せませんでした({reason})")
        }
    }
}

/// 結果を記録する(メニューの回復動線用)。成功なら記録を消す
pub(crate) fn note_outcome(outcome: &notice::DeliveryOutcome, paths: &[std::path::PathBuf]) {
    let mut g = LAST_OUTCOME.lock().unwrap_or_else(|e| e.into_inner());
    if !notice::needs_attention(&outcome.result) {
        *g = None;
        return;
    }
    *g = Some(LastOutcome {
        line: menu_line(&outcome.result),
        paths: paths.to_vec(),
        recovery: notice::recovery_for(&outcome.result),
    });
}

/// メニューに出す直前の結果(無ければ None)
pub(crate) fn last_outcome_line() -> Option<String> {
    LAST_OUTCOME
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|l| l.line.clone())
}

/// 直前の結果に対する回復手段(メニュー項目の出し分け)
pub(crate) fn last_outcome_recovery() -> notice::Recovery {
    LAST_OUTCOME
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|l| l.recovery)
        .unwrap_or(notice::Recovery::None)
}

/// 直前の失敗を同じファイルでもう一度送る(メニューの「もう一度送る」)。
/// 開始できたら true(できない場合は理由を通知する)
pub(crate) fn retry_last() -> bool {
    let paths = LAST_OUTCOME
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|l| l.paths.clone())
        .unwrap_or_default();
    if paths.is_empty() {
        notify("Knit", "もう一度送れるファイルが残っていません");
        return false;
    }
    match send_files_to_win(paths) {
        SendFilesOutcome::Started => true,
        SendFilesOutcome::Busy => {
            notify("Knit", "前のファイルを転送中です。完了後にもう一度お試しください");
            false
        }
        SendFilesOutcome::Denied => false,
    }
}

/// 受信フォルダを Finder で開く(メニュー。Windows トレイの同名項目と対称)
pub(crate) fn open_receive_dir() {
    let dir = desktop_settings::receive_dir();
    if !dir.exists() {
        // 未受信のうちは無い。無言で失敗させず理由を出す
        notify(
            "Knit",
            "受信フォルダはまだありません(ファイルを受け取ると作られます)",
        );
        return;
    }
    if std::process::Command::new("/usr/bin/open")
        .arg(&dir)
        .spawn()
        .is_err()
    {
        notify("Knit", "受信フォルダを開けませんでした");
    }
}

/// 相手が遅延提供(版 17)に対応しているか。**版ゲートに一本化**する
/// (呼び出し側に閾値リテラルを書かない。既存の `peer_features` と同じ方針)
fn peer_features_lazy() -> bool {
    knit_common::proto::peer_features(PEER_VERSION.load(Ordering::Relaxed)).lazy
}

/// 掴みドラッグの受け渡しIDに立てる最上位ビット(通常送信のIDと区別する)
const DRAG_ID_BIT: u64 = 1 << 63;
/// 掴みドラッグが進行中か(offer が通り、転送スレッドの後始末が終わるまで)
static DRAG_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
/// 進行中の掴みドラッグが**まだ渡し終わっていない**か。offer の直後(begin_xfer 前の
/// 指紋計算・集計の間)も true にする: この窓で離す「フリックして離す」操作を取りこぼすと、
/// 相手は保存のみなのに「置けた」と言ってしまう
static DRAG_UNFINISHED: AtomicBool = AtomicBool::new(false);
/// 遅延提供の目録を送った受け渡しID(0 = なし)。相手が版17のときだけ入る。
/// **速い転送では受理の往復より送信完了が先に来る**ため、受理を少し待つ判断に使う
static MANIFEST_SENT_ID: AtomicU64 = AtomicU64::new(0);

/// 遅延提供の受理(相手が**実際に**ドロップを先に受理した事実)。
/// 版(能力)ではなく受理の事実で判断する: 相手が `KNIT_LAZY=1` で動いていなければ
/// 受理は来ないので、「能力があるのに待たされる」を嘘と言わずに済む
static LAZY_ACCEPTED_ID: AtomicU64 = AtomicU64::new(0);

/// 相手から受理(DragAccept)が届いた(自分発の掴みドラッグのみ)
pub(crate) fn note_lazy_accepted(id: u64) {
    LAZY_ACCEPTED_ID.store(id, Ordering::Relaxed);
}

/// この掴みが受理済みか(受理済みなら、離すタイミングを待たなくてよい)
pub(crate) fn lazy_accepted(id: u64) -> bool {
    id != 0 && LAZY_ACCEPTED_ID.load(Ordering::Relaxed) == id
}

/// 相手がドロップを受け取ったか(W4)。相手から届いた**事実**だけを入れる
static DRAG_DONE: Mutex<Option<(u64, bool)>> = Mutex::new(None);

/// 相手から結果が届いた(`Msg::DragDone`)。自分発の掴みドラッグのみ
pub(crate) fn note_drag_done(id: u64, copied: bool) {
    *DRAG_DONE.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, copied));
}

/// 受理が届くのを少し待つ。**速い転送では送信完了が受理より先に来る**ため、
/// 待たずに判断すると「置けたのに保存のみと言う」ことになる(実運用で最も多い経路)。
/// 目録を送っていない相手(版16以前)では待たずに即 false
fn wait_lazy_accepted(id: u64, timeout: std::time::Duration) -> bool {
    if MANIFEST_SENT_ID.load(Ordering::Relaxed) != id {
        return false; // 遅延提供を交渉していない
    }
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if lazy_accepted(id) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// 結果が届くまで少し待つ(遅延提供のドロップは転送完了の後に確定する)。
/// 届いたら `Some(copied)`、来なければ `None`(推測で言い切らない)
fn wait_drag_done(id: u64, timeout: std::time::Duration) -> Option<bool> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some((got_id, copied)) = *DRAG_DONE.lock().unwrap_or_else(|e| e.into_inner()) {
            if got_id == id {
                return Some(copied);
            }
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// 掴みドラッグの転送を「渡し終わる前に離した」か(I5)。相手がネイティブのドラッグを
/// 始められるかは**離した順序**で決まるため、離した瞬間に確定させて結果に反映する
/// (渡し終わってから離した = 置けた / 途中で離した = 相手の受信フォルダに保存のみ)
static DRAG_RELEASED_EARLY: AtomicBool = AtomicBool::new(false);

/// tap の離し(EVT_LEFT_UP)から呼ぶ: 掴みドラッグの転送が未完のまま離されたら記録する。
/// **Mutex を取らない**(AtomicBool 2つの読み出しだけ): 入力経路を待たせない。
/// 通常送信(メニュー・⌘C)は対象外(ドロップの成否に関係しない)
pub(crate) fn note_press_release() {
    if DRAG_IN_FLIGHT.load(Ordering::Relaxed) && DRAG_UNFINISHED.load(Ordering::Relaxed) {
        DRAG_RELEASED_EARLY.store(true, Ordering::Relaxed);
    }
}
/// 掴みドラッグの送信内容。予告(DragOffer)時点で展開したものと同じ内容を送る
///(件数・合計が予告と厳密に一致するようにするため)
static DRAG_TX: Mutex<Option<(u64, Vec<bulk::OutFile>)>> = Mutex::new(None);
/// 掴みドラッグの対象集計(フォルダ展開・サイズ集計)を掴み検出時に先に済ませた
/// 結果。越境時の tap スレッドで同期 I/O を走らせないためのもの(M6)。
/// skipped(リンク等で送れない項目)も予告時に通知へ出すため一緒に持つ
struct DragEntriesCache {
    paths: Vec<std::path::PathBuf>,
    allow_dirs: bool,
    keep_empty_dirs: bool,
    result: std::io::Result<bulk::CollectResult>,
}
static DRAG_ENTRIES_CACHE: Mutex<Option<DragEntriesCache>> = Mutex::new(None);

/// 掴み検出直後に、越境で使う集計を別スレッドで先に済ませる。ユーザーが境界へ
/// 動くまでの間にフォルダ展開(最大512件)とサイズ集計を終えておけば、越境時の
/// tap スレッドでの走査が不要になる。未完・不一致なら offer 側で同期実行に戻る
pub(crate) fn precompute_drag_entries(paths: Vec<std::path::PathBuf>) {
    std::thread::spawn(move || {
        let f = knit_common::proto::peer_features(PEER_VERSION.load(Ordering::Relaxed));
        let result = bulk::collect_with_skips(&paths, true, f.dirs, f.empty_dirs);
        *DRAG_ENTRIES_CACHE.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(DragEntriesCache {
                paths,
                allow_dirs: f.dirs,
                keep_empty_dirs: f.empty_dirs,
                result,
            });
    });
}
/// 送信の識別ID(キャンセル要求の宛先)。通常送信も掴みドラッグもこの
/// カウンタで採番する(独立に振ると最上位ビット空間で ID が衝突し得る)
static NEXT_TX_ID: AtomicU64 = AtomicU64::new(1);
fn begin_xfer(id: u64, total: u64, label: &str) {
    *XFER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Xfer {
        id,
        sent: 0,
        total,
        label: label.to_string(),
        began: std::time::Instant::now(),
    });
}

fn update_xfer(id: u64, sent: u64, total: u64, label: &str) {
    if let Some(x) = XFER.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        if x.id == id {
            x.sent = sent;
            x.total = total;
            x.label = label.to_string();
        }
    }
}

fn end_xfer(id: u64) {
    let mut g = XFER.lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().is_some_and(|x| x.id == id) {
        *g = None;
    }
}

/// 進行中の送信があれば中止を要求する。実際の中止は送信スレッドの次のチャンクで
/// 反映され、キャンセルは接続の切断として相手側のクリーンアップに続く
pub fn cancel_active_xfer() -> bool {
    match XFER.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(x) => {
            knit_common::xfer::request(x.id);
            true
        }
        None => false,
    }
}

/// メニューバーのボタンタイトル用の進捗(例: 43%。進行中のみ)
pub fn xfer_title() -> Option<String> {
    let guard = XFER.lock().unwrap_or_else(|e| e.into_inner());
    let x = guard.as_ref()?;
    let pct = (x.sent * 100).checked_div(x.total).unwrap_or(100);
    Some(format!("{pct}%"))
}

/// メニュー状態行・キャンセル項目の表示(Esc でも中止できる)
pub fn xfer_line() -> Option<String> {
    let guard = XFER.lock().unwrap_or_else(|e| e.into_inner());
    let x = guard.as_ref()?;
    let pct = (x.sent * 100).checked_div(x.total).unwrap_or(100);
    let speed = x.began.elapsed().as_secs_f64().max(0.001);
    Some(format!(
        "転送中 {pct}%・{}/s・{}",
        human_bytes((x.sent as f64 / speed) as u64),
        x.label
    ))
}

/// 本線の転送進捗(xfer_line)が無い間の補助表示(純粋関数・単体テストで守る)。
/// ・adb_remaining: ADB 経由でタブレットへ送信中の残り件数(0=送信なし)。
///   adb push はファイルごとに最大120秒待つため、完了通知まで何も出ない
///   「画面が固まった?」状態を防ぐ(中止はできないため件数のみ出す)
/// ・rx_active: 大容量経路でファイルの受信バッチが進行中か。相手からの送信は
///   進捗がこちらからは測れないため「受信中」であることだけ出す
///(送信と受信が重なったときは、遅い ADB 側を優先して見せる)
pub(crate) fn xfer_extra_line(adb_remaining: usize, rx_active: bool) -> Option<String> {
    if adb_remaining > 0 {
        Some(format!("タブレットへ送信中(残り{adb_remaining}件)"))
    } else if rx_active {
        Some("ファイルを受信中".into())
    } else {
        None
    }
}
/// 直近に送ったクリップボードファイルの指紋(同じ ⌘C の再送防止)
pub(crate) static LAST_SENT_FILES: Mutex<String> = Mutex::new(String::new());

/// 掴みドラッグ offer の経路猶予判定(切替直後の未接続で構造的に失敗する回帰の防止)
/// 補助進捗表示(xfer_extra_line)の文言: ADB 送信は残り件数、bulk 受信は
/// 「受信中」であることだけを出す。どちらも進捗% が測れない経路のため、
/// 出ない(=完了通知しかない)状態に戻さないことをテストで守る
#[cfg(test)]
mod xfer_extra_line_tests {
    use super::xfer_extra_line;

    #[test]
    fn adb_push_shows_remaining_count() {
        // adb push はファイルごとに最大120秒待つため、残り件数が見えないと
        // 「画面が固まった?」と誤読させる。残り 1 件でも必ず文言が出る
        assert_eq!(
            xfer_extra_line(3, false).as_deref(),
            Some("タブレットへ送信中(残り3件)"),
            "残り件数がそのまま読み取れる"
        );
        assert_eq!(
            xfer_extra_line(1, false).as_deref(),
            Some("タブレットへ送信中(残り1件)")
        );
    }

    #[test]
    fn bulk_rx_shows_receiving_without_progress() {
        // 相手からの受信は進捗が測れないため「受信中」の文言のみ
        assert_eq!(
            xfer_extra_line(0, true).as_deref(),
            Some("ファイルを受信中")
        );
    }

    #[test]
    fn adb_takes_priority_and_idle_shows_nothing() {
        // 送受信の重なりでは待ちの長い ADB 側を優先(受信は完了通知で補完される)。
        // どちらも進行していないときは従来どおり何も出さない(常時表示にしない)
        assert_eq!(
            xfer_extra_line(2, true).as_deref(),
            Some("タブレットへ送信中(残り2件)")
        );
        assert_eq!(xfer_extra_line(0, false), None);
    }
}

#[cfg(test)]
mod drag_offer_link_tests {
    use super::{drag_offer_link_ok, DRAG_LINK_REBUILD_WINDOW_MS};

    #[test]
    fn connected_link_is_always_accepted() {
        assert!(drag_offer_link_ok(true, None), "接続済みは即 accept");
        assert!(drag_offer_link_ok(true, Some(999_999)));
    }

    #[test]
    fn freshly_switched_link_is_given_a_grace_window() {
        // 端末切替(activate_peer の clear)の直後は必然的に未接続になる。
        // 猶予窓の内なら通して送信側の再試行に委ねる
        assert!(
            drag_offer_link_ok(false, Some(0)),
            "切替直後(0ms)は通す"
        );
        assert!(
            drag_offer_link_ok(false, Some(DRAG_LINK_REBUILD_WINDOW_MS - 1)),
            "窓の内は通す"
        );
    }

    #[test]
    fn stale_disconnection_is_still_refused() {
        // 窓の外(接続側の張り直しが間に合わない異常)と、一度も切替していない
        // 未接続は従来どおり拒否して掴み直してもらう
        assert!(
            !drag_offer_link_ok(false, Some(DRAG_LINK_REBUILD_WINDOW_MS)),
            "猶予窓の境界の外は拒否"
        );
        assert!(
            !drag_offer_link_ok(false, Some(60_000)),
            "窓の外の未接続は拒否"
        );
        assert!(
            !drag_offer_link_ok(false, None),
            "切替を挟まない未接続は拒否(従来動作)"
        );
    }
}

#[cfg(test)]
mod xfer_ack_tests {
    // XFER_ACK は単一の static のため、テストは直列で1本にまとめる
    use super::{clear_xfer_ack, note_xfer_ack, wait_xfer_ack};
    use std::time::Duration;

    #[test]
    fn ack_is_taken_once_and_clear_prevents_stale_read() {
        // 届いた Ack は即返り、1 回の待ちで消費される(take パターン)
        clear_xfer_ack();
        note_xfer_ack(3, 1, true);
        assert_eq!(
            wait_xfer_ack(Duration::from_millis(0)),
            Some((3, 1, true)),
            "届いた Ack は即返る"
        );
        assert_eq!(
            wait_xfer_ack(Duration::from_millis(10)),
            None,
            "消費済みなので次は None(旧版相手相当)"
        );
        // 送信前の clear で前回分を読まない
        note_xfer_ack(1, 0, false);
        clear_xfer_ack();
        assert_eq!(
            wait_xfer_ack(Duration::from_millis(10)),
            None,
            "clear 後は前回分を読まない"
        );
    }
}

#[cfg(test)]
mod last_outcome_tests {
    use super::*;

    /// 直前の結果はプロセス全体の static なので、テストは直列に走らせる
    static LOCK: Mutex<()> = Mutex::new(());

    fn failed() -> notice::DeliveryOutcome {
        notice::DeliveryOutcome {
            peer: "Windows PC".to_string(),
            count: 1,
            result: notice::DeliveryResult::Failed {
                reason: "接続が切れました".to_string(),
                retryable: true,
                saved_count: 0,
            },
        }
    }

    /// メニューの1行は短く、置けていないのに成功を主張しない
    #[test]
    fn menu_line_never_claims_a_place_that_did_not_happen() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let f = menu_line(&failed().result);
        assert!(f.contains("接続が切れました"), "理由が読める: {f}");
        let saved = menu_line(&notice::DeliveryResult::SavedOnly {
            why: notice::SaveWhy::ReleasedEarly,
        });
        assert!(saved.contains("置けず"), "置けなかったと言い切る: {saved}");
        for w in notice::PLACEMENT_CLAIMS {
            assert!(!saved.contains(w), "「{w}」を入れない: {saved}");
        }
        assert!(saved.chars().count() < 30, "メニューに入る長さ: {saved}");
    }

    /// 失敗を記録 → 成功で消える(メニューに溜めない)。回復手段も連動する
    #[test]
    fn a_later_success_clears_the_recorded_failure() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let paths = vec![std::path::PathBuf::from("/tmp/knit-test-a.txt")];
        let mut outcome = failed();
        note_outcome(&outcome, &paths);
        assert!(last_outcome_line().is_some(), "失敗は残る");
        assert_eq!(last_outcome_recovery(), notice::Recovery::Resend);
        outcome.result = notice::DeliveryResult::Placed;
        note_outcome(&outcome, &paths);
        assert!(last_outcome_line().is_none(), "成功したら消える");
        assert_eq!(last_outcome_recovery(), notice::Recovery::None);
    }

    /// 直らない失敗では「もう一度送る」を出さない(理由の表示だけ)
    #[test]
    fn a_hopeless_failure_offers_no_retry() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        note_outcome(
            &notice::DeliveryOutcome {
                result: notice::DeliveryResult::Failed {
                    reason: "ファイルの共有が許可されていません".to_string(),
                    retryable: false,
                    saved_count: 0,
                },
                ..failed()
            },
            &[std::path::PathBuf::from("/tmp/knit-test-b.txt")],
        );
        assert!(last_outcome_line().is_some(), "理由は出す");
        assert_eq!(
            last_outcome_recovery(),
            notice::Recovery::None,
            "再試行は促さない"
        );
    }
}

#[cfg(test)]
mod lazy_accept_wait_tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// 目録を送っていない相手(版16以前)では待たない
    #[test]
    fn no_wait_when_no_manifest_was_sent() {
        MANIFEST_SENT_ID.store(0, Ordering::Relaxed);
        let t0 = Instant::now();
        assert!(!wait_lazy_accepted(42, Duration::from_millis(300)));
        assert!(
            t0.elapsed() < Duration::from_millis(100),
            "交渉していなければ即座に返す(実際 {:?})",
            t0.elapsed()
        );
    }

    /// 目録を送った相手では、受理が来るまで(または期限まで)待つ。
    /// 速い転送で「受理より送信完了が先」に来る場合の取りこぼしを防ぐ
    #[test]
    fn waiting_stops_as_soon_as_the_acceptance_arrives() {
        MANIFEST_SENT_ID.store(7, Ordering::Relaxed);
        LAZY_ACCEPTED_ID.store(0, Ordering::Relaxed);
        let t0 = Instant::now();
        assert!(
            !wait_lazy_accepted(7, Duration::from_millis(120)),
            "来なければ false(期限まで待つ)"
        );
        assert!(t0.elapsed() >= Duration::from_millis(100), "{:?}", t0.elapsed());
        // 受理済みなら待たずに true
        note_lazy_accepted(7);
        let t1 = Instant::now();
        assert!(wait_lazy_accepted(7, Duration::from_millis(500)));
        assert!(t1.elapsed() < Duration::from_millis(100), "{:?}", t1.elapsed());
        LAZY_ACCEPTED_ID.store(0, Ordering::Relaxed);
        MANIFEST_SENT_ID.store(0, Ordering::Relaxed);
    }
}
