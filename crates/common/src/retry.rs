//! 再接続の待機戦略。常時接続ツールの再試行は「速く回復すること」と
//! 「相手・ネットワークを殴り書きしないこと」の両立が必要なため、
//! 指数バックオフ+フルジッター(AWS の推奨形)と、外部イベント
//!(スリープ復帰・ネットワーク変更)で待機を即座に中断できる Wakeable を提供する。

use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// イベントで割り込み可能な待機。notify() されると sleep が即座に返る。
/// スリープ復帰・ネットワーク変更・電源イベントを再接続ループへ
/// 「次の試行の待ち時間を飛ばす」形で伝えるために使う
pub struct Wakeable {
    flag: Mutex<bool>,
    cv: Condvar,
}

impl Wakeable {
    pub const fn new() -> Self {
        Self {
            flag: Mutex::new(false),
            cv: Condvar::new(),
        }
    }

    /// 待機中の sleep を即座に返させる。未実行の要求は次の sleep まで保持される
    pub fn notify(&self) {
        let mut f = self.flag.lock().unwrap_or_else(|e| e.into_inner());
        *f = true;
        self.cv.notify_all();
    }

    /// d だけ眠る。途中で notify されていれば true(=即再試行など)を返す
    pub fn sleep(&self, d: Duration) -> bool {
        let deadline = Instant::now() + d;
        let mut f = self.flag.lock().unwrap_or_else(|e| e.into_inner());
        if *f {
            *f = false;
            return true;
        }
        loop {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let (guard, _) = self
                .cv
                .wait_timeout(f, deadline - now)
                .unwrap_or_else(|e| e.into_inner());
            f = guard;
            if *f {
                *f = false;
                return true;
            }
        }
    }
}

impl Default for Wakeable {
    fn default() -> Self {
        Self::new()
    }
}

/// 指数バックオフ+フルジッター。遅延は [base, min(max, 現在の上限)] の一様分布から
/// 引き、現在の上限を呼び出しごとに倍々していく。同期した再試行の嵐を散らすための
/// フルジッターで、LAN の常時接続ツールでは上限を浅く(数秒)保つのが適切
pub struct Backoff {
    base: Duration,
    max: Duration,
    cur: Duration,
    rng: u64,
}

fn seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(0x9E3779B97F4A7C15);
    (nanos ^ ((std::process::id() as u64) << 32)) | 1
}

impl Backoff {
    pub fn new(base: Duration, max: Duration) -> Self {
        Self {
            base,
            max: max.max(base),
            cur: base,
            rng: seed(),
        }
    }

    /// 成功時は遅延を初期値へ戻す
    pub fn reset(&mut self) {
        self.cur = self.base;
    }

    /// 次の待ち時間を返す。呼び出すごとに上限が倍々(最大 max まで)になる
    pub fn next_delay(&mut self) -> Duration {
        let hi = self.cur.min(self.max);
        let range = hi.saturating_sub(self.base).as_millis() as u64;
        let millis = if range == 0 {
            self.base.as_millis() as u64
        } else {
            self.base.as_millis() as u64 + (self.rng() % range)
        };
        self.cur = (self.cur * 2).min(self.max);
        Duration::from_millis(millis)
    }

    /// xorshift64(ジッター用。予測不可能性は不要なため軽量なもので足りる)
    fn rng(&mut self) -> u64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_stays_within_bounds_and_grows_its_ceiling() {
        let mut b = Backoff::new(Duration::from_millis(500), Duration::from_secs(10));
        let mut firsts = Vec::new();
        for _ in 0..20 {
            let d = b.next_delay();
            assert!(d >= Duration::from_millis(500), "下限を切らない: {d:?}");
            assert!(d <= Duration::from_secs(10), "上限を超えない: {d:?}");
            firsts.push(d);
        }
        // 上限到達後も範囲内であることは上の assert で担保。上限の成長は
        // 「遅延の最大値が単調に増える(初期はほぼ base ばかり)」ことで観測する
        let mut b2 = Backoff::new(Duration::from_millis(500), Duration::from_secs(10));
        let early_max = (0..3).map(|_| b2.next_delay()).max().unwrap();
        let mut b3 = Backoff::new(Duration::from_millis(500), Duration::from_secs(10));
        for _ in 0..6 {
            b3.next_delay();
        }
        let late_max = (0..20).map(|_| b3.next_delay()).max().unwrap();
        assert!(
            late_max >= early_max,
            "試行を重ねるほど間隔は広がる: {early_max:?} vs {late_max:?}"
        );
        b3.reset();
        assert!(b3.next_delay() <= Duration::from_millis(1000));
    }

    #[test]
    fn wakeable_sleep_returns_early_on_notify() {
        let w = std::sync::Arc::new(Wakeable::new());
        let w2 = w.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            w2.notify();
        });
        let t0 = Instant::now();
        let woken = w.sleep(Duration::from_millis(5000));
        assert!(woken, "notify で早起きした");
        assert!(t0.elapsed() < Duration::from_secs(2), "5 秒待ちきっていない");
        // 要求は consume される: 直後の sleep は即座に返らない
        let w2 = std::sync::Arc::new(Wakeable::new());
        w2.notify();
        assert!(w2.sleep(Duration::from_millis(0)));
        assert!(!w2.sleep(Duration::from_millis(0)));
    }

    #[test]
    fn notify_before_sleep_is_not_lost() {
        let w = Wakeable::new();
        w.notify();
        assert!(w.sleep(Duration::from_millis(200)), "眠る前の要求も伝わる");
    }
}
