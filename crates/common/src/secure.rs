    //! 全経路(本線・音声・大容量)の暗号化と相互認証。
    //! Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s: 両者が同じトークンを知っている時だけ
    //! ハンドシェイクが成立し(トークン自体は回線に流れない)、接続ごとの使い捨て鍵で
    //! 暗号化する(前方秘匿性)。旧方式は平文 TCP にトークンを平文で載せ、
    //! 盗聴・改ざん耐性を Tailscale に全面依存していた
    use std::io::{self, Read, Write};
    use std::net::TcpStream;
    use std::sync::Arc;

    const PATTERN: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";
    const MAX_MSG: usize = 65535;
    const TAG: usize = 16;
    const MAX_PLAIN: usize = MAX_MSG - TAG;

    fn psk(token: &str) -> [u8; 32] {
        use blake2::Digest;
        let mut h = blake2::Blake2s256::new();
        h.update(b"knit-psk-v1\0");
        h.update(token.as_bytes());
        h.finalize().into()
    }

    /// 接続キーのフィンガープリント(短い略号・SHA-256 の先頭8桁)。設定画面に
    /// 「両端末が同じ鍵で認証中か」を確認するための表示専用で、トークン本体や
    /// 派生鍵(psk)を復元できる情報は含まない。psk とは別の固定プレフィックスで
    /// ドメイン分離する(表示用ハッシュを鍵導出に影響させない)
    pub fn fingerprint(token: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(b"knit-fingerprint-v1\0");
        h.update(token.as_bytes());
        let digest = h.finalize();
        digest.iter().take(4).map(|b| format!("{b:02X}")).collect()
    }

    /// 設定画面の暗号化の行の文言。暗号化は常時ONのためトグルは無く、状態を
    /// 読み取れる1行として出す(フィンガープリント付きなら「鍵の略号: XXXXXXXX」を
    /// 併記し、相手側画面との見比せ方を1文で添える)。
    /// 両OSで同じ文言にするため common 側で組立てる
    pub fn encryption_line(fingerprint: Option<&str>) -> String {
        match fingerprint {
            Some(fp) => format!(
                "通信は常時暗号化・相互認証されています(設定不要)・鍵の略号: {fp}(相手側の設定画面と同じ文字列なら正しく対になっています)"
            ),
            None => "通信は常時暗号化・相互認証されています(設定不要)".into(),
        }
    }

    fn invalid(e: impl std::fmt::Display) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, e.to_string())
    }

    fn write_rec(w: &mut TcpStream, data: &[u8]) -> io::Result<()> {
        let mut out = Vec::with_capacity(2 + data.len());
        out.extend_from_slice(&(data.len() as u16).to_be_bytes());
        out.extend_from_slice(data);
        w.write_all(&out)
    }

    fn read_rec(r: &mut TcpStream, buf: &mut Vec<u8>) -> io::Result<()> {
        let mut len = [0u8; 2];
        r.read_exact(&mut len)?;
        buf.resize(u16::from_be_bytes(len) as usize, 0);
        r.read_exact(buf)
    }

    /// 復号側(Read)。1 レコードずつ復号して返す
    pub struct Reader {
        s: TcpStream,
        st: Arc<snow::StatelessTransportState>,
        nonce: u64,
        cipher: Vec<u8>,
        plain: Vec<u8>,
        pos: usize,
    }

    impl Read for Reader {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            while self.pos >= self.plain.len() {
                read_rec(&mut self.s, &mut self.cipher)?;
                // 復号出力は暗号文長−タグ分に収まるため cipher 長だけで足りる。
                // MAX_MSG(64KB)のゼロフィルは毎レコードの無駄なメモリ書き込みになる
                self.plain.resize(self.cipher.len(), 0);
                let n = self
                    .st
                    .read_message(self.nonce, &self.cipher, &mut self.plain)
                    .map_err(invalid)?;
                self.nonce += 1;
                self.plain.truncate(n);
                self.pos = 0;
            }
            let n = out.len().min(self.plain.len() - self.pos);
            out[..n].copy_from_slice(&self.plain[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    /// 暗号化側(Write)。flush で溜めた平文をレコードにして送る(送り手は必ず flush する)
    pub struct Writer {
        s: TcpStream,
        st: Arc<snow::StatelessTransportState>,
        nonce: u64,
        buf: Vec<u8>,
        cipher: Vec<u8>,
    }

    impl Writer {
        fn emit(&mut self, n: usize) -> io::Result<()> {
            self.cipher.resize(n + TAG, 0);
            let len = self
                .st
                .write_message(self.nonce, &self.buf[..n], &mut self.cipher)
                .map_err(invalid)?;
            self.nonce += 1;
            write_rec(&mut self.s, &self.cipher[..len])?;
            self.buf.drain(..n);
            Ok(())
        }
        pub fn shutdown(&self) {
            let _ = self.s.shutdown(std::net::Shutdown::Both);
        }
        /// 送信が Link の slot ロックを握っている間でも即座に切断するための
        /// 複製ハンドル(shutdown は冪等なので二重に呼んでも安全)
        pub fn shutdown_handle(&self) -> io::Result<TcpStream> {
            self.s.try_clone()
        }
    }

    impl Reader {
        /// 受信タイムアウトの設定(相手の生存確認の周期に合わせる)
        pub fn set_read_timeout(&self, d: Option<std::time::Duration>) {
            let _ = self.s.set_read_timeout(d);
        }
    }

    impl Write for Writer {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            self.buf.extend_from_slice(data);
            while self.buf.len() >= MAX_PLAIN {
                self.emit(MAX_PLAIN)?;
            }
            Ok(data.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            if !self.buf.is_empty() {
                let n = self.buf.len();
                self.emit(n)?;
            }
            self.s.flush()
        }
    }

    fn split(s: TcpStream, hs: snow::HandshakeState) -> io::Result<(Reader, Writer)> {
        let st = Arc::new(hs.into_stateless_transport_mode().map_err(invalid)?);
        let w = s.try_clone()?;
        Ok((
            Reader {
                s,
                st: st.clone(),
                nonce: 0,
                cipher: Vec::new(),
                plain: Vec::new(),
                pos: 0,
            },
            Writer {
                s: w,
                st,
                nonce: 0,
                buf: Vec::new(),
                cipher: Vec::new(),
            },
        ))
    }

    fn builder<'a>(label: &'a [u8], key: &'a [u8; 32]) -> io::Result<snow::Builder<'a>> {
        snow::Builder::new(PATTERN.parse().map_err(invalid)?)
            .prologue(label)
            .map_err(invalid)?
            .psk(0, key)
            .map_err(invalid)
    }

    /// 接続した側(TCP クライアント)のハンドシェイク。label は経路ごとの識別子
    /// (本線の通信を音声経路へ差し込む等の取り違えを防ぐ)
    pub fn connect(mut s: TcpStream, token: &str, label: &[u8]) -> io::Result<(Reader, Writer)> {
        let key = psk(token);
        let mut hs = builder(label, &key)?.build_initiator().map_err(invalid)?;
        let mut buf = vec![0u8; MAX_MSG];
        let n = hs.write_message(&[], &mut buf).map_err(invalid)?;
        write_rec(&mut s, &buf[..n])?;
        let mut rec = Vec::new();
        read_rec(&mut s, &mut rec)?;
        hs.read_message(&rec, &mut buf).map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "handshake failed (token mismatch?)",
            )
        })?;
        split(s, hs)
    }

    /// 待ち受けた側(TCP サーバ)のハンドシェイク。トークンが違えばここで失敗する
    pub fn accept(mut s: TcpStream, token: &str, label: &[u8]) -> io::Result<(Reader, Writer)> {
        let key = psk(token);
        let mut hs = builder(label, &key)?.build_responder().map_err(invalid)?;
        let mut buf = vec![0u8; MAX_MSG];
        let mut rec = Vec::new();
        read_rec(&mut s, &mut rec)?;
        hs.read_message(&rec, &mut buf).map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "handshake failed (token mismatch?)",
            )
        })?;
        let n = hs.write_message(&[], &mut buf).map_err(invalid)?;
        write_rec(&mut s, &buf[..n])?;
        split(s, hs)
    }

    /// 待受ループの連続失敗スロットル。認証に失敗し続ける接続が高頻度で
    /// 来た時だけ受け付けを鈍らせ、正規の接続が成功すれば直ちに回復する
    /// (登録ポートの MAX_ATTEMPTS=3 と違い、確立済みの運用を締め出さない)
    pub struct FailThrottle {
        fails: u32,
    }
    impl Default for FailThrottle {
        fn default() -> Self {
            Self::new()
        }
    }

    impl FailThrottle {
        pub const fn new() -> Self {
            Self { fails: 0 }
        }
        /// 失敗を 1 つ数え、5 回目以降は失敗 1 回あたり 250ms ずつ
        /// (上限 5 秒)伸びる待ち時間を返す。呼び出し側で sleep する
        pub fn fail(&mut self) -> std::time::Duration {
            self.fails += 1;
            if self.fails < 5 {
                std::time::Duration::ZERO
            } else {
                std::time::Duration::from_millis(250 * (self.fails - 4).min(20) as u64)
            }
        }
        /// 成功で待ち時間を解除する
        pub fn success(&mut self) {
            self.fails = 0;
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// フィンガープリントは同じ鍵で同じ値・異なる鍵で異なる値。トークンの
        /// 本文を含まない(鍵を画面に出さない前提の確認手段だから)
        #[test]
        fn fingerprint_is_stable_distinct_and_hides_the_token() {
            let token = "7f".repeat(32);
            let a = fingerprint(&token);
            assert_eq!(a.len(), 8);
            assert!(a.chars().all(|c| c.is_ascii_uppercase_hexdigit()), "{a}");
            assert_eq!(a, fingerprint(&token), "同じ鍵は同じ値");
            assert_ne!(a, fingerprint(&"3a".repeat(32)), "異なる鍵は異なる値");
            assert!(!a.contains(token.as_str()), "トークン本文を含まない");
            // 大文字小文字は別の鍵として扱う(psk と同じ・正規化は保存時に済んでいる)
            assert_ne!(a, fingerprint(&token.to_ascii_uppercase()));
        }

        /// 暗号化の行の文言: フィンガープリントの有無で「鍵の略号:」の併記が変わるだけ
        #[test]
        fn encryption_line_appends_fingerprint_only_when_present() {
            let base = "通信は常時暗号化・相互認証されています(設定不要)";
            assert_eq!(encryption_line(None), base);
            assert_eq!(
                encryption_line(Some("AB12CD34")),
                format!("{base}・鍵の略号: AB12CD34(相手側の設定画面と同じ文字列なら正しく対になっています)")
            );
        }
    }

    /// 大文字の16進桁だけか(テスト用の小ヘルパー)。tests モジュールの外へ
    /// 出ていたため dead_code 警告が出ていた: 通常ビルドでは対象外とする
    #[cfg(test)]
    trait AsciiUppercaseHexdigit {
        fn is_ascii_uppercase_hexdigit(&self) -> bool;
    }
    #[cfg(test)]
    impl AsciiUppercaseHexdigit for char {
        fn is_ascii_uppercase_hexdigit(&self) -> bool {
            self.is_ascii_digit() || ('A'..='F').contains(self)
        }
    }
