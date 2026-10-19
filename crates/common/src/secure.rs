    //! 全経路(本線・音声・大容量)の暗号化と相互認証。
    //! Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s: 両者が同じトークンを知っている時だけ
    //! ハンドシェイクが成立し(トークン自体は回線に流れない)、接続ごとの使い捨て鍵で
    //! 暗号化する(前方秘匿性)。旧方式は平文 TCP にトークンを平文で載せ、
    //! 盗聴・改ざん耐性を Tailscale に全面依存していた
    use std::io::{self, IoSlice, Read, Write};
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

    fn invalid(e: impl std::fmt::Display) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, e.to_string())
    }

    fn write_rec(w: &mut TcpStream, data: &[u8]) -> io::Result<()> {
        let head = (data.len() as u16).to_be_bytes();
        write_all_vectored(&head, data, |slices| w.write_vectored(slices))
    }

    /// 2バイト長ヘッダと本体を `write_vectored` で順に書き切る。
    /// 部分書き込み(短書き)に対応し、0 が返ったらエラーにする。
    /// 書き込み関数を差し替えられる形にしてあるのは、短書きの分岐を
    /// テストで再現するため(レビューで境界のバグが見つかった)。
    fn write_all_vectored(
        head: &[u8],
        data: &[u8],
        mut write: impl FnMut(&mut [std::io::IoSlice<'_>]) -> io::Result<usize>,
    ) -> io::Result<()> {
        let mut head_off = 0usize;
        let mut data_off = 0usize;
        while head_off < head.len() || data_off < data.len() {
            let mut slices: [std::io::IoSlice<'_>; 2] = [
                IoSlice::new(&head[head_off..]),
                IoSlice::new(&data[data_off..]),
            ];
            // ヘッダを書き終えていたら**本体だけ**を渡す(以前は空になったヘッダ側を
            // 渡してしまい、0 が返って接続を切っていた)
            let n = if head_off >= head.len() {
                write(&mut slices[1..])?
            } else {
                write(&mut slices[..2])?
            };
            // 先頭スライス(ヘッダ)が先に消費される: n のうち min(残りヘッダ, n) はヘッダ
            let taken_head = (head.len() - head_off).min(n);
            head_off += taken_head;
            data_off += n - taken_head;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "write_vectored が 0 を返した",
                ));
            }
        }
        Ok(())
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

    /// 待受側の資格の束。従来の単一トークン運用も、台帳からの複数鍵運用も
    /// この型で表す。中身(鍵の並び・legacy の扱い)は非公開で、空では作れない
    /// (accept が「1本以上を試る」ことを型で保証する。鍵ゼロでの待受=
    /// 全拒否の見落としを防ぐ)
    #[derive(Clone)]
    pub struct ServerKeys {
        entries: Vec<(Who, [u8; 32])>,
    }

    impl ServerKeys {
        /// 単一トークン(従来運用・KNIT_TOKEN・Windows 側・テスト)
        pub fn single(token: &str) -> Self {
            Self {
                entries: vec![(Who::Legacy, psk(token))],
            }
        }

        /// 台帳運用(Mac)。legacy を受付中なら master の鍵を先頭に置き、
        /// 端末別トークンの鍵を加える。legacy 失効かつ登録0台は Err
        /// (待ち受けられる鍵が無い=待受を始められない)
        pub fn legacy_plus(
            master: &str,
            legacy_accepts: bool,
            devices: &[(DeviceKeyId, String)],
        ) -> io::Result<Self> {
            let mut entries = Vec::new();
            if legacy_accepts {
                entries.push((Who::Legacy, psk(master)));
            }
            for (id, token) in devices {
                entries.push((Who::Device(id.clone()), psk(token)));
            }
            if entries.is_empty() {
                return Err(io::Error::other(
                    "待ち受けできる鍵がありません(旧形式を閉じた状態で登録済み端末もありません)",
                ));
            }
            Ok(Self { entries })
        }

        /// 束の鍵数(テストと監視用)
        pub fn len(&self) -> usize {
            self.entries.len()
        }
    }

    /// 待ち受けた側(TCP サーバ)のハンドシェイク。単一トークン向けの簡易形
    /// (認証結果は暗黙に Legacy)。複数鍵と認証結果が必要な側は accept_as
    pub fn accept(s: TcpStream, token: &str, label: &[u8]) -> io::Result<(Reader, Writer)> {
        let (r, w, _) = accept_as(s, &ServerKeys::single(token), label)?;
        Ok((r, w))
    }

    /// 待ち受けた側のハンドシェイク(複数鍵)。最初の Noise レコードを1回だけ
    /// 読み、束の鍵ごとに responder を作り直して試す(TCP 往復は単一鍵の現行と
    /// 同一で、クライアント側からは区別できない)。どの鍵で通ったかを Who で
    /// 返す。全滅なら現行どおり PermissionDenied。線形探索(先頭=legacy から)の
    /// ため「どの鍵で当たったか」が処理時間に漏れる素地はある(設計書の弱点として
    /// 明記。cap と FailThrottle で軽減し、除去はしない)
    pub fn accept_as(
        mut s: TcpStream,
        keys: &ServerKeys,
        label: &[u8],
    ) -> io::Result<(Reader, Writer, Who)> {
        let mut rec = Vec::new();
        read_rec(&mut s, &mut rec)?;
        let mut buf = vec![0u8; MAX_MSG];
        for (who, key) in &keys.entries {
            let mut cand = builder(label, key)?.build_responder().map_err(invalid)?;
            // 失敗した responder は捨てて次の鍵で作り直す(read_message は
            // レコードを借用渡しで読むため、同じレコードを再試行できる)
            if cand.read_message(&rec, &mut buf).is_ok() {
                let n = cand.write_message(&[], &mut buf).map_err(invalid)?;
                write_rec(&mut s, &buf[..n])?;
                let (r, w) = split(s, cand)?;
                return Ok((r, w, who.clone()));
            }
        }
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "handshake failed (token mismatch?)",
        ))
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

    /// 登録台帳の1端末を指す鍵ID(OS 乱数由来のhex32)。newtype であり
    /// `parse` 以外の経路では作らせない(境界で検証し、内部では信頼する)。
    /// 台帳ファイルへの保存・復元もこの形だけで行う
    #[derive(Clone, PartialEq, Eq, Debug, Hash, serde::Serialize, serde::Deserialize)]
    pub struct DeviceKeyId(String);

    impl DeviceKeyId {
        /// hex32(16バイトの16進)だけを通す。台帳の壊れた行はこれで弾く。
        /// 大文字は小文字へ正規化する
        pub fn parse(s: &str) -> Option<Self> {
            if s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
                Some(Self(s.to_ascii_lowercase()))
            } else {
                None
            }
        }
        pub fn as_str(&self) -> &str {
            &self.0
        }
        /// 新規採番(OS の安全な乱数から128bit)。登録ごとに新規で、使い回ししない
        pub fn generate() -> io::Result<Self> {
            let full = crate::credentials::generate()?;
            Ok(Self(full[..32].to_string()))
        }
    }

    impl std::fmt::Display for DeviceKeyId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }

    /// accept が「どの鍵で認証したか」を返す。Legacy は従来の単一共有トークン
    /// (KNIT_TOKEN 運用・移行中の旧端末)。Device は端末別導出鍵。呼び出し側は
    /// これを PeerEntry 等へ運ぶ(認証済み主体。hello の id は表示名の紐付け用で
    /// 認証の根拠にはしない)
    #[derive(Clone, PartialEq, Eq, Debug)]
    pub enum Who {
        Legacy,
        Device(DeviceKeyId),
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// レビュー指摘: 短書き(部分書き込み)でもヘッダ+本体が順に書き切られること。
        /// 1バイトずつしか書けない書き込み関数で、ヘッダを書き終えた後に本体が
        /// 残る状況を作る(以前は空のヘッダ側を渡して 0 を返し、接続を切っていた)
        #[test]
        fn vectored_write_survives_partial_writes() {
            let (head, data) = (b"\x00\x05", b"hello");
            let mut seen: Vec<u8> = Vec::new();
            write_all_vectored(head, data, |slices| {
                let mut wrote = 0usize;
                for s in slices.iter() {
                    if let Some(b) = s.first() {
                        seen.push(*b);
                        wrote += 1;
                        break;
                    }
                }
                Ok(wrote)
            })
            .unwrap();
            assert_eq!(seen, b"\x00\x05hello", "ヘッダも本体も順に書き切る");

            // 0 を返す書き込みはエラー(無限ループにしない)
            let err = write_all_vectored(b"\x00\x01", b"x", |_| Ok(0)).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::WriteZero);
        }

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

        // ── 複数鍵の待受(端末別認証)のループバックテスト ──
        use crate::registry::device_token;

        fn dev_key(n: u8) -> DeviceKeyId {
            DeviceKeyId::parse(&format!("{:032x}", n as u32 * 0x01010101)).unwrap()
        }

        /// 1組の accept_as×connect を試し、accept 側の認証結果(Who)を返す。
        /// connect 側の成否は accept 側と一致する(失敗時はどちらも Err)
        fn who_among(keys: &ServerKeys, token: &str) -> io::Result<Who> {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = l.local_addr().unwrap();
            let keys = keys.clone();
            let server = std::thread::spawn(move || {
                let (stream, _) = l.accept().unwrap();
                accept_as(stream, &keys, b"knit-main").map(|(_, _, who)| who)
            });
            let client = std::net::TcpStream::connect(addr).unwrap();
            let _ = connect(client, token, b"knit-main");
            server.join().unwrap()
        }

        fn bundle(legacy: bool, devs: &[u8]) -> io::Result<ServerKeys> {
            let master = "5f".repeat(32);
            let devices: Vec<(DeviceKeyId, String)> = devs
                .iter()
                .map(|n| {
                    let id = dev_key(*n);
                    let tok = device_token(&master, &id);
                    (id, tok)
                })
                .collect();
            ServerKeys::legacy_plus(&master, legacy, &devices)
        }

        fn token_of(n: u8) -> String {
            device_token(&"5f".repeat(32), &dev_key(n))
        }

        /// 束のどの鍵で接続しても、その鍵の Who が返る(legacy・端末別の混在)
        #[test]
        fn multi_key_accept_tells_who_connected() {
            let keys = bundle(true, &[1, 2]).unwrap();
            assert_eq!(keys.len(), 3, "legacy + 2台");
            assert_eq!(who_among(&keys, &"5f".repeat(32)).unwrap(), Who::Legacy);
            assert_eq!(who_among(&keys, &token_of(1)).unwrap(), Who::Device(dev_key(1)));
            assert_eq!(who_among(&keys, &token_of(2)).unwrap(), Who::Device(dev_key(2)));
        }

        /// 束に無いトークンは従来どおり PermissionDenied(成否以外の差は出ない)
        #[test]
        fn key_miss_is_permission_denied_for_every_key() {
            let keys = bundle(true, &[1, 2]).unwrap();
            let err = who_among(&keys, &"aa".repeat(32)).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        }

        /// 鍵が1本も無い束は作れない(待受開始時の全拒否見落としを防ぐ)
        #[test]
        fn empty_server_keys_are_rejected_at_construction() {
            assert!(bundle(false, &[]).is_err());
            // legacy だけ・端末だけは作れる
            assert!(bundle(true, &[]).is_ok());
            assert!(bundle(false, &[1]).is_ok());
        }

        /// 従来形 accept(単一トークン)は Legacy として認識される
        #[test]
        fn single_key_accept_keeps_legacy_identity() {
            let keys = ServerKeys::single(&"7e".repeat(32));
            assert_eq!(who_among(&keys, &"7e".repeat(32)).unwrap(), Who::Legacy);
        }

        /// 経路ラベルの取り違えは複数鍵でも弾かれる
        #[test]
        fn label_mismatch_fails_multi_key_too() {
            let keys = bundle(true, &[1]).unwrap();
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = l.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (stream, _) = l.accept().unwrap();
                accept_as(stream, &keys, b"knit-main")
            });
            let client = std::net::TcpStream::connect(addr).unwrap();
            let wrong_label = connect(client, &token_of(1), b"knit-audio");
            assert!(wrong_label.is_err(), "接続側も失敗する");
            assert!(server.join().unwrap().is_err(), "accept 側も失敗する");
        }

        /// 失効(束からの取り下げ)は他端末に影響しない(AC2 の secure 層の核)
        #[test]
        fn removed_key_is_denied_while_others_stay() {
            let with_both = bundle(true, &[1, 2]).unwrap();
            assert!(who_among(&with_both, &token_of(1)).is_ok());
            let without_1 = bundle(true, &[2]).unwrap();
            assert!(who_among(&without_1, &token_of(1)).is_err(), "取り下げた鍵は拒否");
            assert!(who_among(&without_1, &token_of(2)).is_ok(), "他端末は無影響");
            assert_eq!(who_among(&without_1, &"5f".repeat(32)).unwrap(), Who::Legacy, "legacy も無影響");
        }

        /// 鍵数上限(64)でも最後尾の鍵で接続が完結する(線形探索の回帰監視。
        /// しきい値は debug ビルドを見込んだ甘めの上限で、比例関係は主張しない)
        #[test]
        fn accept_stays_fast_at_the_key_cap() {
            let devs: Vec<u8> = (0..crate::registry::MAX_DEVICES as u8 - 1).collect();
            let keys = bundle(true, &devs).unwrap();
            assert_eq!(keys.len(), crate::registry::MAX_DEVICES);
            let start = std::time::Instant::now();
            let who = who_among(&keys, &token_of(devs[devs.len() - 1])).unwrap();
            assert_eq!(who, Who::Device(dev_key(devs[devs.len() - 1])));
            assert!(start.elapsed() < std::time::Duration::from_secs(2), "上限鍵数でも即座に完了する");
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
