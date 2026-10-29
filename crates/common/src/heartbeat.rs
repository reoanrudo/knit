use std::time::Duration;

/// Mac の ping 送信間隔。
pub const MAC_PING_INTERVAL: Duration = Duration::from_secs(1);
/// Windows の ping 送信間隔。
pub const WIN_PING_INTERVAL: Duration = Duration::from_secs(3);
/// Windows が受信の途絶を許容する時間。
pub const WIN_READ_SILENCE: Duration = Duration::from_secs(9);
/// Mac が pong の途絶を許容する時間。
pub const MAC_PONG_SILENCE: Duration = Duration::from_secs(15);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_limits_allow_multiple_peer_ping_intervals() {
        assert!(WIN_READ_SILENCE >= MAC_PING_INTERVAL * 3);
        assert!(MAC_PONG_SILENCE >= WIN_PING_INTERVAL * 3);
        assert!(MAC_PONG_SILENCE > WIN_READ_SILENCE);
    }
}
