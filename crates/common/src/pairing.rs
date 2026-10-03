//! Explicit, short-lived enrollment. Discovery is only a hint; SPAKE2 and Noise
//! authenticate the code before any persistent credential is transferred.
use crate::credentials;
use blake2::{Blake2s256, Digest};
use curve25519_dalek::{constants::X25519_BASEPOINT, MontgomeryPoint};
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::{
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const PORT: u16 = 24904;
pub const LIFETIME: Duration = Duration::from_secs(300);
const MAX_ATTEMPTS: usize = 3;
const IO_TIMEOUT: Duration = Duration::from_secs(8);
const ASK: &[u8] = b"KNIT-PAIR-DISCOVER-1";
const LABEL: &[u8] = b"knit-enrollment-v1";
/// 承認方式(6桁を打たずに、両方の画面の確認番号を見比べて許可する)の開始を示す印
const SAS_MARK: &[u8] = b"KNIT-SAS1";
/// 承認方式で、人が確認番号を見比べて押すまで待つ時間(登録の期限を超えない)
const APPROVAL_WINDOW: Duration = Duration::from_secs(120);
/// 持ち主へ「許可しますか」を尋ねる関数(名前・確認番号・相手のアドレス・相手がまだいるかの確認)
type AskFn = dyn Fn(String, String, SocketAddr, &dyn Fn() -> bool) -> io::Result<bool>;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Candidate {
    pub name: String,
    pub address: SocketAddr,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Beacon {
    version: u8,
    name: String,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Credential {
    token: String,
    port: u16,
}

/// 相手から「つなぎたい」と求められた。確認番号が相手の画面と同じなら `approve`、心当たりがなければ `deny`。
/// 一定時間応答が無ければ拒否として扱う。
pub struct Approval {
    pub name: String,
    /// 両方の画面に出る4桁の確認番号
    pub sas: String,
    pub peer: SocketAddr,
    decision: mpsc::Sender<bool>,
}
impl Approval {
    pub fn approve(&self) {
        let _ = self.decision.send(true);
    }
    pub fn deny(&self) {
        let _ = self.decision.send(false);
    }
}
impl std::fmt::Debug for Approval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 確認番号はログに出さない
        write!(f, "Approval({} from {})", self.name, self.peer)
    }
}
impl PartialEq for Approval {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.sas == other.sas && self.peer == other.peer
    }
}

#[derive(Debug, PartialEq)]
pub enum Event {
    Approval(Approval),
    /// 確認待ちの間に、依頼した側が接続を切った(確認画面を取り下げる)
    Withdrawn,
    AttemptFailed(usize),
    Registered(SocketAddr),
    Expired,
    Locked,
}

pub fn parse_code(input: &str) -> io::Result<String> {
    // Separators are permitted for the displayed form "123 456", never other characters.
    let code: String = input
        .trim()
        .chars()
        .filter(|c| *c != ' ' && *c != '-')
        .collect();
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Macに表示された6桁の数字を入力してください。",
        ));
    }
    Ok(code)
}
fn new_code() -> io::Result<String> {
    loop {
        let entropy = credentials::generate()?;
        let n = u32::from_str_radix(&entropy[..8], 16).map_err(invalid)?;
        if n < 4_294_000_000 {
            return Ok(format!("{:06}", n % 1_000_000));
        }
    }
}
fn invalid(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}
#[cfg(test)]
fn frame_read(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len = [0; 2];
    r.read_exact(&mut len)?;
    let len = u16::from_be_bytes(len) as usize;
    if len == 0 || len > 1024 {
        return Err(invalid("invalid enrollment frame length"));
    }
    let mut bytes = vec![0; len];
    r.read_exact(&mut bytes)?;
    Ok(bytes)
}
#[cfg(test)]
fn configure(s: &TcpStream, timeout: Duration) -> io::Result<()> {
    s.set_nonblocking(false)?;
    s.set_nodelay(true)?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))
}
fn check_active(stop: &AtomicBool, until: Instant) -> io::Result<()> {
    if stop.load(Ordering::SeqCst) || Instant::now() >= until {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "invitation closed",
        ));
    }
    Ok(())
}

