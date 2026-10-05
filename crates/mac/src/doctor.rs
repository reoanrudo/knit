//! ドクター(macOS 側)。入力の取り残しを実測して、安全な範囲を自動で直す。
//! 共通の判断・記録は knit_common::doctor。ここは macOS 固有の実測と修復の実行だけ。
//! 既存の自己修復(隠れたカーソルの復元・転送停止のウォッチドッグ)は main.rs に残し、
//! 動いたときの記録だけこのジャーナルへ書く。
//! アクセシビリティ権限は自動では付与できないため、案内の扱い(権限喪失時は入力を
//! 解放して終了する既存処理)にとどめる。
use crate::{leave_win_mode_cursor_unlock, notify, CONNECTED, LOCK_POS, WIN_MODE};
use knit_common::diagnose::Status;
use knit_common::doctor::{self, Finding, Fix, Governor};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_secs(1);
const STUCK_AFTER: Duration = Duration::from_secs(3);

struct Since(Option<Instant>);
impl Since {
    fn update(&mut self, on: bool) -> Duration {
        match (on, self.0) {
            (true, None) => {
                self.0 = Some(Instant::now());
                Duration::ZERO
            }
            (true, Some(t)) => t.elapsed(),
            (false, _) => {
                self.0 = None;
                Duration::ZERO
            }
        }
    }
}

pub(crate) fn start() {
    std::thread::spawn(|| {
        let mut gov = Governor::new();
        let (mut orphan_win, mut frozen) = (Since(None), Since(None));
        loop {
            std::thread::sleep(TICK);
            let mut findings = Vec::new();
            // Windows 操作中なのに接続が無い: 入力を握ったまま誰にも渡らない
            let t = orphan_win.update(
                WIN_MODE.load(Ordering::Relaxed) && !CONNECTED.load(Ordering::Relaxed),
            );
            if t >= STUCK_AFTER {
                findings.push(Finding {
                    id: "操作先の消失",
                    status: Status::Fail,
                    detail: format!("操作中の端末との接続が {} 秒途切れています", t.as_secs()),
                    fix: Some(Fix::ReleaseInput),
                });
            }
            // Mac に戻っているのにカーソルの位置固定が残る(動かなくなる)
            let f = frozen.update(
                !WIN_MODE.load(Ordering::Relaxed)
                    && LOCK_POS.lock().unwrap_or_else(|e| e.into_inner()).is_some(),
            );
            if f >= STUCK_AFTER {
                findings.push(Finding {
                    id: "カーソル固定の残り",
                    status: Status::Fail,
                    detail: format!("Macに戻ったのに位置固定が {} 秒残っています", f.as_secs()),
                    fix: Some(Fix::ReleaseInput),
                });
            }
            doctor::cycle(&findings, &mut gov, crate::now_ms(), &mut |fix| match fix {
                Fix::ReleaseInput => {
                    leave_win_mode_cursor_unlock(None);
                    Ok("カーソルと入力をMacに戻しました".into())
                }
                Fix::Reconnect => {
                    crate::WAKE.notify();
                    Ok("再接続の待機を飛ばしました".into())
                }
            });
            // 修復の連発上限に達した初回だけ通知する(ジャーナルは診断レポートを
            // 開かないと見えないため)。再接続自体は続いていることを伝える
            if doctor::take_exhausted() {
                notify(
                    "Knit",
                    "自動修復の待ち時間の短縮を停止しました(再接続は続いています)。",
                );
            }
        }
    });
}
