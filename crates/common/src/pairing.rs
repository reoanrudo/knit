//! Explicit, short-lived enrollment. Discovery is only a hint; SPAKE2 and Noise
//! authenticate the code before any persistent credential is transferred.
use crate::credentials;
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

#[derive(Debug, PartialEq)]
pub enum Event {
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
        if token.is_empty() || token.len() > 512 || service_port == 0 {
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
        let name = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "Mac · Knit".into());
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
                        let result = serve_one(
                            s,
                            &worker_code,
                            &invitation_id,
                            &token,
                            service_port,
                            &worker_stop,
                            attempt_until,
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
fn serve_one(
    s: TcpStream,
    code: &str,
    id: &str,
    token: &str,
    port: u16,
    stop: &AtomicBool,
    until: Instant,
) -> io::Result<()> {
    let mut wire = Wire::new(s, stop, until)?;
    wire.send(id.as_bytes())?;
    let (state, message) = Spake2::<Ed25519Group>::start_b(
        &Password::new(code.as_bytes()),
        &Identity::new(b"knit-windows-v1"),
        &Identity::new(id.as_bytes()),
    );
    let incoming = wire.receive()?;
    wire.send(&message)?;
    let key = state
        .finish(&incoming)
        .map_err(|_| invalid("invalid PAKE message"))?;
    let mut channel = Channel::handshake(wire, &key, false)?;
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
    channel.send(b"ready")?;
    let credential: Credential = serde_json::from_slice(&channel.receive()?).map_err(invalid)?;
    // OS credential stores only accept the new canonical token format. Legacy
    // env keys remain usable by the existing app, but cannot be exported here.
    let token = credentials::parse_key(&credential.token)?;
    if token != credential.token || credential.port == 0 {
        return Err(invalid("unsupported credential"));
    }
    check_active(&cancelled, until)?;
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
}