/// Dropping the invitation immediately cancels its sockets and joins its workers.
/// The code is intentionally not Debug/Serialize and never included in discovery.
pub struct Invitation {
    pub code: String,
    pub events: mpsc::Receiver<Event>,
    stop: Arc<AtomicBool>,
    active: Arc<Mutex<Option<TcpStream>>>,
    workers: Vec<JoinHandle<()>>,
}
impl Invitation {
    pub fn open(token: String, service_port: u16) -> io::Result<Self> {
        Self::bind(
            "0.0.0.0:24904".parse().unwrap(),
            token,
            service_port,
            LIFETIME,
        )
    }
    fn bind(
        address: SocketAddr,
        token: String,
        service_port: u16,
        lifetime: Duration,
    ) -> io::Result<Self> {
        // Legacy env tokens may have other lengths. Do not silently normalize them.
        credentials::validate_transport_key(&token)?;
        if service_port == 0 {
            return Err(invalid("invalid credential"));
        }
        let tcp = TcpListener::bind(address)?;
        let udp = UdpSocket::bind(tcp.local_addr()?)?;
        tcp.set_nonblocking(true)?;
        udp.set_read_timeout(Some(Duration::from_millis(100)))?;
        let code = new_code()?;
        let invitation_id = credentials::generate()?;
        let stop = Arc::new(AtomicBool::new(false));
        let active = Arc::new(Mutex::new(None));
        let until = Instant::now() + lifetime;
        let (tx, events) = mpsc::channel();
        // ビーコンは LAN 上の全員から見える。端末名(人名・社名を含むことがある)を
        // 載せず、固定の匿名ラベルで応答する。登録の突合は4桁の確認番号と
        // 送信元アドレスで行うため、名前は登録に必要ない
        let name = beacon_name();
        let beacon = serde_json::to_vec(&Beacon {
            version: 1,
            name: safe_name(&name),
        })
        .map_err(invalid)?;
        let discovery_stop = stop.clone();
        let discovery = thread::spawn(move || {
            let mut data = [0; 128];
            let mut last = Instant::now() - Duration::from_secs(1);
            while !discovery_stop.load(Ordering::SeqCst) && Instant::now() < until {
                if let Ok((n, from)) = udp.recv_from(&mut data) {
                    if &data[..n] == ASK
                        && crate::net::is_allowed(from.ip())
                        && last.elapsed() >= Duration::from_millis(30)
                    {
                        let _ = udp.send_to(&beacon, from);
                        last = Instant::now();
                    }
                }
            }
        });
        let worker_stop = stop.clone();
        let worker_active = active.clone();
        let worker_code = code.clone();
        let server_name = safe_name(&name);
        let worker = thread::spawn(move || {
            let mut attempts = 0;
            while check_active(&worker_stop, until).is_ok() {
                match tcp.accept() {
                    Ok((s, peer)) if crate::net::is_allowed(peer.ip()) => {
                        attempts += 1;
                        let timeout = until
                            .saturating_duration_since(Instant::now())
                            .min(IO_TIMEOUT);
                        if timeout.is_zero() {
                            break;
                        }
                        let attempt_until = Instant::now() + timeout;
                        *worker_active.lock().unwrap_or_else(|e| e.into_inner()) =
                            s.try_clone().ok();
                        // 承認方式: 画面の持ち主に「許可しますか」を尋ね、答えを待つ
                        let ask_tx = tx.clone();
                        let ask_stop = worker_stop.clone();
                        let ask = move |name: String,
                                        sas: String,
                                        from: SocketAddr,
                                        alive: &dyn Fn() -> bool|
                              -> io::Result<bool> {
                            let (decision, answer) = mpsc::channel();
                            ask_tx
                                .send(Event::Approval(Approval { name, sas, peer: from, decision }))
                                .map_err(|_| invalid("no approver"))?;
                            let wait_until = Instant::now() + APPROVAL_WINDOW;
                            while Instant::now() < wait_until && !ask_stop.load(Ordering::SeqCst) {
                                match answer.recv_timeout(Duration::from_millis(100)) {
                                    Ok(v) => return Ok(v),
                                    Err(mpsc::RecvTimeoutError::Timeout) => {
                                        // 依頼した側が去ったら、確認画面を取り下げて占有を解く
                                        if !alive() {
                                            let _ = ask_tx.send(Event::Withdrawn);
                                            return Ok(false);
                                        }
                                    }
                                    Err(_) => return Ok(false),
                                }
                            }
                            Ok(false)
                        };
                        let result = serve_one(
                            s,
                            peer,
                            &worker_code,
                            &invitation_id,
                            &token,
                            service_port,
                            &worker_stop,
                            attempt_until,
                            until,
                            &server_name,
                            &ask,
                        );
                        worker_active
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .take();
                        if result.is_ok() {
                            let _ = tx.send(Event::Registered(peer));
                            break;
                        }
                        if worker_stop.load(Ordering::SeqCst) {
                            break;
                        }
                        if attempts >= MAX_ATTEMPTS {
                            let _ = tx.send(Event::Locked);
                            break;
                        }
                        let _ = tx.send(Event::AttemptFailed(MAX_ATTEMPTS - attempts));
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(40))
                    }
                    Err(_) => break,
                }
            }
            if Instant::now() >= until {
                let _ = tx.send(Event::Expired);
            }
            worker_stop.store(true, Ordering::SeqCst);
        });
        Ok(Self {
            code,
            events,
            stop,
            active,
            workers: vec![discovery, worker],
        })
    }
}
impl Drop for Invitation {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(s) = self.active.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}
#[allow(clippy::too_many_arguments)] // 登録の1接続に必要な状態をまとめて渡す
fn serve_one(
    s: TcpStream,
    peer: SocketAddr,
    code: &str,
    id: &str,
    token: &str,
    port: u16,
    stop: &AtomicBool,
    until: Instant,
    session_until: Instant,
    server_name: &str,
    ask: &AskFn,
) -> io::Result<()> {
    let mut wire = Wire::new(s, stop, until)?;
    wire.send(id.as_bytes())?;
    let incoming = wire.receive()?;
    if incoming.starts_with(SAS_MARK) {
        return serve_sas(wire, &incoming, peer, id, token, port, stop, session_until, server_name, ask);
    }
    let (state, message) = Spake2::<Ed25519Group>::start_b(
        &Password::new(code.as_bytes()),
        &Identity::new(b"knit-windows-v1"),
        &Identity::new(id.as_bytes()),
    );
    wire.send(&message)?;
    let key = state
        .finish(&incoming)
        .map_err(|_| invalid("invalid PAKE message"))?;
    let mut channel = Channel::handshake(wire, &key, false)?;
    serve_credential(&mut channel, token, port, stop, until)
}

/// 認証済みの暗号化経路で長期キーを渡す(6桁方式と承認方式で共通)。
fn serve_credential(
    channel: &mut Channel,
    token: &str,
    port: u16,
    stop: &AtomicBool,
    until: Instant,
) -> io::Result<()> {
    // Authenticated transport proves possession of the PAKE key on both sides.
    if channel.receive()? != b"ready" {
        return Err(invalid("invalid confirmation"));
    }
    check_active(stop, until)?;
    channel.send(
        &serde_json::to_vec(&Credential {
            token: token.into(),
            port,
        })
        .map_err(invalid)?,
    )?;
    if channel.receive()? != b"saved" {
        return Err(invalid("credential not saved"));
    }
    check_active(stop, until)?;
    channel.send(b"complete")
}

