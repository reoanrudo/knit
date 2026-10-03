    //! 同じ LAN にいる相手の自動発見(接続先の IP を手で入れずに済むように)。
    //! 問い合わせと応答には、トークンから導いた「部屋 ID」だけを載せる(トークンそのもの
    //! は含まない)。同じトークンを持つ相手だけが応答するため、他人の Knit とは混ざらない。
    //! Tailscale や AP 隔離の環境ではブロードキャストが届かないため KNIT_HOST を併用する
    use std::net::{IpAddr, SocketAddr, UdpSocket};
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
        let Ok(sock) = UdpSocket::bind(("0.0.0.0", 0)) else {
            return None;
        };
        let _ = sock.set_broadcast(true);
        let ask = format!("KNIT?{}", room_id(token));
        let ans = format!("KNIT!{}", room_id(token));
        sock.send_to(ask.as_bytes(), target).ok()?;
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
                Err(_) => return None,
            }
        }
        None
    }

    /// LAN 全体へ問い合わせ、最初に応答した相手を 1 つ返す。
    /// LAN 内なら応答は数 ms、誰もいなくても 600ms で諦める(再接続 1 回あたりの上乗せがこれ以下)
    pub fn seek_first_lan(port: u16, token: &str) -> Option<IpAddr> {
        seek_first(
            SocketAddr::from(([255, 255, 255, 255], port + PORT_OFFSET)),
            token,
            Duration::from_millis(600),
        )
    }
