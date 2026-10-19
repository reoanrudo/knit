// 長命なワーカースレッドの番人。
//
// 常駐アプリのワーカースレッドが panic すると、これまではその機能が
// 再起動まで無言で死んでいた。ループをこの薄い supervisor で包み、
// panic を記録して指数バックオフで再開する。利用者への通知は
// 「プロセス内で最初の 1 回」だけに絞る(機能ごとに鳴らすと、同じ原因で
// 複数機能が落ちた時に通知が連打されるため)。

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 再起動待ちの上限。これ以上待つと「復帰が遅すぎる」方が目立つため 30 秒で頭打ち。
const MAX_RESTART_DELAY_SECS: u64 = 30;

/// 利用者への通知を出したか(プロセス全体で 1 回)。
static NOTIFIED_ONCE: AtomicBool = AtomicBool::new(false);

/// panic 後の再起動待ちを返す。`attempt` は「これまでに panic した回数」
/// (0 始まり): 0→1s, 1→2s, 2→4s, ... 上限 30s。
pub(crate) fn next_restart_delay(attempt: u32) -> Duration {
    // 2^6=64 まで数えれば上限 30 を必ず超えるため、巨大な attempt でも
    // シフトあふれしない(飽和させる)。
    let secs = 1u64 << attempt.min(6);
    Duration::from_secs(secs.min(MAX_RESTART_DELAY_SECS))
}

/// panic payload から読めるメッセージを取り出す。`panic!("...")` は
/// `&'static str`、`panic!("{x}")` や `unwrap()` は `String` で届く。
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "メッセージを持たない panic payload".to_string()
    }
}

/// `body` を実行し、panic したら記録してバックオフ後に再実行する。
/// 正常に return したら終了する(無限再起動しない)。
///
/// `name` はログと通知に出す機能名。呼び出し側のスレッドは既存の
/// `thread::spawn` のままでよい(この関数は同期に走る)。
pub(crate) fn run_supervised(name: &'static str, mut body: impl FnMut()) {
    let mut attempt: u32 = 0;
    loop {
        match catch_unwind(AssertUnwindSafe(&mut body)) {
            Ok(()) => return,
            Err(payload) => {
                let message = panic_message(payload);
                // スレッドに名前が付いていればそれを、無ければ機能名を出す
                // (この crate のワーカースレッドは大半が unnamed のため)。
                let thread = std::thread::current().name().unwrap_or(name).to_string();
                eprintln!(
                    "[supervise] {name} のループが panic しました(thread={thread}, 連続{}回目): {message}",
                    attempt + 1
                );
                if !NOTIFIED_ONCE.swap(true, Ordering::SeqCst) {
                    // 最初の 1 回だけ利用者へ知らせる。詳細(panic 内容・再起動の
                    // 繰り返し)はログに残るため、通知は 1 行に留める。
                    crate::notify(
                        "Knit",
                        &format!("{name}の機能が停止し、自動で再起動しました(詳細はログ)"),
                    );
                }
                let delay = next_restart_delay(attempt);
                eprintln!(
                    "[supervise] {name} を {} 秒後に再起動します",
                    delay.as_secs()
                );
                std::thread::sleep(delay);
                attempt = attempt.saturating_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{next_restart_delay, panic_message, run_supervised};
    use std::time::Duration;

    #[test]
    fn backoff_starts_at_one_second() {
        assert_eq!(next_restart_delay(0), Duration::from_secs(1));
    }

    #[test]
    fn backoff_doubles_each_attempt() {
        let got: Vec<u64> = (0..5).map(|a| next_restart_delay(a).as_secs()).collect();
        assert_eq!(got, vec![1, 2, 4, 8, 16]);
    }

    #[test]
    fn backoff_caps_at_thirty_seconds() {
        assert_eq!(next_restart_delay(5), Duration::from_secs(30));
        assert_eq!(next_restart_delay(6), Duration::from_secs(30));
        assert_eq!(next_restart_delay(7), Duration::from_secs(30));
    }

    #[test]
    fn backoff_saturates_for_huge_attempts() {
        // シフト量を飽和させても 30 秒を超えない(デバッグビルドで
        // 「attempt out of range」の panic を起こさない)。
        assert_eq!(next_restart_delay(u32::MAX), Duration::from_secs(30));
    }

    #[test]
    fn panic_message_reads_str_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("boom");
        assert_eq!(panic_message(payload), "boom");
    }

    #[test]
    fn panic_message_reads_string_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("詳細: boom"));
        assert_eq!(panic_message(payload), "詳細: boom");
    }

    #[test]
    fn panic_message_describes_non_text_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(42u32);
        assert_eq!(panic_message(payload), "メッセージを持たない panic payload");
    }

    #[test]
    fn normal_return_ends_supervision_without_restart() {
        // body が正常 return したら 1 回で終わる(無限再起動しない)。
        // panic させると 1 秒待つテストになるため、ここでは正常系だけを確認する。
        run_supervised("test", || {});
    }
}