fn random32() -> io::Result<[u8; 32]> {
    // OS の乱数(Android を含む全 OS)。取得できなければ失敗し、弱い乱数へは落とさない
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| invalid("entropy unavailable"))?;
    Ok(bytes)
}
/// 確認番号を選ぶ画面用の候補。本物の番号と、無関係な3つの番号を混ぜて並べ替える。
/// 相手の画面を見ずに押しても、当たるのは1/4になる。
pub fn number_choices(sas: &str) -> io::Result<Vec<String>> {
    let mut list = vec![sas.to_string()];
    while list.len() < 4 {
        let bytes = random32()?;
        for pair in bytes.chunks(2) {
            let n = format!("{:04}", u16::from_be_bytes([pair[0], pair[1]]) % 10_000);
            if list.len() < 4 && !list.contains(&n) {
                list.push(n);
            }
        }
    }
    // Fisher-Yates(乱数を都度取り直す)
    for i in (1..list.len()).rev() {
        let j = (random32()?[0] as usize) % (i + 1);
        list.swap(i, j);
    }
    Ok(list)
}
fn public_of(secret: &[u8; 32]) -> [u8; 32] {
    X25519_BASEPOINT.mul_clamped(*secret).to_bytes()
}
fn shared_secret(secret: &[u8; 32], peer_public: &[u8; 32]) -> io::Result<[u8; 32]> {
    let shared = MontgomeryPoint(*peer_public).mul_clamped(*secret).to_bytes();
    // 位数の小さい点(全ゼロの共有値)は、相手が鍵交換を無効にしようとしている
    if shared == [0; 32] {
        return Err(invalid("weak key exchange"));
    }
    Ok(shared)
}
/// 先に鍵を約束する(コミット)。確認番号を都合のよい値に合わせるための鍵の総当たりを防ぐ。
fn commitment(public: &[u8; 32], nonce: &[u8; 16]) -> [u8; 32] {
    let mut h = Blake2s256::new();
    h.update(b"knit-sas-commit-v1");
    h.update(public);
    h.update(nonce);
    h.finalize().into()
}
/// 両方の公開値から、画面に出す4桁の確認番号と、以降の暗号化に使う鍵を導く。
fn sas_and_key(
    id: &[u8],
    a: (&[u8; 32], &[u8; 16]),
    b: (&[u8; 32], &[u8; 16]),
    shared: &[u8; 32],
) -> (String, [u8; 32]) {
    let mut h = Blake2s256::new();
    h.update(b"knit-sas-v1");
    h.update((id.len() as u16).to_be_bytes());
    h.update(id);
    h.update(a.0);
    h.update(a.1);
    h.update(b.0);
    h.update(b.1);
    let transcript: [u8; 32] = h.finalize().into();
    let n = u32::from_be_bytes([transcript[0], transcript[1], transcript[2], transcript[3]]);
    let mut k = Blake2s256::new();
    k.update(b"knit-sas-key-v1");
    k.update(shared);
    k.update(transcript);
    (format!("{:04}", n % 10_000), k.finalize().into())
}

/// 承認方式の受け側(Mac)。相手の名前と確認番号を持ち主に見せ、許可された時だけ長期キーを渡す。
#[allow(clippy::too_many_arguments)]
fn serve_sas(
    mut wire: Wire,
    first: &[u8],
    peer: SocketAddr,
    id: &str,
    token: &str,
    port: u16,
    stop: &AtomicBool,
    session_until: Instant,
    server_name: &str,
    ask: &AskFn,
) -> io::Result<()> {
    // 約束の照合が済むまでは、通常の短い期限のまま(黙って居座る接続に待ち受けを塞がせない)
    let body = &first[SAS_MARK.len()..];
    if body.len() < 32 || body.len() > 32 + 64 {
        return Err(invalid("invalid approval request"));
    }
    let commit: [u8; 32] = body[..32].try_into().unwrap();
    let client_name = safe_name(&String::from_utf8_lossy(&body[32..]));
    let secret = random32()?;
    let public = public_of(&secret);
    let nonce_b: [u8; 16] = random32()?[..16].try_into().unwrap();
    let mut msg2 = Vec::with_capacity(48 + server_name.len());
    msg2.extend_from_slice(&public);
    msg2.extend_from_slice(&nonce_b);
    msg2.extend_from_slice(clamp_name_bytes(server_name, 64).as_bytes());
    wire.send(&msg2)?;
    let msg3 = wire.receive()?;
    if msg3.len() != 48 {
        return Err(invalid("invalid approval reveal"));
    }
    let pk_a: [u8; 32] = msg3[..32].try_into().unwrap();
    let nonce_a: [u8; 16] = msg3[32..].try_into().unwrap();
    if commitment(&pk_a, &nonce_a) != commit {
        return Err(invalid("commitment mismatch"));
    }
    let shared = shared_secret(&secret, &pk_a)?;
    let (sas, key) = sas_and_key(id.as_bytes(), (&pk_a, &nonce_a), (&public, &nonce_b), &shared);
    // ここから先は、人が判断する時間を見込んで期限を延ばす(登録全体の期限は超えない)
    wire.until = (Instant::now() + APPROVAL_WINDOW + Duration::from_secs(10)).min(session_until);
    let probe = wire.socket.try_clone()?;
    let alive = move || {
        let mut byte = [0u8; 1];
        match probe.peek(&mut byte) {
            Ok(0) => false,
            Ok(_) => true,
            Err(e) => e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::Interrupted,
        }
    };
    if !ask(client_name, sas, peer, &alive)? {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "not approved"));
    }
    check_active(stop, session_until)?;
    let mut channel = Channel::handshake(wire, &key, false)?;
    serve_credential(&mut channel, token, port, stop, session_until)
}

