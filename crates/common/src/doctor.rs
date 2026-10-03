//! ドクター: 異常を見つけて、安全な範囲で自動修復する仕組みの共通部分。
//! 実測(Finding を作る)と修復の実行(Fix を行う)は各アプリが担い、ここは
//! 「同じ修復を連発しない」判断(Governor)と、何をしたかの記録(Journal)を持つ。
//! 修復は Knit 自身の状態(接続・押しっぱなし・隠れたカーソル)に限る。OS の権限や
//! 設定、利用者のデータには触らない(それらは案内だけ出す)。
//! KNIT_DOCTOR=0 で修復を止められる(診断と記録は続く)。

use crate::diagnose::Status;
use std::collections::VecDeque;
use std::sync::Mutex;

/// 自動で行える修復。増やすときは policy に連発の上限を必ず決める
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fix {
    /// 再接続の待機を飛ばして今すぐつなぎ直す
    Reconnect,
    /// 押しっぱなしのキー・ボタン・隠れたカーソルなど、入力まわりの残骸を解放する
    ReleaseInput,
}

impl Fix {
    pub fn name(self) -> &'static str {
        match self {
            Fix::Reconnect => "再接続",
            Fix::ReleaseInput => "入力の解放",
        }
    }

    /// 連発上限に達したときの説明。再接続は待ち時間の短縮だけが止まる
    /// (再接続自体は続く)ため、誤解を招く「これ以上は自動で行いません」は使わない
    fn halt_note(self) -> &'static str {
        match self {
            Fix::Reconnect => "待ち時間の短縮を停止しました(再接続自体は続いています)",
            Fix::ReleaseInput => "自動での解放をしばらく停止しました(落ち着けば再開します)",
        }
    }

    /// (同じ修復の最短間隔 ms, 窓内の上限回数, 窓 ms)
    fn policy(self) -> (u64, usize, u64) {
        match self {
            Fix::Reconnect => (30_000, 6, 600_000),
            Fix::ReleaseInput => (3_000, 10, 600_000),
        }
    }
}

/// 実測 1 件。fix があれば自動修復の候補
#[derive(Debug, Clone)]
pub struct Finding {
    pub id: &'static str,
    pub status: Status,
    pub detail: String,
    pub fix: Option<Fix>,
}

impl Finding {
    pub fn line(&self) -> String {
        let mark = match self.status {
            Status::Ok => "✓",
            Status::Warn => "!",
            Status::Fail => "✗",
            Status::Info => "・",
        };
        format!("{mark} {} {}", self.id, self.detail)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Run,
    TooSoon,
    Exhausted,
}

/// 同じ修復の連発を止める。直す→また壊れる→直す…の無限ループと、
/// 直せない異常への延々とした介入を防ぐ
#[derive(Default)]
pub struct Governor {
    log: Vec<(Fix, u64)>,
    exhausted_noted: Vec<Fix>,
}

impl Governor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn check(&mut self, fix: Fix, now_ms: u64) -> Verdict {
        let (gap, max, window) = fix.policy();
        self.log.retain(|&(_, t)| now_ms.saturating_sub(t) < window);
        let mine: Vec<u64> = self.log.iter().filter(|(f, _)| *f == fix).map(|&(_, t)| t).collect();
        if mine.last().is_some_and(|&t| now_ms.saturating_sub(t) < gap) {
            return Verdict::TooSoon;
        }
        if mine.len() >= max {
            return Verdict::Exhausted;
        }
        self.exhausted_noted.retain(|f| *f != fix);
        self.log.push((fix, now_ms));
        Verdict::Run
    }

    /// 上限到達を記録へ書くのは窓ごとに 1 回だけ(記録の洪水を防ぐ)
    fn note_exhausted_once(&mut self, fix: Fix) -> bool {
        if self.exhausted_noted.contains(&fix) {
            false
        } else {
            self.exhausted_noted.push(fix);
            true
        }
    }
}

pub fn enabled() -> bool {
    !matches!(
        crate::envutil::get("KNIT_DOCTOR").as_deref(),
        Some("0") | Some("off")
    )
}

