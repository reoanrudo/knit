    //! 接続元の判定など、両 OS の待受処理で共通の小物
    use std::net::IpAddr;

    /// 接続ソケットへ OS の TCP keepalive を設定する(無通信 30 秒で探測・10 秒間隔)。
    /// アプリ層の ping/pong(10 秒)は相手の生存を早く見つけるが、NAT・ルータの
    /// フロー維持は OS の TCP keepalive の役割。Wi-Fi 中継器やモバイルルータ越しの
    /// 長時間接続で「黙って切れる」原因への対策(Deskflow v1.27 が SO_KEEPALIVE を
    /// 有効化したのと同じ方向)。失敗は無視する(keepalive が無くても通信は成立する)
    pub fn tune_tcp(stream: &std::net::TcpStream) {
        use socket2::TcpKeepalive;
        let ka = TcpKeepalive::new()
            .with_time(std::time::Duration::from_secs(30))
            .with_interval(std::time::Duration::from_secs(10));
        let _ = socket2::SockRef::from(stream).set_tcp_keepalive(&ka);
    }

    /// 接続を受け入れてよい相手か。通信は暗号化と相互認証(secure)で守られるため、
    /// 家庭・社内の LAN と有線直結(リンクローカル)を許可する。
    /// Tailscale(外出先からの遠隔操作)は既定で拒否し `KNIT_ALLOW_TS=1` の時だけ、
    /// インターネット側のアドレスは `KNIT_ALLOW_ANY=1` の時だけ許可する
    /// (旧: Tailscale を常に許可。離れた場所の Windows が勝手に再接続して境界に
    /// 現れ、マウスが消えたように見える事故の原因だった。利用形態は自宅デスク)
    pub fn is_allowed(ip: IpAddr) -> bool {
        if crate::envutil::get("KNIT_ALLOW_ANY").as_deref() == Some("1") {
            return true;
        }
        allowed(ip, tailscale_allowed())
    }

    /// Tailscale(100.64.0.0/10)での接続を許可するか。既定は OFF
    /// (家のデスク環境での LAN・直結運用のみとし、外出先からのマウス操作は行わない)
    pub fn tailscale_allowed() -> bool {
        crate::envutil::get("KNIT_ALLOW_TS").as_deref() == Some("1")
    }

    /// 接続の受け入れ範囲(is_allowed と同じ環境変数から導く)。設定画面への
    /// 表示用で、環境変数の緩和が画面から見えない問題への対策
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum AcceptScope {
        /// 既定: 家庭・社内の LAN と有線直結(リンクローカル)
        Lan,
        /// KNIT_ALLOW_TS=1: 既定に Tailscale を加える
        Tailscale,
        /// KNIT_ALLOW_ANY=1: すべてのアドレス(信頼できるネットワークでのみ使う)
        Any,
    }

    impl AcceptScope {
        /// 設定画面の「受け入れ範囲」に並べる説明
        pub fn label(self) -> &'static str {
            match self {
                Self::Lan => "LAN・有線直結(既定)",
                Self::Tailscale => "LAN・有線直結・Tailscale(KNIT_ALLOW_TS=1)",
                Self::Any => "すべてのアドレス(KNIT_ALLOW_ANY=1)",
            }
        }
        /// 警告色で示すべき範囲か(KNIT_ALLOW_ANY=1 のとき true)
        pub fn is_wide_open(self) -> bool {
            matches!(self, Self::Any)
        }
    }

    pub fn accept_scope() -> AcceptScope {
        accept_scope_from(
            crate::envutil::get("KNIT_ALLOW_ANY").as_deref(),
            crate::envutil::get("KNIT_ALLOW_TS").as_deref(),
        )
    }

    /// accept_scope の本体。環境変数の値を引数へ分離し、テストで直接扱えるようにする
    pub(crate) fn accept_scope_from(any: Option<&str>, ts: Option<&str>) -> AcceptScope {
        if any == Some("1") {
            AcceptScope::Any
        } else if ts == Some("1") {
            AcceptScope::Tailscale
        } else {
            AcceptScope::Lan
        }
    }

    /// is_allowed の本体。環境変数を引数へ分離し、テストで直接扱えるようにする
    pub(crate) fn allowed(ip: IpAddr, allow_ts: bool) -> bool {
        match ip {
            IpAddr::V4(v4) => {
                v4.is_private()
                    || v4.is_link_local()
                    || v4.is_loopback()
                    || (allow_ts && is_tailscale(ip))
            }
            IpAddr::V6(v6) => {
                v6.is_loopback()
                    || (v6.segments()[0] & 0xfe00) == 0xfc00 // ULA
                    || (v6.segments()[0] & 0xffc0) == 0xfe80 // リンクローカル
                    || v6
                        .to_ipv4_mapped()
                        .is_some_and(|v4| allowed(IpAddr::V4(v4), allow_ts))
            }
        }
    }

    /// Tailscale の CGNAT 範囲(100.64.0.0/10)か
    pub fn is_tailscale(ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(v4) => {
                let o = v4.octets();
                o[0] == 100 && (64..=127).contains(&o[1])
            }
            IpAddr::V6(_) => false,
        }
    }

    /// LAN 昇格監視の世代管理(Tailscale 接続のたびに begin で発行する)。
    /// 旧実装は Tailscale 接続のたびに監視スレッドを spawn していたため、
    /// 再接続を重ねるほど 30 秒毎の LAN 探索(UDP ブロードキャスト)が並走
    /// した。単一の常駐スレッドがこの世代を見て監視対象を切り替える
    pub struct LanPromotionWatch {
        gen: std::sync::atomic::AtomicU64,
    }

    impl LanPromotionWatch {
        pub const fn new() -> Self {
            Self {
                gen: std::sync::atomic::AtomicU64::new(0),
            }
        }
        /// 新しい Tailscale 接続を通知する(世代を発行)。戻り値は発行した世代
        pub fn begin(&self) -> u64 {
            use std::sync::atomic::Ordering;
            self.gen.fetch_add(1, Ordering::Relaxed) + 1
        }
        /// 現在の世代(0=まだ Tailscale 接続が無い)
        pub fn current(&self) -> u64 {
            use std::sync::atomic::Ordering;
            self.gen.load(Ordering::Relaxed)
        }
    }

    /// LAN 昇格監視(単一常駐スレッド)の状態機械。周期(500ms)毎に step を
    /// 呼ぶと、LAN 探索を実行してよいときだけ true を返す。
    /// 旧実装(接続ごとのスレッド spawn)の分岐をそのまま純粋な遷移へ起こした
    /// もので、挙動は等価: 確立待ち(15 秒で諦め)→ 監視(切断・別セッション・
    /// 昇格済みで待機へ)→ 30 秒毎の探索
    #[derive(Debug)]
    pub struct LanWatch {
        /// 前回見た監視世代
        seen_gen: u64,
        /// 世代に乗り換えた時点の LAST_CONNECTED_MS(確立検出の基準)
        prev_last_connected: u64,
        /// 確立待ちの経過 ms
        waited_ms: u64,
        /// 監視対象セッションの確立時刻(None=まだ確立待ち)
        established_ms: Option<u64>,
        /// この世代は監視しない(確立しなかった・切断・昇格済み。次の世代を待つ)
        idle: bool,
    }

    impl LanWatch {
        pub const fn new() -> Self {
            Self {
                seen_gen: 0,
                prev_last_connected: 0,
                waited_ms: 0,
                established_ms: None,
                idle: false,
            }
        }

        /// 1 周期分の進行。tick_ms は呼び出し周期(通常 500)。
        /// 戻り値 true のとき LAN 探索(seek)を実行してよい
        pub fn step(
            &mut self,
            current_gen: u64,
            connected: bool,
            last_connected_ms: u64,
            peer: Option<IpAddr>,
            seek_due: bool,
            tick_ms: u64,
        ) -> bool {
            // 世代が変わった: 新しい接続の監視へ乗り換える(古い監視はここで
            // 自然終了する。スレッドは残るため次の世代も拾える)
            if current_gen != self.seen_gen {
                self.seen_gen = current_gen;
                self.prev_last_connected = last_connected_ms;
                self.waited_ms = 0;
                self.established_ms = None;
                self.idle = false;
                return false;
            }
            if self.idle {
                return false;
            }
            let Some(established) = self.established_ms else {
                // 確立待ち: mark_connected による LAST_CONNECTED_MS の更新を待つ
                if connected && last_connected_ms != self.prev_last_connected {
                    self.established_ms = Some(last_connected_ms);
                    return false;
                }
                self.waited_ms += tick_ms;
                if self.waited_ms >= 15_000 {
                    // この接続は確立しなかった(旧実装の「監視しない」)
                    self.idle = true;
                }
                return false;
            };
            // 監視中: 切断・別セッションへの切替・昇格済み(または相手不明)なら待機へ
            let promoted = peer.is_some_and(|p| !is_tailscale(p));
            if !connected || last_connected_ms != established || peer.is_none() || promoted {
                self.idle = true;
                return false;
            }
            seek_due
        }
    }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_scope_follows_the_relaxation_environment_variables() {
        assert_eq!(accept_scope_from(None, None), AcceptScope::Lan);
        assert_eq!(accept_scope_from(Some("0"), Some("0")), AcceptScope::Lan);
        assert_eq!(accept_scope_from(None, Some("1")), AcceptScope::Tailscale);
        assert_eq!(accept_scope_from(Some("0"), Some("1")), AcceptScope::Tailscale);
        // ANY は TS に優先する(全許可が最も広いため)
        assert_eq!(accept_scope_from(Some("1"), None), AcceptScope::Any);
        assert_eq!(accept_scope_from(Some("1"), Some("1")), AcceptScope::Any);
        assert!(AcceptScope::Any.is_wide_open());
        assert!(!AcceptScope::Lan.is_wide_open());
        assert!(!AcceptScope::Tailscale.is_wide_open());
    }

    #[test]
    fn allowed_addresses_follow_lan_direct_and_tailscale_defaults() {
        let lan = "192.168.1.5".parse::<IpAddr>().unwrap();
        let link = "169.254.10.9".parse::<IpAddr>().unwrap();
        let ts = "100.100.10.9".parse::<IpAddr>().unwrap();
        let net = "8.8.8.8".parse::<IpAddr>().unwrap();
        assert!(allowed(lan, false));
        assert!(allowed(link, false));
        assert!(!allowed(ts, false), "Tailscale は既定で拒否");
        assert!(allowed(ts, true));
        assert!(!allowed(net, false));
        assert!(!allowed(net, true), "インターネット側は ALLOW_ANY でのみ許可");
    }

    /// LAN 昇格監視の世代: begin のたびに増え、単一スレッドが乗り換え判断に使う
    #[test]
    fn lan_promotion_generations_advance_with_each_tailscale_connection() {
        let watch = LanPromotionWatch::new();
        assert_eq!(watch.current(), 0, "最初は Tailscale 接続が無い");
        let g1 = watch.begin();
        assert_eq!(g1, 1);
        assert_eq!(watch.current(), g1);
        let g2 = watch.begin();
        assert_eq!(g2, 2, "再接続のたびに世代が増える(古い監視は不一致で終了)");
        assert!(g2 > g1);
    }

    /// LAN 昇格監視の状態機械: 確立待ち → 監視 → 30 秒周期の探索、
    /// 切断・別セッション・昇格済みでは探索しない(旧実装の並走スレッドと
    /// 等価の判定を単一スレッドの遷移として守る)
    #[test]
    fn lan_watch_steps_through_establish_then_seek() {
        let ts = "100.100.10.9".parse::<IpAddr>().unwrap();
        let mut w = LanWatch::new();
        // 世代 1 の接続が始まる(乗り換えは探索しない)
        assert!(!w.step(1, true, 500, Some(ts), true, 500));
        // 確立待ち: mark_connected 前(500 のまま)は待つ
        assert!(!w.step(1, true, 500, Some(ts), true, 500));
        // 確立(600 に更新)→ 最初の周期は探索しない
        assert!(!w.step(1, true, 600, Some(ts), true, 500));
        // 確立後・接続中・Tailscale のまま: 探索周期(seek_due)のときだけ探索
        assert!(w.step(1, true, 600, Some(ts), true, 500));
        assert!(!w.step(1, true, 600, Some(ts), false, 500), "周期外では探索しない");
        // 切断したら探索しない(旧実装の「セッション終了済み」終了条件)
        assert!(!w.step(1, false, 600, Some(ts), true, 500));
        // 待機状態は世代が変わるまで何もしない
        assert!(!w.step(1, true, 600, Some(ts), true, 500));
    }

    #[test]
    fn lan_watch_gives_up_when_the_session_never_establishes() {
        let ts = "100.100.10.9".parse::<IpAddr>().unwrap();
        let mut w = LanWatch::new();
        w.step(1, true, 500, Some(ts), true, 500);
        // 15 秒(30 周期×500ms)までは待ち続ける
        for _ in 0..29 {
            assert!(!w.step(1, true, 500, Some(ts), true, 500));
        }
        assert!(!w.step(1, true, 500, Some(ts), true, 500), "ちょうど 15 秒でもまだ手は出さない");
        // 15 秒超で諦め(この世代は確立しなかった)。以降は探索しない
        assert!(!w.step(1, true, 500, Some(ts), true, 500));
        assert!(!w.step(1, true, 500, Some(ts), true, 500), "諦めた世代では探索しない");
        // 新しい世代が来れば改めて確立を待つ
        assert!(!w.step(2, true, 500, Some(ts), true, 500));
        assert!(!w.step(2, true, 900, Some(ts), true, 500));
        assert!(w.step(2, true, 900, Some(ts), true, 500), "新しい世代の確立後は探索を再開");
    }

    #[test]
    fn lan_watch_stops_once_promoted_or_peer_lost() {
        let ts = "100.100.10.9".parse::<IpAddr>().unwrap();
        let lan = "192.168.1.20".parse::<IpAddr>().unwrap();
        let mut w = LanWatch::new();
        w.step(1, true, 500, Some(ts), true, 500);
        w.step(1, true, 600, Some(ts), true, 500);
        // 相手が Tailscale 外(LAN 直)へ変わった=昇格済み。探索しない
        assert!(!w.step(1, true, 600, Some(lan), true, 500));
        // 待機後は世代交代まで動かない
        assert!(!w.step(1, true, 600, Some(ts), true, 500));
        // 別セッション(LAST_CONNECTED_MS が変わった)でも探索しない
        let mut w2 = LanWatch::new();
        w2.step(1, true, 500, Some(ts), true, 500);
        w2.step(1, true, 600, Some(ts), true, 500);
        assert!(!w2.step(1, true, 700, Some(ts), true, 500), "別セッションでは探索しない");
        // 相手不明(peer=None)でも探索しない
        let mut w3 = LanWatch::new();
        w3.step(1, true, 500, Some(ts), true, 500);
        w3.step(1, true, 600, Some(ts), true, 500);
        assert!(!w3.step(1, true, 600, None, true, 500));
    }
}