/// 承認方式の依頼側(Windows・Android)が、確認番号を出した状態で待っている接続。
/// 持ち主が番号を見比べて `confirm` を呼ぶと登録が完了する。捨てる(drop)と取り消す。
pub struct Pending {
    socket: TcpStream,
    cancelled: Arc<AtomicBool>,
    address: SocketAddr,
    key: [u8; 32],
    /// 両方の画面に出る4桁の確認番号
    pub sas: String,
    /// 相手(Mac)の名前
    pub server_name: String,
}
impl Pending {
    /// 相手(Mac)のアドレス。画面に出して、心当たりのある相手か確かめてもらう
    pub fn peer(&self) -> SocketAddr {
        self.address
    }
    /// 番号が一致すると持ち主が確認した。Mac 側の許可も得られた時に、長期キーを受け取って保存する。
    pub fn confirm(
        self,
        preserve_existing: bool,
        save: impl FnOnce(&str, SocketAddr) -> io::Result<()>,
    ) -> io::Result<String> {
        let Pending { socket, cancelled, address, key, .. } = self;
        let until = Instant::now() + APPROVAL_WINDOW + Duration::from_secs(10);
        let wire = Wire::new(socket, &cancelled, until)?;
        let mut channel = Channel::handshake(wire, &key, true)?;
        client_credential(&mut channel, address, preserve_existing, &cancelled, until, save)
    }
}

/// 承認方式を依頼する。相手が見つかったら自動で呼んでよい(持ち主が確認するまで何も保存されない)。
pub fn request_approval(
    address: SocketAddr,
    my_name: &str,
    cancelled: Arc<AtomicBool>,
) -> io::Result<Pending> {
    check_active(&cancelled, Instant::now() + IO_TIMEOUT)?;
    let s = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
    let until = Instant::now() + Duration::from_secs(20);
    let mut wire = Wire::new(s, &cancelled, until)?;
    let id = wire.receive()?;
    if id.len() != 64 || !id.iter().all(u8::is_ascii_hexdigit) {
        return Err(invalid("unknown invitation"));
    }
    let secret = random32()?;
    let public = public_of(&secret);
    let nonce_a: [u8; 16] = random32()?[..16].try_into().unwrap();
    let name = safe_name(my_name);
    let mut msg1 = Vec::from(SAS_MARK);
    msg1.extend_from_slice(&commitment(&public, &nonce_a));
    msg1.extend_from_slice(clamp_name_bytes(&name, 64).as_bytes());
    wire.send(&msg1)?;
    let msg2 = wire.receive()?;
    if msg2.len() < 48 || msg2.len() > 48 + 64 {
        return Err(invalid("invalid approval response"));
    }
    let pk_b: [u8; 32] = msg2[..32].try_into().unwrap();
    let nonce_b: [u8; 16] = msg2[32..48].try_into().unwrap();
    let server_name = safe_name(&String::from_utf8_lossy(&msg2[48..]));
    let mut msg3 = Vec::with_capacity(48);
    msg3.extend_from_slice(&public);
    msg3.extend_from_slice(&nonce_a);
    wire.send(&msg3)?;
    let shared = shared_secret(&secret, &pk_b)?;
    let (sas, key) = sas_and_key(&id, (&public, &nonce_a), (&pk_b, &nonce_b), &shared);
    let socket = wire.socket;
    Ok(Pending { socket, cancelled, address, key, sas, server_name })
}

/// The caller persists only after authenticated decryption. A save failure never
/// emits a success acknowledgement. The returned token is never the short code.
pub fn enroll(
    address: SocketAddr,
    code: &str,
    save: impl FnOnce(&str, SocketAddr) -> io::Result<()>,
) -> io::Result<String> {
    enroll_cancellable(address, code, Arc::new(AtomicBool::new(false)), save)
}

pub fn enroll_cancellable(
    address: SocketAddr,
    code: &str,
    cancelled: Arc<AtomicBool>,
    save: impl FnOnce(&str, SocketAddr) -> io::Result<()>,
) -> io::Result<String> {
    enroll_impl(address, code, cancelled, false, save)
}

/// Android's Keystore-backed store can preserve an existing env key exactly.
/// The same PAKE, Noise, expiration, attempt limit and save acknowledgement apply.
pub fn enroll_existing_key(
    address: SocketAddr,
    code: &str,
    save: impl FnOnce(&str, SocketAddr) -> io::Result<()>,
) -> io::Result<String> {
    enroll_impl(address, code, Arc::new(AtomicBool::new(false)), true, save)
}