/// 1 周: 異常な実測のうち修復候補を、重複を除いて Governor の許す範囲で実行する。
/// 実行した修復の結果は Journal に残る。戻り値は実行した修復の数
pub fn cycle(
    findings: &[Finding],
    gov: &mut Governor,
    now_ms: u64,
    apply: &mut dyn FnMut(Fix) -> Result<String, String>,
) -> usize {
    let mut done: Vec<Fix> = Vec::new();
    let mut ran = 0;
    for f in findings {
        let Some(fix) = f.fix else { continue };
        if matches!(f.status, Status::Ok | Status::Info) || done.contains(&fix) {
            continue;
        }
        done.push(fix);
        if !enabled() {
            continue;
        }
        match gov.check(fix, now_ms) {
            Verdict::TooSoon => {}
            Verdict::Exhausted => {
                if gov.note_exhausted_once(fix) {
                    note(&format!(
                        "「{}」を短時間に繰り返したため、{}({})",
                        fix.name(),
                        fix.halt_note(),
                        f.detail
                    ));
                    EXHAUSTED_EVENT.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            Verdict::Run => {
                ran += 1;
                match apply(fix) {
                    Ok(r) => note(&format!("{}: {} → {}を実行({r})", f.id, f.detail, fix.name())),
                    Err(e) => note(&format!("{}: {} → {}に失敗({e})", f.id, f.detail, fix.name())),
                }
            }
        }
    }
    ran
}

// ---------- 記録(Journal) ----------

static JOURNAL: Mutex<VecDeque<(u64, String)>> = Mutex::new(VecDeque::new());
const JOURNAL_MAX: usize = 50;

/// 修復の連発上限に達した(Exhausted)最初の 1 回だけ true が立つ。
/// ここ(common)は通知の経路を持たないため、mac/win の呼び出し側が
/// 周期タイマーでこれを読んで通知 1 回へ変換する(読むと自動で落ちる)
static EXHAUSTED_EVENT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// ドクターが修復の連発上限に達した初回だけ true を返す(読むとクリアされる)。
/// ジャーナルは診断レポートを開かないと見えないため、通知へ変換するための口
pub fn take_exhausted() -> bool {
    EXHAUSTED_EVENT.swap(false, std::sync::atomic::Ordering::Relaxed)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// ドクターの動きを記録する(ログにも出す)。既存の自己修復の箇所からも呼べる
pub fn note(text: &str) {
    println!("[doctor] {text}");
    let mut j = JOURNAL.lock().unwrap_or_else(|e| e.into_inner());
    if j.len() >= JOURNAL_MAX {
        j.pop_front();
    }
    j.push_back((now_ms(), text.to_string()));
}

/// 画面表示用: 新しい順の記録
pub fn journal_text() -> String {
    let j = JOURNAL.lock().unwrap_or_else(|e| e.into_inner());
    if j.is_empty() {
        return "ドクターの記録: なし(自動修復はまだ働いていません)".into();
    }
    let now = now_ms();
    let mut out = String::from("ドクターの記録(新しい順):");
    for (t, s) in j.iter().rev().take(10) {
        let ago = now.saturating_sub(*t) / 1000;
        let when = if ago < 60 {
            format!("{ago}秒前")
        } else if ago < 3600 {
            format!("{}分前", ago / 60)
        } else {
            format!("{}時間前", ago / 3600)
        };
        out.push_str(&format!("\n・{when} {s}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bad(fix: Fix) -> Finding {
        Finding { id: "t", status: Status::Fail, detail: "x".into(), fix: Some(fix) }
    }

    #[test]
    fn governor_blocks_rapid_repeat_then_allows_after_gap() {
        let mut g = Governor::new();
        assert_eq!(g.check(Fix::Reconnect, 1_000), Verdict::Run);
        assert_eq!(g.check(Fix::Reconnect, 10_000), Verdict::TooSoon);
        assert_eq!(g.check(Fix::Reconnect, 31_001), Verdict::Run);
    }

    #[test]
    fn governor_stops_after_limit_within_window() {
        let mut g = Governor::new();
        let mut t = 0;
        for _ in 0..6 {
            assert_eq!(g.check(Fix::Reconnect, t), Verdict::Run);
            t += 31_000;
        }
        assert_eq!(g.check(Fix::Reconnect, t), Verdict::Exhausted);
        // 窓が過ぎれば再び許す
        assert_eq!(g.check(Fix::Reconnect, t + 600_000), Verdict::Run);
    }

    #[test]
    fn exhaust_fires_take_exhausted_once_and_note_is_not_misleading() {
        let mut g = Governor::new();
        let f = [bad(Fix::Reconnect)];
        let mut t = 0;
        for _ in 0..6 {
            cycle(&f, &mut g, t, &mut |_| Ok("ok".into()));
            t += 31_000;
        }
        assert!(!take_exhausted(), "上限内の間はイベントなし");
        cycle(&f, &mut g, t, &mut |_| Ok("ok".into()));
        assert!(take_exhausted(), "上限到達の初回だけイベント");
        assert!(!take_exhausted(), "読むと落ちる(通知は 1 回)");
        // 文言: 「これ以上は自動で行いません」(再接続まで止まった誤解)を含まない
        let journal = journal_text();
        assert!(journal.contains("待ち時間の短縮を停止しました"));
        assert!(!journal.contains("これ以上は自動で行いません"));
    }

    #[test]
    fn different_fixes_are_limited_independently() {
        let mut g = Governor::new();
        assert_eq!(g.check(Fix::Reconnect, 0), Verdict::Run);
        assert_eq!(g.check(Fix::ReleaseInput, 1), Verdict::Run);
    }

    #[test]
    fn cycle_applies_each_fix_once_and_skips_healthy_findings() {
        let mut g = Governor::new();
        let ok = Finding { id: "ok", status: Status::Ok, detail: String::new(), fix: Some(Fix::Reconnect) };
        let f = [ok, bad(Fix::ReleaseInput), bad(Fix::ReleaseInput)];
        let mut calls = vec![];
        let n = cycle(&f, &mut g, 0, &mut |fx| {
            calls.push(fx);
            Ok("ok".into())
        });
        assert_eq!(n, 1);
        assert_eq!(calls, vec![Fix::ReleaseInput]);
    }

    #[test]
    fn cycle_does_not_repeat_within_gap() {
        let mut g = Governor::new();
        let f = [bad(Fix::Reconnect)];
        let mut n = 0;
        for t in [0, 5_000, 10_000] {
            n += cycle(&f, &mut g, t, &mut |_| Ok("ok".into()));
        }
        assert_eq!(n, 1);
    }
}
