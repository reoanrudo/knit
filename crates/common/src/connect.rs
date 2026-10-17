    //! 複数の接続候補(有線直結・LAN・Tailscale 等)へ同時に接続を試み、最初に
    //! 繋がったものを使う。遅延の小さい経路ほど早く繋がるため、自然に最速経路が選ばれ、
    //! 一つが使えなくなっても次回の接続で別経路へ切り替わる
    use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
    use std::time::Duration;

    /// 接続先手動指定(KNIT_HOST)の入力検証。カンマ区切りの各要素が IP アドレス
    /// (IPv4/IPv6)として書けているかを確かめ、最初の不正要素を Err で返す
    /// (通知の文言に使う)。空欄=自動発見へ戻す指定のため空文字は Ok。
    /// Mac 設定の imp_save_host と Windows トレイの MENU_SAVEHOST が同じ基準で
    /// 検証するための共有関数(ホスト名は接続先指定で使わないため入力ミスを止める)
    pub fn validate_host_input(text: &str) -> Result<(), String> {
        if let Some(bad) = text
            .split(',')
            .map(str::trim)
            .filter(|h| !h.is_empty())
            .find(|h| h.parse::<std::net::IpAddr>().is_err())
        {
            Err(bad.to_string())
        } else {
            Ok(())
        }
    }

    /// "host1,host2:port" のようなカンマ区切りを解決する(ポート省略時は既定ポート)
    pub fn parse_hosts(list: &str, port: u16) -> Vec<SocketAddr> {
        list.split(',')
            .map(str::trim)
            .filter(|h| !h.is_empty())
            .flat_map(|h| {
                // [v6]:port 形式と素の IPv6 リテラル(: が複数)を IPv4/ホスト名と
                // 区別する。IPv6 に port を二重付与すると黙って解決に失敗する
                let with_port = if h.matches(':').count() > 1 {
                    if h.starts_with('[') {
                        h.to_string()
                    } else {
                        format!("[{h}]:{port}")
                    }
                } else if h.contains(':') {
                    h.to_string()
                } else {
                    format!("{h}:{port}")
                };
                match with_port.to_socket_addrs() {
                    Ok(mut it) => it.next(),
                    Err(e) => {
                        eprintln!("[conn] 接続先 {h} を解決できません: {e}");
                        None
                    }
                }
            })
            .collect()
    }

    /// 発見結果と手動指定を併合する(発見を先頭・IP+ポート単位で重複排除)。
    /// LAN 直と Tailscale を両方候補へ並べるため、first_reachable が自然に最速経路を採用する
    pub(crate) fn merge_candidates(
        found: Vec<std::net::IpAddr>,
        hosts: Option<&str>,
        port: u16,
    ) -> Vec<SocketAddr> {
        let mut addrs: Vec<SocketAddr> = found
            .into_iter()
            .map(|ip| SocketAddr::new(ip, port))
            .collect();
        if let Some(h) = hosts {
            for a in parse_hosts(h, port) {
                if !addrs.contains(&a) {
                    addrs.push(a);
                }
            }
        }
        addrs
    }

    /// 接続候補: LAN 自動発見の結果を先頭に、指定(KNIT_HOST)を併せて返す。
    /// 同じ LAN にいれば発見=LAN 直が最速で、いなければ指定へフォールバックする。
    /// Tailscale 経路は許可時(KNIT_ALLOW_TS=1)だけ候補に残す: 拒否時に試すと
    /// 遠隔の相手が受け入れ拒否へ延々と再試行し続けることになる
    pub fn resolve(hosts: Option<&str>, port: u16, token: &str) -> Vec<SocketAddr> {
        let found = crate::discover::seek_first_lan(port, token)
            .into_iter()
            .collect::<Vec<_>>();
        let mut candidates = merge_candidates(found, hosts, port);
        if hosts.is_none() {
            if let Some(peer) = crate::credentials::load_peer() {
                if !candidates.contains(&peer) {
                    candidates.push(peer);
                }
            }
        }
        retain_allowed(&mut candidates);
        candidates
    }

    /// Tailscale 経路は許可時(KNIT_ALLOW_TS=1)だけ候補に残す: 拒否時に試すと
    /// 遠隔の相手が受け入れ拒否へ延々と再試行し続けることになる
    fn retain_allowed(candidates: &mut Vec<SocketAddr>) {
        if !crate::net::tailscale_allowed() {
            candidates.retain(|a| !crate::net::is_tailscale(a.ip()));
        }
    }

    /// LAN 発見の満了(seek_first_lan の 600ms)を待たずにダイヤルできる既知候補。
    /// 優先順位と絞り込みは resolve と同じ(指定 → 保存済みピア、Tailscale は許可時のみ)
    pub fn known_candidates(hosts: Option<&str>, port: u16) -> Vec<SocketAddr> {
        let mut addrs = match hosts {
            Some(h) => parse_hosts(h, port),
            None => Vec::new(),
        };
        // 指定があるときは保存済みピアを混ぜない(resolve と同じ規則)
        if hosts.is_none() {
            if let Some(peer) = crate::credentials::load_peer() {
                if !addrs.contains(&peer) {
                    addrs.push(peer);
                }
            }
        }
        retain_allowed(&mut addrs);
        addrs
    }

    /// 1 候補へ繋ぎ、成功したら channel へ流す(first_reachable と connect_first の共通部分)
    fn dial(
        addr: SocketAddr,
        timeout: Duration,
        tx: &std::sync::mpsc::Sender<(TcpStream, SocketAddr)>,
    ) {
        if let Ok(s) = TcpStream::connect_timeout(&addr, timeout) {
            crate::net::tune_tcp(&s);
            let _ = tx.send((s, addr));
        }
    }

    pub fn first_reachable(
        addrs: &[SocketAddr],
        timeout: Duration,
    ) -> Option<(TcpStream, SocketAddr)> {
        let (tx, rx) = std::sync::mpsc::channel();
        for a in addrs.iter().copied() {
            let tx = tx.clone();
            std::thread::spawn(move || dial(a, timeout, &tx));
        }
        drop(tx);
        rx.recv_timeout(timeout + Duration::from_millis(500)).ok()
    }

    /// 既知候補のダイヤルを LAN 発見へ僅かに道を譲る時間。LAN 内なら発見の応答は
    /// 数 ms で返るため、これだけ待っても従来どおり LAN 直が選ばれる
    const KNOWN_DIAL_HEAD_START: Duration = Duration::from_millis(100);

    /// 既知候補(known_candidates: KNIT_HOST・保存済みピア)へのダイヤルと LAN 発見を
    /// 並行に走らせ、最初に確立した 1 本を返す。従来は resolve が発見の満了(600ms)まで
    /// 待ってから最初のダイヤルを撃っていたため、発見が応答しない環境(Tailscale・
    /// AP 隔離・サブネット越え・相手が再起動中)では再接続 1 回ごとにその 600ms を
    /// 捨てていた。ここでは既知候補のダイヤルを先に始め、LAN 発見は並行して走らせる。
    /// 全滅した場合は送信側が全て消えるため、timeout を待たずに None を返す
    pub fn connect_first(
        known: &[SocketAddr],
        port: u16,
        token: &str,
        timeout: Duration,
    ) -> Option<(TcpStream, SocketAddr)> {
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let tx = tx.clone();
            let token = token.to_string();
            std::thread::spawn(move || {
                if let Some(ip) = crate::discover::seek_first_lan(port, &token) {
                    dial(SocketAddr::new(ip, port), timeout, &tx);
                }
            });
        }
        for a in known.iter().copied() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                std::thread::sleep(KNOWN_DIAL_HEAD_START);
                dial(a, timeout, &tx);
            });
        }
        // 送信側を手放す: 全滅(発見の満了を含む)なら recv が即座に Err を返す
        drop(tx);
        rx.recv_timeout(timeout + Duration::from_millis(500)).ok()
    }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hosts_keeps_ipv6_literals_and_honors_explicit_ports() {
        assert_eq!(
            parse_hosts("[::1]:24900", 1234),
            vec![SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 24900))]
        );
        assert_eq!(
            parse_hosts("::1", 1234),
            vec![SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 1234))]
        );
        assert_eq!(
            parse_hosts("127.0.0.1:80", 1234),
            vec![SocketAddr::from(([127, 0, 0, 1], 80))]
        );
        assert_eq!(
            parse_hosts("127.0.0.1", 1234),
            vec![SocketAddr::from(([127, 0, 0, 1], 1234))]
        );
        assert!(parse_hosts("", 1234).is_empty());
    }

    /// 接続先入力の検証は Mac・Windows で同じ基準(IP のみ・カンマ区切り・空欄=解除)
    #[test]
    fn validate_host_input_reports_first_bad_element() {
        // 空欄・IP 単独・カンマ区切り・前後の空白は OK
        assert!(validate_host_input("").is_ok());
        assert!(validate_host_input("192.168.1.23").is_ok());
        assert!(validate_host_input(" 192.168.1.23 , ::1 ").is_ok());
        // ホスト名・ポート付き・空要素混じりは NG(最初の不正要素を返す)
        assert_eq!(
            validate_host_input("my-mac.local").unwrap_err(),
            "my-mac.local"
        );
        assert_eq!(validate_host_input("192.168.1.23:24900").unwrap_err(), "192.168.1.23:24900");
        assert_eq!(
            validate_host_input("::1, bad! , 10.0.0.5").unwrap_err(),
            "bad!"
        );
    }

    /// 既知候補(KNIT_HOST)があれば、LAN 発見の満了(600ms)を待たずに繋がること。
    /// 従来の resolve → first_reachable では発見が応答しない環境で 604ms かかっていた
    #[test]
    fn connect_first_dials_known_candidates_without_waiting_for_lan_discovery() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let known = known_candidates(Some(&addr.to_string()), addr.port());
        assert_eq!(known, vec![addr], "指定したアドレスがそのまま既知候補になる");
        let t = std::time::Instant::now();
        let (stream, picked) = connect_first(&known, addr.port(), "probe-token", Duration::from_secs(3))
            .expect("既知候補へは接続できる");
        assert_eq!(picked, addr);
        assert!(
            t.elapsed() < Duration::from_millis(300),
            "LAN 発見の満了待ちで遅れた: {:?}",
            t.elapsed()
        );
        drop(stream);
    }

    /// どの候補も繋がらず LAN 発見も空なら、connect の timeout(3 秒)を待たずに
    /// None を返す(全滅時は送信側が消えるため即座に返る性質を固定する)
    #[test]
    fn connect_first_gives_up_when_every_candidate_is_dead() {
        let port = {
            // bind してすぐ閉じる=確実に誰も待ち受けていないポート
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let known = vec![SocketAddr::from(([127, 0, 0, 1], port))];
        let t = std::time::Instant::now();
        let got = connect_first(&known, port, "probe-token", Duration::from_secs(5));
        assert!(got.is_none(), "誰もいなければ None");
        assert!(
            t.elapsed() < Duration::from_millis(2500),
            "全滅時に timeout まで待った: {:?}",
            t.elapsed()
        );
    }
}