fn enroll_impl(
    address: SocketAddr,
    code: &str,
    cancelled: Arc<AtomicBool>,
    preserve_existing: bool,
    save: impl FnOnce(&str, SocketAddr) -> io::Result<()>,
) -> io::Result<String> {
    let code = parse_code(code)?;
    check_active(&cancelled, Instant::now() + IO_TIMEOUT)?;
    let s = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
    let until = Instant::now() + Duration::from_secs(20);
    let mut wire = Wire::new(s, &cancelled, until)?;
    let id = wire.receive()?;
    if id.len() != 64 || !id.iter().all(u8::is_ascii_hexdigit) {
        return Err(invalid("unknown invitation"));
    }
    let (state, message) = Spake2::<Ed25519Group>::start_a(
        &Password::new(code.as_bytes()),
        &Identity::new(b"knit-windows-v1"),
        &Identity::new(&id),
    );
    wire.send(&message)?;
    let key = state
        .finish(&wire.receive()?)
        .map_err(|_| invalid("invalid PAKE message"))?;
    let mut channel = Channel::handshake(wire, &key, true)?;
    client_credential(&mut channel, address, preserve_existing, &cancelled, until, save)
}
/// 認証済みの暗号化経路で長期キーを受け取って保存する(6桁方式と承認方式で共通)。
fn client_credential(
    channel: &mut Channel,
    address: SocketAddr,
    preserve_existing: bool,
    cancelled: &AtomicBool,
    until: Instant,
    save: impl FnOnce(&str, SocketAddr) -> io::Result<()>,
) -> io::Result<String> {
    channel.send(b"ready")?;
    let credential: Credential = serde_json::from_slice(&channel.receive()?).map_err(invalid)?;
    let token = if preserve_existing {
        credentials::validate_transport_key(&credential.token)?;
        credential.token.clone()
    } else {
        credentials::parse_key(&credential.token)?
    };
    if token != credential.token || credential.port == 0 {
        return Err(invalid("unsupported credential"));
    }
    check_active(cancelled, until)?;
    save(&token, SocketAddr::new(address.ip(), credential.port))?;
    // Persistence succeeded: a lost final acknowledgement must not invite an
    // overwrite on retry. The normal authenticated connection completes recovery.
    let _ = channel.send(b"saved");
    let _ = channel.receive();
    Ok(token)
}
/// Nonblocking records with an absolute deadline. On Windows, shutdown of a
/// duplicate socket need not abort an already pending blocking recv. Checking
/// cancellation between nonblocking reads avoids relying on that OS behavior.
struct Wire<'a> {
    socket: TcpStream,
    cancelled: &'a AtomicBool,
    until: Instant,
}
impl<'a> Wire<'a> {
    fn new(socket: TcpStream, cancelled: &'a AtomicBool, until: Instant) -> io::Result<Self> {
        socket.set_nonblocking(true)?;
        socket.set_nodelay(true)?;
        Ok(Self {
            socket,
            cancelled,
            until,
        })
    }
    fn read_exact(&mut self, data: &mut [u8]) -> io::Result<()> {
        let mut offset = 0;
        while offset < data.len() {
            check_active(self.cancelled, self.until)?;
            match self.socket.read(&mut data[offset..]) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "enrollment peer closed",
                    ))
                }
                Ok(n) => offset += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn send(&mut self, data: &[u8]) -> io::Result<()> {
        if data.is_empty() || data.len() > 1024 {
            return Err(invalid("invalid enrollment frame length"));
        }
        let mut bytes = Vec::with_capacity(2 + data.len());
        bytes.extend_from_slice(&(data.len() as u16).to_be_bytes());
        bytes.extend_from_slice(data);
        let mut offset = 0;
        while offset < bytes.len() {
            check_active(self.cancelled, self.until)?;
            match self.socket.write(&bytes[offset..]) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "enrollment peer closed",
                    ))
                }
                Ok(n) => offset += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn receive(&mut self) -> io::Result<Vec<u8>> {
        let mut length = [0; 2];
        self.read_exact(&mut length)?;
        let length = u16::from_be_bytes(length) as usize;
        if length == 0 || length > 1024 {
            return Err(invalid("invalid enrollment frame length"));
        }
        let mut bytes = vec![0; length];
        self.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}
/// Same Noise pattern/library as the normal connection, with a PAKE-derived
/// 256-bit PSK, a distinct prologue and small deadline-aware enrollment records.
struct Channel<'a> {
    wire: Wire<'a>,
    state: snow::TransportState,
}
impl<'a> Channel<'a> {
    fn handshake(mut wire: Wire<'a>, key: &[u8], initiator: bool) -> io::Result<Self> {
        let key: &[u8; 32] = key.try_into().map_err(invalid)?;
        let builder = snow::Builder::new(
            "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s"
                .parse()
                .map_err(invalid)?,
        )
        .prologue(LABEL)
        .map_err(invalid)?
        .psk(0, key)
        .map_err(invalid)?;
        let mut state = if initiator {
            builder.build_initiator()
        } else {
            builder.build_responder()
        }
        .map_err(invalid)?;
        let mut buffer = [0; 1024];
        if initiator {
            let size = state.write_message(&[], &mut buffer).map_err(invalid)?;
            wire.send(&buffer[..size])?;
            state
                .read_message(&wire.receive()?, &mut buffer)
                .map_err(|_| invalid("code authentication failed"))?;
        } else {
            state
                .read_message(&wire.receive()?, &mut buffer)
                .map_err(|_| invalid("code authentication failed"))?;
            let size = state.write_message(&[], &mut buffer).map_err(invalid)?;
            wire.send(&buffer[..size])?;
        }
        Ok(Self {
            wire,
            state: state.into_transport_mode().map_err(invalid)?,
        })
    }
    fn send(&mut self, plaintext: &[u8]) -> io::Result<()> {
        let mut ciphertext = [0; 1024];
        let size = self
            .state
            .write_message(plaintext, &mut ciphertext)
            .map_err(invalid)?;
        self.wire.send(&ciphertext[..size])
    }
    fn receive(&mut self) -> io::Result<Vec<u8>> {
        let ciphertext = self.wire.receive()?;
        let mut plaintext = vec![0; 1024];
        let size = self
            .state
            .read_message(&ciphertext, &mut plaintext)
            .map_err(invalid)?;
        plaintext.truncate(size);
        Ok(plaintext)
    }
}

fn safe_name(name: &str) -> String {
    crate::proto::safe_peer_name(name)
}

/// 登録ビーコンと承認方式で名乗る表示名。LAN 全体に見えるため、環境変数
/// (COMPUTERNAME/HOSTNAME)の実名ではなく OS の分かる匿名ラベルを返す。
/// 相手がどの Mac かは確認番号とアドレスで確かめる
fn beacon_name() -> String {
    if cfg!(target_os = "macos") {
        "Knit (Mac)".into()
    } else if cfg!(target_os = "windows") {
        "Knit (Windows)".into()
    } else {
        "Knit".into()
    }
}

