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
}
