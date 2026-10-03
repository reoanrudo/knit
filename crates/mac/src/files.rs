use crate::*;
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
        notify("Knit", "この Mac ではファイルの共有が許可されていません(KNIT_SHARE)");
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
                    "{} 件・合計 {} を {peer} へ送信します(相手の保存先 Downloads/Knit の空き容量をご確認ください)",
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
                // collect 時(フォルダ走査)と送信時(open 失敗)の両方の読み取り不可を通知に反映
                let mut all_skipped = cr.skipped.clone();
                all_skipped.extend(rep.skipped.iter().cloned());
                let skip_note = skip_note(&all_skipped);
                // 相手の受理結果を待つ(版 15 以降の相手だけ返す。旧版はタイムアウト)。
                // scope=true は設定による拒否、rejected>0 は相手側での保存失敗
                match wait_xfer_ack(Duration::from_millis(2500)) {
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
                        notify(
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

/// 掴んだまま境界を越えたファイルを Windows へ渡す準備。本線の予告(DragOffer)と
/// ファイル転送に同じ受け渡しIDを付け、Windows は越えてきた押下が続いている間に
/// 届いた場合だけ OLE ドラッグを始める(離した後の転送を別の操作に混ぜない)。
/// 予告は押下より前に本線へ積む。受け付けなければ None を返し、呼び出し元は
/// 境界を越えない。tap スレッドから呼ぶため、ファイル情報の読み出しは 1 回にする
pub(crate) fn offer_drag_to_win(paths: &[std::path::PathBuf]) -> Option<u64> {
    if paths.is_empty() || paths.len() > knit_common::drag::MAX_FILES {
        notify(
            "Knit",
            &format!(
                "掴んだまま渡せるのは {} フォルダ/ファイルまでです",
                knit_common::drag::MAX_FILES
            ),
        );
        return None;
    }
    // 越境を確定する前にファイル経路が張れていることを確かめる(M7)。
    // 張り直し中に越えると元のドラッグは合成 Up で終わるのに転送が届かず、
    // 「どこにも渡らない」結果になるため、拒んで Mac 側のドラッグを続けさせる。
    // is_up_fast はロックを待たない(転送中の slot ロック待ちで tap が止まらない)
    if !BULK_LINK.is_up_fast() {
        eprintln!("[drag] mac->win 対象外: ファイル経路が未接続");
        notify(
            "Knit",
            "ファイルの転送経路がまだ準備中です。少し待ってもう一度掴んでください",
        );
        return None;
    }
    // 相手の版が持つ機能(フォルダ=版 14 以降・空フォルダ=版 15 以降)
    let f = knit_common::proto::peer_features(PEER_VERSION.load(Ordering::Relaxed));
    let allow_dirs = f.dirs;
    let keep_empty_dirs = f.empty_dirs;
    // 掴み検出時にポーリングスレッドで集計済みの結果が使えるなら使い、
    // この tap スレッドでのフォルダ走査を省く(M6)。版が変わって集計条件が
    // 異なる結果は使い回せない(版 14 相手に版 15 用の空フォルダ入りが流れると
    // サイズ 0 の空ファイルとして実体化してしまう)
    let mut cached_entries: Option<std::io::Result<bulk::CollectResult>> = None;
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
                cached_entries = Some(c.result);
                break;
            }
            Some(_) => {
                // 保存済みだが条件が違う(接続先の版が変わった等)。ここで走査し直すと
                // tap スレッドを長く止めるため、越境を断って掴み直してもらう
                eprintln!("[drag] mac->win 対象外: 集計条件が変わっています");
                notify(
                    "Knit",
                    "接続の状態が変わったため、ファイルをもう一度掴み直してください",
                );
                return None;
            }
            None => {
                // 集計スレッドがまだ書いていない: 少しだけ待つ(合計 150ms 未満。
                // ここで同期走査に落とすと深いフォルダで CGEventTap がタイムアウト
                // し、Mac 全体の入力が瞬断する)
                if attempt < 14 {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
    let result = match cached_entries {
        Some(e) => e,
        None => {
            eprintln!("[drag] mac->win 対象外: 集計結果の準備が間に合っていません");
            notify(
                "Knit",
                "ファイルの確認がまだ終わっていません。少し待ってからもう一度境界へ動かしてください",
            );
            return None;
        }
    };
    let cr = match result {
        Ok(cr) => cr,
        Err(e) => {
            eprintln!("[drag] mac->win 対象外: {e}");
            // ラベルに実際の値と上限が入る(通常送信と同じ文言生成を使う)
            notify(
                "Knit",
                &format!(
                    "掴んだまま渡せません({})",
                    knit_common::bulk::send_error_label(&e)
                ),
            );
            return None;
        }
    };
    let entries = cr.entries;
    // リンク等で送れない項目は展開後の件数に含まれない。件数が合っているように
    // 見えて黙って欠けるのを防ぐため、skipped があれば越境のタイミングで通知する
    let skip_note = skip_note(&cr.skipped);
    if !skip_note.is_empty() {
        notify(
            "Knit",
            &format!("掴んだまま渡せるのは通常のファイルとフォルダだけです{skip_note}"),
        );
    }
    if entries.is_empty() {
        eprintln!("[drag] mac->win 対象外: 渡れるものが無い");
        notify("Knit", "掴んだ項目の中に送れるものがありません");
        return None;
    }
    if FILE_TX_BUSY.swap(true, Ordering::Relaxed) {
        eprintln!("[drag] mac->win 送信中のため受け付けません");
        notify(
            "Knit",
            "前のファイルを転送中です。完了後にもう一度掴んでください",
        );
        return None;
    }
    // Windows 側が自分で採番する受け渡しIDと重ならないよう最上位ビットを立てる。
    // 採番は NEXT_TX_ID に一本化(独立カウンタだと通常送信と同じ ID を振り得る)
    let id = (1 << 63) | NEXT_TX_ID.fetch_add(1, Ordering::Relaxed);
    let count = entries.len();
    let total = bulk::entries_total(&entries);
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
        let guard = TxGuard::new();
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
            }
            Err(e) => {
                eprintln!("[drag] mac->win {id} 転送失敗: {e}");
                // 相手側の待ち状態(Pending・Carried)を確実に解く
                send_msg(&Msg::DragCancel { id });
                if e.kind() == std::io::ErrorKind::Interrupted {
                    notify("Knit", "ファイル転送を中止しました");
                } else {
                    notify(
                        "Knit",
                        &format!(
                            "{peer} へファイルを渡せませんでした({}。接続を確認してもう一度掴んでください)",
                            knit_common::bulk::send_error_label(e)
                        ),
                    );
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
            let place = std::env::var_os("HOME")
                .map(|h| std::path::Path::new(&h).join("Downloads/Knit"))
                .filter(|p| !p.as_os_str().is_empty())
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "既定の保存先(Downloads/Knit)".to_string());
            // 同名衝突で「名前 (n)」へ保存した分を通知に添える(黙って別名になるのを防ぐ)
            let rename_note = rename_note(renamed);
            if ok {
                eprintln!("[file] win->mac 受信: {n} 件(⌘V で貼り付け可)");
                history_push_files(&paths, history_device_for_active());
                let total = knit_common::bulk::total_size(&paths);
                notify(
                    "Knit",
                    &format!(
                        "ファイルを受信: {n} 件({}){rename_note}(⌘V で貼り付け可)。実体は {place}",
                        human_bytes(total),
                    ),
                );
            } else {
                // 実体は保存済みだが ⌘V に載らなかった。黙っていると
                // 「受信したのに貼り付けられない」というトラブルに見えるため伝える
                eprintln!("[file] win->mac 受信: {n} 件(クリップボード載せ失敗)");
                history_push_files(&paths, history_device_for_active());
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
                history_push_image(&bmp, "Windows");
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
            "{}件を保存できませんでした({reason}: {name}{more})。保存先(Downloads/Knit)の空き容量を確認してください",
            failed.len()
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
struct TxGuard(std::cell::Cell<Option<u64>>);
impl TxGuard {
    fn new() -> Self {
        TxGuard(std::cell::Cell::new(None))
    }
    fn arm(&self, id: u64) {
        self.0.set(Some(id));
    }
}
impl Drop for TxGuard {
    fn drop(&mut self) {
        FILE_TX_BUSY.store(false, Ordering::Relaxed);
        if let Some(id) = self.0.get() {
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
/// 直近に送ったクリップボードファイルの指紋(同じ ⌘C の再送防止)
pub(crate) static LAST_SENT_FILES: Mutex<String> = Mutex::new(String::new());

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