/// SAS ワイヤに載せる名前は 64 バイト上限(受信側の検査値)。UTF-8 の文字境界を
/// 壊さず切詰める。切らないと長い・非 ASCII の端末名で承認が毎回拒否され、
/// 3 回の失敗で Locked になる
fn clamp_name_bytes(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    &s[..cut]
}

pub fn discover() -> io::Result<Vec<Candidate>> {
    discover_at(
        SocketAddr::from(([255, 255, 255, 255], PORT)),
        Duration::from_millis(900),
    )
}
fn discover_at(target: SocketAddr, wait: Duration) -> io::Result<Vec<Candidate>> {
    let s = UdpSocket::bind("0.0.0.0:0")?;
    s.set_broadcast(true)?;
    s.send_to(ASK, target)?;
    let until = Instant::now() + wait;
    let mut found = Vec::new();
    let mut data = [0; 512];
    while let Some(left) = until
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
    {
        s.set_read_timeout(Some(left))?;
        match s.recv_from(&mut data) {
            Ok((n, from)) if crate::net::is_allowed(from.ip()) => {
                if let Ok(b) = serde_json::from_slice::<Beacon>(&data[..n]) {
                    if b.version == 1
                        && !found.iter().any(|p: &Candidate| p.address == from)
                        && found.len() < 32
                    {
                        found.push(Candidate {
                            name: safe_name(&b.name),
                            address: from,
                        });
                    }
                }
            }
            Ok(_) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                break
            }
            Err(e) => return Err(e),
        }
    }
    found.sort_by_key(|p| p.address);
    Ok(found)
}
/// IP input is a connection hint, never an authentication credential.
pub fn manual_address(input: &str) -> io::Result<SocketAddr> {
    let ip: IpAddr = input.trim().parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "MacのIPアドレスを入力してください。",
        )
    })?;
    if !ip.is_ipv4()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip == IpAddr::V4(std::net::Ipv4Addr::BROADCAST)
    {
        return Err(invalid("invalid peer address"));
    }
    Ok(SocketAddr::new(ip, PORT))
}

#[cfg(test)]
mod tests {
    #[test]
    fn beacon_name_is_an_anonymous_label_without_the_host_name() {
        let name = beacon_name();
        assert!(name == "Knit (Mac)" || name == "Knit (Windows)" || name == "Knit");
        // 環境変数の端末名(COMPUTERNAME/HOSTNAME)がビーコンに漏れない
        for var in ["COMPUTERNAME", "HOSTNAME"] {
            if let Ok(host) = std::env::var(var) {
                assert!(!name.contains(&host), "beacon に端末名を載せない: {name}");
            }
        }
    }

    #[test]
    fn sas_name_clamp_keeps_utf8_boundaries_within_the_64_byte_wire_limit() {
        assert_eq!(clamp_name_bytes("abcdefghij", 64).len(), 10);
        let long_ascii: String = "a".repeat(100);
        assert_eq!(clamp_name_bytes(&long_ascii, 64).len(), 64);
        // 日本語(3バイト/文字)は境界で割らない。63バイト=21文字まで載る
        let jp = "あ".repeat(40);
        assert_eq!(clamp_name_bytes(&jp, 64), "あ".repeat(21));
        assert_eq!(clamp_name_bytes("", 64), "");
    }

