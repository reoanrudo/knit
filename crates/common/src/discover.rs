    //! 同じ LAN にいる相手の自動発見(接続先の IP を手で入れずに済むように)。
    //! 問い合わせと応答には、トークンから導いた「部屋 ID」だけを載せる(トークンそのもの
    //! は含まない)。同じトークンを持つ相手だけが応答するため、他人の Knit とは混ざらない。
    //! Tailscale や AP 隔離の環境ではブロードキャストが届かないため KNIT_HOST を併用する
    use std::io::ErrorKind;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
    use std::time::{Duration, Instant};

    /// 本線ポートからの差分(24900 → 24903/UDP)
    pub const PORT_OFFSET: u16 = 3;

    pub fn room_id(token: &str) -> String {
        use blake2::Digest;
        let mut h = blake2::Blake2s256::new();
        h.update(b"knit-room-v1\0");
        h.update(token.as_bytes());
        h.finalize()[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// IPv4 アドレスとプレフィックス長(/n)から、そのサブネットの直接ブロードキャスト
    /// アドレス(ホスト部全立て)を計算する。プレフィックスが不正(33 以上)は None。
    /// 純粋関数(単体テスト対象): インタフェース列挙に依存しない
    pub fn direct_broadcast(ip: Ipv4Addr, prefix: u8) -> Option<Ipv4Addr> {
        if prefix > 32 {
            return None;
        }
        let mask = if prefix == 0 {
            0
        } else {
            !0u32 << (32 - prefix as u32)
        };
        let bcast = (u32::from(ip) & mask) | !mask;
        Some(Ipv4Addr::from(bcast))
    }

    /// LAN 内で相手が居そうなアドレスか(プライベート範囲のみ)。
    /// 直接ブロードキャストの併送はこの範囲に限る: グローバルアドレスへの
    /// ブロードキャストは出て行ってはならないため
    fn is_private_lan(ip: Ipv4Addr) -> bool {
        let o = ip.octets();
        o[0] == 10 || (o[0] == 172 && (16..=31).contains(&o[1])) || (o[0] == 192 && o[1] == 168)
    }

    /// ローカルインタフェースの IPv4 とプレフィックス長の一覧。
    /// macOS は getifaddrs(ネットマスクが正確に取れる)、Windows は
    /// GetIpAddrTable(iphlpapi)を使う。それ以外(Android 等)は std だけでは
    /// インタフェース列挙ができないため、デフォルトルートのインタフェースだけ
    /// UDP connect で拾い /24 を仮定する(パケットは出ない)
    #[cfg(target_os = "macos")]
    fn local_subnets() -> Vec<(Ipv4Addr, u8)> {
        // getifaddrs(3)。構造体は先頭 6 ポインタ分だけ読む(以降のフィールドは
        // 使わない)。sockaddr は先頭 2 バイトが [len, family](BSD 流れ)で、
        // sockaddr_in の sin_addr はオフセット 4 から 4 バイト(ネットワーク順)
        #[repr(C)]
        struct IfAddrs {
            next: *mut IfAddrs,
            name: *mut u8,
            flags: u32,
            addr: *const u8,
            netmask: *const u8,
            _dstaddr: *const u8,
        }
        unsafe extern "C" {
            fn getifaddrs(ifap: *mut *mut IfAddrs) -> i32;
            fn freeifaddrs(ifa: *mut IfAddrs);
        }
        const AF_INET: u8 = 2;
        // getifaddrs の sockaddr は先頭 2 バイトが [len, family](BSD 流れ)で、
        // sockaddr_in の sin_addr はオフセット 4 から 4 バイト(ネットワーク順)
        let read_v4 = |p: *const u8| -> Option<Ipv4Addr> {
            unsafe {
                if p.is_null() || *p.add(1) != AF_INET {
                    return None;
                }
                let b = std::slice::from_raw_parts(p.add(4), 4);
                Some(Ipv4Addr::new(b[0], b[1], b[2], b[3]))
            }
        };
        let prefix_of = |p: *const u8| -> u8 {
            unsafe {
                if p.is_null() || *p.add(1) != AF_INET {
                    return 0;
                }
                let b = std::slice::from_raw_parts(p.add(4), 4);
                let bits = (u32::from_be_bytes([b[0], b[1], b[2], b[3]])).count_ones();
                // 0(取れない/未設定)は /24 と仮定する
                if bits == 0 || bits > 32 { 24 } else { bits as u8 }
            }
        };
        unsafe {
            let mut list: *mut IfAddrs = std::ptr::null_mut();
            if getifaddrs(&mut list) != 0 {
                return Vec::new();
            }
            let mut out = Vec::new();
            let mut cur = list;
            while !cur.is_null() {
                let ifa = &*cur;
                if let Some(ip) = read_v4(ifa.addr) {
                    if !ip.is_loopback() && !ip.is_link_local() {
                        out.push((ip, prefix_of(ifa.netmask)));
                    }
                }
                cur = ifa.next;
            }
            freeifaddrs(list);
            out
        }
    }

    #[cfg(windows)]
    fn local_subnets() -> Vec<(Ipv4Addr, u8)> {
        // GetIpAddrTable(iphlpapi): IPv4 のアドレスとネットマスクの一覧。
        // GetAdaptersAddresses より構造体が小さく手宣言に向く(発見には IPv4 で十分)
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Row {
            addr: u32,
            _index: u32,
            mask: u32,
            _bcast: u32,
            _reasm: u32,
            _u1: u16,
            _u2: u16,
        }
        #[repr(C)]
        struct Table {
            num: u32,
            // 可変長(先頭 4 バイトの num 分が続く)
            rows: [Row; 1],
        }
        #[link(name = "iphlpapi")]
        unsafe extern "system" {
            fn GetIpAddrTable(t: *mut Table, size: *mut u32, order: i32) -> u32;
        }
        const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
        let mut buf = vec![0u8; 4096];
        loop {
            let mut size = buf.len() as u32;
            let r = unsafe {
                GetIpAddrTable(buf.as_mut_ptr() as *mut Table, &mut size, 0 /*順不同*/)
            };
            if r == 0 {
                break;
            }
            if r == ERROR_INSUFFICIENT_BUFFER && (size as usize) <= 1 << 20 {
                buf.resize(size as usize, 0);
                continue;
            }
            return Vec::new();
        }
        let num = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        let max = (buf.len().saturating_sub(4)) / std::mem::size_of::<Row>();
        // addr/mask はネットワークバイトオーダー(メモリ順=オクテット順)
        let mut out = Vec::new();
        for i in 0..num.min(max) {
            let off = 4 + i * std::mem::size_of::<Row>();
            let w = |o: usize| -> [u8; 4] {
                [buf[off + o], buf[off + o + 1], buf[off + o + 2], buf[off + o + 3]]
            };
            let ip = Ipv4Addr::from(w(0));
            let mask_bits = u32::from_be_bytes(w(8)).count_ones();
            if !ip.is_loopback() && !ip.is_link_local() {
                out.push((ip, if mask_bits == 0 || mask_bits > 32 { 24 } else { mask_bits as u8 }));
            }
        }
        out
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    fn local_subnets() -> Vec<(Ipv4Addr, u8)> {
        // std のみのフォールバック: デフォルトルートのインタフェース 1 つだけ
        // 拾う(connect は経路選択だけでパケットを出さない)。/24 を仮定する
        let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
            return Vec::new();
        };
        if sock.connect("8.8.8.8:53").is_err() {
            return Vec::new();
        }
        match sock.local_addr() {
            Ok(SocketAddr::V4(a)) => vec![(*a.ip(), 24)],
            _ => Vec::new(),
        }
    }

    /// この端末の IPv4 アドレス一覧(設定画面の「この端末のアドレス」表示用)。
    /// local_subnets と同じ除外条件(ループバック・リンクローカルを除く)のため、
    /// 複数 NIC(Wi-Fi・有線・Tailscale 等)のアドレスを列挙できる
    pub fn local_ipv4s() -> Vec<Ipv4Addr> {
        local_subnets().into_iter().map(|(ip, _)| ip).collect()
    }

    /// 問い合わせを送るブロードキャスト宛先一覧(ポートは呼び出し側で加算済みの
    /// 発見ポートを渡す)。限定ブロードキャスト(255.255.255.255)に加え、各
    /// インタフェースのサブネットの直接ブロードキャストへ併送できるようにする。
    /// WSL2/Docker 等の仮想 NIC がある環境では、単一の 255.255.255.255 がどの
    /// インタフェースから出るか保証されないため
    pub fn broadcast_targets(discover_port: u16) -> Vec<SocketAddr> {
        let mut out = vec![SocketAddr::from((Ipv4Addr::BROADCAST, discover_port))];
        for (ip, prefix) in local_subnets() {
            if !is_private_lan(ip) {
                continue;
            }
            if let Some(b) = direct_broadcast(ip, prefix) {
                let a = SocketAddr::from((b, discover_port));
                if !out.contains(&a) {
                    out.push(a);
                }
            }
        }
        out
    }

    /// 応答側(待受する側が動かす)。許可範囲の相手からの正しい問い合わせにだけ答える。
    /// 戻り値は bind 失敗のみ(待受に入ったら戻らない)。呼び出し側でログに出す
    pub fn respond(
        bind: &str,
        port: u16,
        token: &str,
        allow: fn(IpAddr) -> bool,
    ) -> std::io::Result<()> {
        let sock = UdpSocket::bind((bind, port))?;
        let ask = format!("KNIT?{}", room_id(token));
        let ans = format!("KNIT!{}", room_id(token));
        let mut buf = [0u8; 128];
        // 1 回の recv エラーで応答ループを抜けない: Windows の未接続 UDP ソケットは
        // 閉じた port への送信に対する ICMP(Port Unreachable)を次の recv が
        // WSAECONNRESET として拾うことがあり、1 度で終了すると恒久的に
        // LAN 発見へ応答しなくなる
        let mut err_logged_at = Instant::now() - Duration::from_secs(3600);
        loop {
            let (n, from) = match sock.recv_from(&mut buf) {
                Ok(x) => x,
                Err(e) => {
                    if err_logged_at.elapsed() >= Duration::from_secs(60) {
                        eprintln!("[discover] recv エラー(継続します): {e}");
                        err_logged_at = Instant::now();
                    }
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
            };
            if allow(from.ip()) && &buf[..n] == ask.as_bytes() {
                let _ = sock.send_to(ans.as_bytes(), from);
            }
        }
    }

    /// 問い合わせ側(早期終了版)。最初の応答が届いた時点で返る。
    /// 再接続のたびに呼ばれるため、LAN 内の実質レイテンシは応答 1 往復分で済む
    pub fn seek_first(target: SocketAddr, token: &str, wait: Duration) -> Option<IpAddr> {
        seek_first_multi(&[target], token, wait)
    }

    /// 複数宛先へ問い合わせを併送し、最初に応答した相手を 1 つ返す。
    /// 1 つでも送れれば発見の機会が残るため、個々の send_to の失敗は無視する。
    /// recv の一時エラー(WSAECONNRESET 等)は応答側と同じく継続する。
    /// 待ち時間の期限切れ(WouldBlock/TimedOut)は「誰もいなかった」扱いで終了する
    fn seek_first_multi(targets: &[SocketAddr], token: &str, wait: Duration) -> Option<IpAddr> {
        if targets.is_empty() {
            return None;
        }
        let Ok(sock) = UdpSocket::bind(("0.0.0.0", 0)) else {
            return None;
        };
        let _ = sock.set_broadcast(true);
        let ask = format!("KNIT?{}", room_id(token));
        let ans = format!("KNIT!{}", room_id(token));
        for t in targets {
            let _ = sock.send_to(ask.as_bytes(), t);
        }
        let until = Instant::now() + wait;
        let mut buf = [0u8; 128];
        // 不正な応答を受け取るたびにタイムアウトが延びないよう、残り時間を都度計算し直す
        while let Some(left) = until
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
        {
            let _ = sock.set_read_timeout(Some(left));
            match sock.recv_from(&mut buf) {
                Ok((n, from)) if &buf[..n] == ans.as_bytes() => return Some(from.ip()),
                Ok(_) => {}
                Err(e)
                    if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    return None // 待ち時間を使い切った
                }
                // 一時エラー(閉じた port への ICMP 等)は打ち切らず待ち続ける
                Err(e) => eprintln!("[discover] seek recv エラー(継続します): {e}"),
            }
        }
        None
    }

    /// LAN 全体へ問い合わせ、最初に応答した相手を 1 つ返す。
    /// LAN 内なら応答は数 ms、誰もいなくても 600ms で諦める(再接続 1 回あたりの上乗せがこれ以下)
    pub fn seek_first_lan(port: u16, token: &str) -> Option<IpAddr> {
        seek_first_multi(
            &broadcast_targets(port + PORT_OFFSET),
            token,
            Duration::from_millis(600),
        )
    }

    #[cfg(test)]
    mod tests {
        use super::direct_broadcast;
        use std::net::Ipv4Addr;

        fn v4(s: &str) -> Ipv4Addr {
            s.parse().unwrap()
        }

        #[test]
        fn computes_the_direct_broadcast_of_common_prefixes() {
            assert_eq!(direct_broadcast(v4("192.168.1.10"), 24), Some(v4("192.168.1.255")));
            assert_eq!(direct_broadcast(v4("192.168.5.10"), 16), Some(v4("192.168.255.255")));
            // 172.16/12(172.16-31)のサブネット境界
            assert_eq!(direct_broadcast(v4("172.31.100.5"), 12), Some(v4("172.31.255.255")));
            assert_eq!(direct_broadcast(v4("172.20.3.9"), 12), Some(v4("172.31.255.255")));
            assert_eq!(direct_broadcast(v4("10.6.7.8"), 8), Some(v4("10.255.255.255")));
        }

        #[test]
        fn host_bits_fill_regardless_of_host_part() {
            // 同じサブネットの任意のホスト部から同じブロードキャストへ辿り着く
            assert_eq!(
                direct_broadcast(v4("192.168.0.1"), 24),
                direct_broadcast(v4("192.168.0.254"), 24)
            );
            // プレフィックス境界をまたぐサブネットでは別のブロードキャストになる
            assert_ne!(
                direct_broadcast(v4("192.168.0.1"), 24),
                direct_broadcast(v4("192.168.1.1"), 24)
            );
        }

        #[test]
        fn rejects_invalid_prefixes() {
            assert_eq!(direct_broadcast(v4("192.168.1.1"), 33), None);
            assert_eq!(direct_broadcast(v4("192.168.1.1"), 255), None);
        }

        #[test]
        fn extreme_prefixes_are_valid() {
            // /0 = 0.0.0.0/0 全体(実用上は併送対象外だが計算としては有効)
            assert_eq!(direct_broadcast(v4("1.2.3.4"), 0), Some(v4("255.255.255.255")));
            // /32 = 単一ホスト
            assert_eq!(direct_broadcast(v4("192.168.1.7"), 32), Some(v4("192.168.1.7")));
        }
    }
