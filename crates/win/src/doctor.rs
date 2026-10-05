//! ドクター(Windows 側)。異常を実測して、安全な範囲を自動で直す。
//! 共通の判断・記録は knit_common::doctor。ここは Windows 固有の実測と修復の実行だけ。
//! 直せないもの(管理者権限が要る操作補助の導入など)は記録と案内にとどめる。
use crate::dragdrop::edge::CONTROLLED;
use crate::helper;
use crate::input::{release_all_input, BTN_W};
use crate::state::{CONNECTED, WAKE};
use crate::tray;
use knit_common::diagnose::Status;
use knit_common::doctor::{self, Finding, Fix, Governor};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_secs(5);
/// 取り残しとみなすまでの時間。Mac 側が手を離す通常の切替(数百 ms)と区別する
const STUCK_AFTER: Duration = Duration::from_secs(5);
const LINK_DOWN_AFTER: Duration = Duration::from_secs(120);
const HELPER_DOWN_AFTER: Duration = Duration::from_secs(30);

fn helper_installed() -> bool {
    std::path::Path::new(&std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into()))
        .join("Knit")
        .join("knit-win-input.exe")
        .exists()
}

fn injected_button_down() -> bool {
    BTN_W.iter().any(|b| b.load(Ordering::Relaxed))
}

/// 条件が成り立ち続けた時間を測る小道具
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
        let (mut stuck, mut link, mut helper_down) = (Since(None), Since(None), Since(None));
        let mut installed_noted = false;
        loop {
            std::thread::sleep(TICK);
            let findings = observe(&mut stuck, &mut link, &mut helper_down, &mut installed_noted);
            // 連発制限(Governor)の時刻源は単調時計の ms。壁時計を渡すと NTP 補正や
            // スリープ復帰で gap/窓の判定が狂う(Mac 側 doctor と同じ)
            let now = crate::state::now_ms();
            doctor::cycle(&findings, &mut gov, now, &mut |fix| match fix {
                Fix::ReleaseInput => {
                    release_all_input();
                    Ok("押下中の入力を離しました".into())
                }
                Fix::Reconnect => {
                    WAKE.notify();
                    Ok("再接続の待機を飛ばしました".into())
                }
            });
            // 修復の連発上限に達した初回だけバルーン通知する(ジャーナルは
            // 診断レポートを開かないと見えないため)。tray::notify は
            // どのスレッドからでも呼べる(conn.rs の接続通知と同じ経路)
            if doctor::take_exhausted() {
                tray::notify(
                    "Knit",
                    "自動修復の待ち時間の短縮を停止しました(再接続は続いています)。",
                );
            }
        }
    });
}

fn observe(
    stuck: &mut Since,
    link: &mut Since,
    helper_down: &mut Since,
    installed_noted: &mut bool,
) -> Vec<Finding> {
    let connected = CONNECTED.load(Ordering::Relaxed);
    let controlled = CONTROLLED.load(Ordering::Relaxed);
    let mut out = Vec::new();

    // 相手が離しそびれた注入ボタン: 制御されていない(または切断中)のに押下が残る
    let held = stuck.update(injected_button_down() && (!controlled || !connected));
    if held >= STUCK_AFTER {
        out.push(Finding {
            id: "入力の取り残し",
            status: Status::Fail,
            detail: format!("相手から押したボタンが {} 秒離されていません", held.as_secs()),
            fix: Some(Fix::ReleaseInput),
        });
    }

    let down = link.update(!connected);
    if down >= LINK_DOWN_AFTER {
        out.push(Finding {
            id: "接続",
            status: Status::Fail,
            detail: format!("相手と {} 秒つながっていません", down.as_secs()),
            fix: Some(Fix::Reconnect),
        });
    }

    // 操作補助は管理者権限での導入が要るため自動では直せない。案内を残す(1 回だけ)
    let hd = helper_down.update(helper_installed() && !helper::is_connected());
    if hd >= HELPER_DOWN_AFTER && !*installed_noted {
        *installed_noted = true;
        doctor::note("操作補助が導入済みなのに応答しません。トレイの「操作補助を更新…」で入れ直してください");
    }
    if hd == Duration::ZERO {
        *installed_noted = false;
    }
    out
}