    use super::*;
    fn invitation(lifetime: Duration) -> (Invitation, SocketAddr, String) {
        let token = credentials::generate().unwrap();
        // Reserve a test port by obtaining the address after bind in a test-only helper.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        (
            Invitation::bind(addr, token.clone(), 24900, lifetime).unwrap(),
            addr,
            token,
        )
    }
    #[test]
    fn code_and_frame_validation() {
        assert_eq!(parse_code("123 456").unwrap(), "123456");
        for bad in ["", "12345", "1234567", "１２３４５６", "123\n456", "abcdef"] {
            assert!(parse_code(bad).is_err());
        }
        assert!(frame_read(&mut &b"\xff\xff"[..]).is_err());
        assert!(manual_address("127.0.0.1").is_ok());
        assert!(manual_address("localhost\nTOKEN=x").is_err());
    }
    #[test]
    fn pairing_success_is_single_use_and_discovery_exposes_no_secret() {
        let (i, addr, token) = invitation(Duration::from_secs(10));
        let found = discover_at(addr, Duration::from_millis(150)).unwrap();
        assert_eq!(found.len(), 1);
        // ビーコンは端末名ではなく匿名ラベルで応答する(LAN 全体に見えるため)
        assert_eq!(found[0].name, beacon_name());
        let beacon = serde_json::to_string(&found).unwrap();
        assert!(!beacon.contains(&token));
        assert!(!beacon.contains(&i.code));
        let received = enroll(addr, &i.code, |key, peer| {
            assert_eq!(key, token);
            assert_eq!(peer.port(), 24900);
            Ok(())
        })
        .unwrap();
        assert_eq!(received, token);
        assert!(matches!(
            i.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::Registered(_)
        ));
        assert!(enroll(addr, &i.code, |_, _| panic!("must not save twice")).is_err());
    }
    #[test]
    fn existing_key_enrollment_preserves_bytes_without_changing_desktop_import() {
        let token = "Legacy-Key-With-Mixed-CASE-And-Symbols+/==";
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let invitation = Invitation::bind(address, token.into(), 24900, Duration::from_secs(10)).unwrap();
        assert!(enroll(address, &invitation.code, |_, _| panic!("canonical store must reject legacy key")).is_err());
        assert_eq!(invitation.events.recv_timeout(Duration::from_secs(2)).unwrap(), Event::AttemptFailed(2));
        let received = enroll_existing_key(address, &invitation.code, |key, peer| {
            assert_eq!(key.as_bytes(), token.as_bytes());
            assert_eq!(peer.port(), 24900);
            Ok(())
        }).unwrap();
        assert_eq!(received, token);
        assert!(matches!(invitation.events.recv_timeout(Duration::from_secs(2)).unwrap(), Event::Registered(_)));
        for bad in ["123456".into(), "x".repeat(31), "x".repeat(513), format!("{}\n", "x".repeat(32))] {
            assert!(credentials::validate_transport_key(&bad).is_err());
        }
    }
    #[test]
    fn wrong_codes_lock_the_invitation_without_saving() {
        let (i, addr, _) = invitation(Duration::from_secs(10));
        let wrong = if i.code == "000000" {
            "111111"
        } else {
            "000000"
        };
        for left in [2, 1, 0] {
            assert!(enroll(addr, wrong, |_, _| panic!("wrong code must not save")).is_err());
            let event = i.events.recv_timeout(Duration::from_secs(2)).unwrap();
            assert_eq!(
                event,
                if left == 0 {
                    Event::Locked
                } else {
                    Event::AttemptFailed(left)
                }
            );
        }
        assert!(enroll(addr, &i.code, |_, _| panic!("locked")).is_err());
    }
    #[test]
    fn storage_failure_never_reports_registration() {
        let (i, addr, _) = invitation(Duration::from_secs(10));
        assert!(enroll(addr, &i.code, |_, _| Err(io::Error::other(
            "test storage failure"
        )))
        .is_err());
        assert_eq!(
            i.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::AttemptFailed(2)
        );
    }
    #[test]
    fn expiry_and_cancel_stop_listeners_and_active_connections() {
        let (i, addr, _) = invitation(Duration::from_millis(120));
        assert_eq!(
            i.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::Expired
        );
        assert!(TcpStream::connect(addr).is_err());
        drop(i);
        let (i, addr, _) = invitation(Duration::from_secs(10));
        let mut socket = TcpStream::connect(addr).unwrap();
        configure(&socket, Duration::from_secs(1)).unwrap();
        frame_read(&mut socket).unwrap();
        let now = Instant::now();
        drop(i);
        assert!(now.elapsed() < Duration::from_secs(1));
        assert!(frame_read(&mut socket).is_err());
        assert!(TcpStream::connect(addr).is_err());
    }
    #[test]
    fn malformed_peer_uses_an_attempt_without_releasing_a_credential() {
        let (i, addr, _) = invitation(Duration::from_secs(10));
        let mut peer = TcpStream::connect(addr).unwrap();
        configure(&peer, Duration::from_secs(1)).unwrap();
        frame_read(&mut peer).unwrap();
        peer.write_all(&[0xff, 0xff]).unwrap();
        assert_eq!(
            i.events.recv_timeout(Duration::from_secs(2)).unwrap(),
            Event::AttemptFailed(2)
        );
        assert!(frame_read(&mut peer).is_err());
    }
    #[test]
    fn expiry_closes_a_stalled_handshake() {
        let (i, addr, _) = invitation(Duration::from_millis(200));
        let mut peer = TcpStream::connect(addr).unwrap();
        configure(&peer, Duration::from_secs(1)).unwrap();
        frame_read(&mut peer).unwrap();
        assert!(frame_read(&mut peer).is_err());
        let mut expired = false;
        for _ in 0..2 {
            if i.events.recv_timeout(Duration::from_secs(1)).unwrap() == Event::Expired {
                expired = true;
                break;
            }
        }
        assert!(expired);
    }
    #[test]
    fn client_cancel_interrupts_a_silent_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = cancelled.clone();
        let client = thread::spawn(move || {
            enroll_cancellable(address, "123456", cancel, |_, _| {
                panic!("cancelled session must not save")
            })
        });
        let (_socket, _) = listener.accept().unwrap();
        cancelled.store(true, Ordering::SeqCst);
        let start = Instant::now();
        assert!(client.join().unwrap().is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    // ---- 承認方式(6桁を打たず、両方の画面の確認番号を見比べる) ----
    fn approval_client(addr: SocketAddr) -> JoinHandle<io::Result<Pending>> {
        thread::spawn(move || request_approval(addr, "Test PC", Arc::new(AtomicBool::new(false))))
    }
    fn next_approval(i: &Invitation) -> Approval {
        match i.events.recv_timeout(Duration::from_secs(5)).unwrap() {
            Event::Approval(a) => a,
            other => panic!("expected approval, got {other:?}"),
        }
    }

    #[test]
    fn approval_needs_both_sides_and_shows_the_same_number() {
        let (i, addr, token) = invitation(Duration::from_secs(30));
        let client = approval_client(addr);
        let approval = next_approval(&i);
        assert_eq!(approval.name, "Test PC");
        let pending = client.join().unwrap().unwrap();
        assert_eq!(pending.sas, approval.sas, "両方の画面に同じ確認番号が出る");
        assert_eq!(pending.sas.len(), 4);
        assert!(!pending.server_name.is_empty());
        // Mac が許可し、依頼側も確認して初めて長期キーが渡る
        approval.approve();
        let saved = Arc::new(Mutex::new(None));
        let sink = saved.clone();
        let got = pending
            .confirm(false, move |key, _| {
                *sink.lock().unwrap() = Some(key.to_string());
                Ok(())
            })
            .unwrap();
        assert_eq!(got, token);
        assert_eq!(saved.lock().unwrap().as_deref(), Some(token.as_str()));
        assert!(matches!(
            i.events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::Registered(_)
        ));
    }

    #[test]
    fn denied_approval_never_transfers_a_credential() {
        let (i, addr, _token) = invitation(Duration::from_secs(30));
        let client = approval_client(addr);
        let approval = next_approval(&i);
        let pending = client.join().unwrap().unwrap();
        approval.deny();
        let saved = Arc::new(AtomicBool::new(false));
        let flag = saved.clone();
        let result = pending.confirm(false, move |_, _| {
            flag.store(true, Ordering::SeqCst);
            Ok(())
        });
        assert!(result.is_err());
        assert!(!saved.load(Ordering::SeqCst), "拒否されたら何も保存しない");
        assert_eq!(
            i.events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::AttemptFailed(2)
        );
    }

    #[test]
    fn dropping_the_pending_request_cancels_it() {
        let (i, addr, _token) = invitation(Duration::from_secs(30));
        let client = approval_client(addr);
        let approval = next_approval(&i);
        drop(client.join().unwrap().unwrap()); // 依頼側が確認せずにやめた
        // 切断を検知して、Mac の確認画面を取り下げる
        assert_eq!(
            i.events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::Withdrawn
        );
        approval.approve(); // 遅れて許可しても、相手がいないので何も起きない
        assert_eq!(
            i.events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::AttemptFailed(2)
        );
    }

    #[test]
    fn approval_attempts_are_limited_like_the_code() {
        let (i, addr, _token) = invitation(Duration::from_secs(30));
        for left in [2usize, 1] {
            let client = approval_client(addr);
            next_approval(&i).deny();
            drop(client.join().unwrap().unwrap());
            assert_eq!(
                i.events.recv_timeout(Duration::from_secs(5)).unwrap(),
                Event::AttemptFailed(left)
            );
        }
        let client = approval_client(addr);
        next_approval(&i).deny();
        drop(client.join().unwrap().unwrap());
        assert_eq!(i.events.recv_timeout(Duration::from_secs(5)).unwrap(), Event::Locked);
    }

    #[test]
    fn a_key_that_does_not_match_the_commitment_is_rejected_before_any_prompt() {
        let (i, addr, _token) = invitation(Duration::from_secs(30));
        let cancelled = AtomicBool::new(false);
        let s = TcpStream::connect(addr).unwrap();
        let mut wire = Wire::new(s, &cancelled, Instant::now() + Duration::from_secs(10)).unwrap();
        let id = wire.receive().unwrap();
        // 鍵Aで約束しておきながら、別の鍵を明かす(確認番号を合わせるための細工)
        let honest = public_of(&random32().unwrap());
        let other = public_of(&random32().unwrap());
        let nonce = [7u8; 16];
        let mut m1 = Vec::from(SAS_MARK);
        m1.extend_from_slice(&commitment(&honest, &nonce));
        m1.extend_from_slice(b"Attacker");
        wire.send(&m1).unwrap();
        let _m2 = wire.receive().unwrap();
        let mut m3 = Vec::new();
        m3.extend_from_slice(&other);
        m3.extend_from_slice(&nonce);
        wire.send(&m3).unwrap();
        let _ = id;
        // 利用者への確認は出ず、失敗として数えられる
        assert_eq!(
            i.events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::AttemptFailed(2)
        );
    }

    #[test]
    fn confirmation_number_binds_both_keys_and_low_order_points_are_refused() {
        let (ka, kb) = (random32().unwrap(), random32().unwrap());
        let (pa, pb) = (public_of(&ka), public_of(&kb));
        let (na, nb) = ([1u8; 16], [2u8; 16]);
        let sa = shared_secret(&ka, &pb).unwrap();
        let sb = shared_secret(&kb, &pa).unwrap();
        assert_eq!(sa, sb, "DH は両側で同じ値になる");
        let one = sas_and_key(b"id", (&pa, &na), (&pb, &nb), &sa);
        let two = sas_and_key(b"id", (&pa, &na), (&pb, &nb), &sb);
        assert_eq!(one, two, "両側は同じ確認番号と鍵を得る");
        // 片側だけ違う鍵(割り込み)なら、鍵は必ず変わる
        let mitm = public_of(&random32().unwrap());
        assert_ne!(sas_and_key(b"id", (&pa, &na), (&mitm, &nb), &sa).1, one.1);
        assert!(shared_secret(&ka, &[0u8; 32]).is_err(), "全ゼロの点は拒否");
    }

    #[test]
    fn a_silent_requester_cannot_hold_the_listener_for_the_approval_window() {
        let (i, addr, _token) = invitation(Duration::from_secs(60));
        let cancelled = AtomicBool::new(false);
        let s = TcpStream::connect(addr).unwrap();
        let mut wire = Wire::new(s, &cancelled, Instant::now() + Duration::from_secs(30)).unwrap();
        let _id = wire.receive().unwrap();
        let mut m1 = Vec::from(SAS_MARK);
        m1.extend_from_slice(&commitment(&public_of(&random32().unwrap()), &[1u8; 16]));
        m1.extend_from_slice(b"Slow");
        wire.send(&m1).unwrap();
        let _m2 = wire.receive().unwrap();
        // 開示(msg3)を送らず黙る。承認待ちの長い期限ではなく、通常の短い期限(8秒)で切られる
        let started = Instant::now();
        let event = i.events.recv_timeout(Duration::from_secs(20)).unwrap();
        assert_eq!(event, Event::AttemptFailed(2));
        assert!(started.elapsed() < Duration::from_secs(15), "待ち受けを長く塞がない");
    }

    #[test]
    fn number_choices_hide_the_real_number_among_four_distinct_options() {
        let mut positions = std::collections::HashSet::new();
        for _ in 0..60 {
            let list = number_choices("4821").unwrap();
            assert_eq!(list.len(), 4);
            assert!(list.iter().all(|n| n.len() == 4 && n.bytes().all(|b| b.is_ascii_digit())));
            let unique: std::collections::HashSet<_> = list.iter().collect();
            assert_eq!(unique.len(), 4, "候補は重ならない");
            positions.insert(list.iter().position(|n| n == "4821").unwrap());
        }
        assert!(positions.len() > 1, "本物の番号の位置は毎回変わる");
    }
}
