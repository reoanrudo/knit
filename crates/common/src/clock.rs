//! 時計の選び方(プロジェクト全体の時刻源の原則)。
//! - 単調時計(Instant 起点): 経過時間・間引き・期限判定。NTP 補正で値が飛んだり
//!   巻き戻ったりしないため、内部比較はすべてこちらで行う(スリープ中は止まるが
//!   「進みすぎる」ことはなく、長さの測定には安全)。
//! - 壁時計(SystemTime): ログ・表示の絶対時刻(unix ms)。スリープ復帰や NTP
//!   ステップで飛びうるため、時間の長さを測る用途には使わない。
//! 各 OS 側の `now_ms()` はここへ委譲し、壁時計が必要な表示は各 OS の `wall_ms()`
//! (ログのメトリクス行など)で別に取る。

use std::sync::OnceLock;
use std::time::Instant;

/// 単調時計の ms(プロセス起動からの経過)。
/// 経過時間・間引き・期限判定(ドクターの連発制限・再試行予定・通知の間引き等)に
/// 使う。壁時計(SystemTime)は NTP 補正やスリープ復帰で飛び、これらの判定を
/// 誤動作させるため使わない。
/// 0 を「未設定」の意味で使う箇所があるため 1 秒のオフセットを足す
/// (Mac 側の旧実装と同じ値域・同じ挙動)。
pub fn mono_now_ms() -> u64 {
    static T0: OnceLock<Instant> = OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_now_ms_never_goes_backwards() {
        // 単調時計: どのタイミングで呼んでも減らない(壁時計の NTP 巻き戻しと違う)
        let mut prev = mono_now_ms();
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            let now = mono_now_ms();
            assert!(now >= prev, "単調時計が減った: {prev} -> {now}");
            prev = now;
        }
        // 未設定(0)と区別するオフセットが乗っている
        assert!(prev >= 1_000);
    }
}
