// 共通プロトコル定義(JSON Lines over TCP)
pub mod envutil {
    //! 設定値の参照: 環境変数 > 実行ファイル同階層の .env > ~/.config/tsunagu/env。
    //! 配布形態(.app バンドル埋め込み / exe 同梱 .env / ホーム設定)のどれでも
    //! 同一コードで動かすための仕組み。KEY=VALUE 形式(1行1エントリ、# はコメント)。
    //! 旧名称(v0.7 以前の seamless-desk)の環境変数・設定パスもフォールバックで
    //! 読むため、既存環境を書き換えずにそのまま移行できる。

    use std::sync::OnceLock;

    /// 旧名称(v0.7 以前)へのキー変換。
    /// TSUNAGU_TOKEN ← SEAMLESS_DESK_TOKEN、TSUNAGU_X ← SEAMLESS_X
    fn legacy_key(key: &str) -> String {
        if key == "TSUNAGU_TOKEN" {
            return "SEAMLESS_DESK_TOKEN".to_string();
        }
        match key.strip_prefix("TSUNAGU_") {
            Some(rest) => format!("SEAMLESS_{rest}"),
            None => String::new(),
        }
    }

    fn entries() -> &'static Vec<(String, String)> {
        static E: OnceLock<Vec<(String, String)>> = OnceLock::new();
        E.get_or_init(|| {
            let mut v = Vec::new();
            let mut paths = Vec::new();
            if let Ok(exe) = std::env::current_exe() {
                if let Some(d) = exe.parent() {
                    paths.push(d.join(".env"));
                    // .app バンドル配布用: Contents/Resources/.env
                    // (MacOS/ 内に置くと codesign の署名対象になって失敗するため)
                    if let Some(res) = d.parent().map(|p| p.join("Resources/.env")) {
                        paths.push(res);
                    }
                }
            }
            for key in ["HOME", "USERPROFILE"] {
                if let Some(home) = std::env::var_os(key) {
                    let cfg = std::path::Path::new(&home).join(".config");
                    paths.push(cfg.join("tsunagu/env"));
                    // 旧名称時代の設定パス(v0.7 からの移行措置)
                    paths.push(cfg.join("seamless-desk/env"));
                }
            }
            for p in paths {
                let Ok(s) = std::fs::read_to_string(&p) else { continue };
                for line in s.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some((k, val)) = line.split_once('=') {
                        v.push((
                            k.trim().to_string(),
                            val.trim().trim_matches('"').to_string(),
                        ));
                    }
                }
            }
            v
        })
    }

    /// 環境変数を第一優先とし、未設定なら設定ファイル群から検索する。
    /// 旧名称のキー(SEAMLESS_*)も最後に確認する(v0.7 設定からの移行)
    pub fn get(key: &str) -> Option<String> {
        if let Ok(v) = std::env::var(key) {
            if !v.is_empty() {
                return Some(v);
            }
        }
        let legacy = legacy_key(key);
        let hit = entries()
            .iter()
            .find(|(k, _)| k == key)
            .or_else(|| {
                if legacy.is_empty() {
                    None
                } else {
                    entries().iter().find(|(k, _)| *k == legacy)
                }
            })
            .map(|(_, v)| v.clone());
        // 旧名称の環境変数も受け入れる(スクリプト側の書き換え漏れ保険)
        if hit.is_none() && !legacy.is_empty() {
            if let Ok(v) = std::env::var(&legacy) {
                if !v.is_empty() {
                    return Some(v);
                }
            }
        }
        hit
    }
}

pub mod proto {
    use serde::{Deserialize, Serialize};

    pub const PORT: u16 = 24900;
    /// プロトコル版。9: Leave 追加・Focus/Minimize 削除・版交渉(MIN_VERSION)導入。
    /// 10: ファイル・画像を大容量経路(bulk, 24902)へ移し本線から File*/ClipData を削除
    pub const VERSION: u32 = 10;
    /// 接続を受け入れる最小の相手版。新しいメッセージは未知として無視される
    /// (decode が None を返す)ため、MIN_VERSION 以上なら新旧混在でも通信できる。
    /// 片側だけ更新された状態で接続拒否が続く事故を防ぐ
    pub const MIN_VERSION: u32 = 10;

    /// 相手の版を受け入れてよいか
    pub fn compatible(peer: u32) -> bool {
        peer >= MIN_VERSION
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(tag = "t")]
    pub enum Msg {
        #[serde(rename = "hello")]
        Hello {
            ver: u32,
            name: String,
            token: String,
            /// 送信側の画面幅/高さ(px)。スケール自動算出と絶対座標送信に使う
            #[serde(default)]
            w: i32,
            #[serde(default)]
            h: i32,
        },
        #[serde(rename = "hello_ok")]
        HelloOk { name: String, w: i32, h: i32 },
        /// 画面構成の変化(Windows→Mac)。解像度変更・モニター抜き差しで送る
        #[serde(rename = "screen")]
        Screen { w: i32, h: i32 },
        #[serde(rename = "key")]
        Key {
            kc: u16,
            down: bool,
            ctrl: bool,
            opt: bool,
            cmd: bool,
            shift: bool,
            /// 翻訳済みキー(⌘]→Tab 等)。Win 側の ⌘Tab→Alt+Tab 変換など
            /// 「生の Mac 入力」前提の特殊処理を適用しない
            #[serde(default)]
            tr: bool,
        },
        #[serde(rename = "mouse_move")]
        MouseMove { dx: f64, dy: f64 },
        /// カーソル絶対位置(0..1 正規化)。Windows 側は MOUSEEVENTF_ABSOLUTE で注入し、
        /// ポインタ加速曲線を通さず Mac の速度感をそのまま再現する
        #[serde(rename = "mouse_abs")]
        MouseAbs { nx: f64, ny: f64 },
        #[serde(rename = "mouse_btn")]
        MouseButton { btn: u8, down: bool },
        #[serde(rename = "scroll")]
        Scroll { dx: f64, dy: f64 },
        /// Windows 左端到達による復帰通知。ny = 復帰時のカーソル高さ(0..1、Mac 側復帰位置へ反映)
        #[serde(rename = "return")]
        Return {
            #[serde(default)]
            ny: f64,
        },
        /// クリップボード同期(プレーンテキスト)
        #[serde(rename = "clip")]
        Clip { text: String },
        /// Mac が制御を取り戻した(Windows を離れた)。Windows は押下中の全キー・
        /// ボタン・Alt+Tab を解放する。ホットキー/Mac 内完結の左端復帰/切断など
        /// Windows が自力で検知できない離脱経路のための後片付け合図
        #[serde(rename = "leave")]
        Leave,
        /// カーソル絶対ワープ(0..1 正規化。切替時に相手画面の対応位置へ飛ばす)
        #[serde(rename = "warp")]
        Warp { nx: f64, ny: f64 },
        #[serde(rename = "ping")]
        Ping {
            /// 送信時刻(unix ms)。Pong にエコーバックされ RTT 測定に使う
            #[serde(default)]
            ts: u64,
        },
        #[serde(rename = "pong")]
        Pong {
            #[serde(default)]
            ts: u64,
        },
        /// 設定同期: ⌘キーのマップ先(false=Ctrl 既定 / true=Alt)と
        /// Windows スピーカーのミュート(true=接続中ミュート=Mac のみ発音)。
        /// 接続確立時とメニュー切替時に Mac→Windows へ送る
        #[serde(rename = "cfg")]
        Cfg {
            cmd_alt: bool,
            #[serde(default)]
            spk_mute: bool,
            /// Windows 画面の位置(0=Macの右/1=左/2=上/3=下。Deskflow の links 相当)
            #[serde(default)]
            side: u8,
            /// クリップボード共有。false の間は Windows 側も送らない(相手任せにしない)
            #[serde(default = "default_true")]
            clip: bool,
        },
        /// Windows の音量制御(0=up / 1=down / 2=ミュート)。Mac メニューから送る
        #[serde(rename = "vol")]
        Vol { op: u8 },
        /// 接続品質通知: Mac が測定した RTT(ms)を Windows 側の表示へ回す
        #[serde(rename = "stat")]
        Stat { rtt: u64 },
        #[serde(rename = "bye")]
        Bye,
    }

    fn default_true() -> bool {
        true
    }

    pub fn encode(msg: &Msg) -> String {
        let mut s = serde_json::to_string(msg).unwrap_or_default();
        s.push('\n');
        s
    }

    pub fn decode(line: &str) -> Option<Msg> {
        serde_json::from_str(line.trim()).ok()
    }
}

pub mod files {
    //! 受信ファイルの保存(Mac/Windows 共通)。名前の無害化と同名回避を一箇所に置き、
    //! 両側で規則が食い違う(Windows だけ上書きしていた)事故を防ぐ
    use std::path::{Path, PathBuf};

    /// 1 ファイルの受信上限(送信側の合計上限と同じ)
    pub const MAX_FILE: u64 = 200 * 1024 * 1024;

    /// 相手から届いたファイル名を、どちらの OS でも安全な単一の名前へ変換する。
    /// パス区切り・予約文字・制御文字は '_'、先頭末尾の '.' と空白は除去、
    /// Windows の予約デバイス名(CON/NUL/COM1 等)は先頭に '_' を付ける
    pub fn sanitize(name: &str) -> String {
        let mut s: String = name
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                c if c.is_control() => '_',
                c => c,
            })
            .collect();
        s = s.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_string();
        if s.chars().count() > 200 {
            s = s.chars().take(200).collect();
        }
        if s.is_empty() {
            return "file".into();
        }
        let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit());
        if reserved {
            s.insert(0, '_');
        }
        s
    }

    /// 相手 PC から来たファイルに「外部から入手した」印を付ける。これが無いと、
    /// 受信した実行ファイルや .app が OS の警告(SmartScreen / Gatekeeper)なしで開ける。
    /// 付与できなくても受信自体は続ける(NTFS 以外のドライブ等)
    pub fn mark_untrusted(path: &Path) {
        #[cfg(windows)]
        {
            let mut ads = path.as_os_str().to_owned();
            ads.push(":Zone.Identifier");
            let _ = std::fs::write(ads, "[ZoneTransfer]\r\nZoneId=3\r\n");
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::ffi::OsStrExt;
            unsafe extern "C" {
                fn setxattr(
                    path: *const core::ffi::c_char,
                    name: *const core::ffi::c_char,
                    value: *const core::ffi::c_void,
                    size: usize,
                    position: u32,
                    options: i32,
                ) -> i32;
            }
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            // 形式: フラグ;時刻(16進);取得元アプリ;UUID(省略可)
            let value = format!("0081;{secs:x};Tsunagu;");
            let Ok(p) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return };
            unsafe {
                setxattr(
                    p.as_ptr(),
                    c"com.apple.quarantine".as_ptr(),
                    value.as_ptr() as *const core::ffi::c_void,
                    value.len(),
                    0,
                    0,
                );
            }
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        let _ = path;
    }

    /// dir 内に新規ファイルを作る。同名があれば「名前 (n).拡張子」で回避する
    pub fn create_unique(dir: &Path, name: &str) -> Option<(std::fs::File, PathBuf)> {
        std::fs::create_dir_all(dir).ok()?;
        let base = sanitize(name);
        let p = Path::new(&base);
        let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        for i in 0..1000u32 {
            let cand = if i == 0 { dir.join(&base) } else { dir.join(format!("{stem} ({i}){ext}")) };
            if let Ok(f) = std::fs::OpenOptions::new().write(true).create_new(true).open(&cand) {
                mark_untrusted(&cand);
                return Some((f, cand));
            }
        }
        None
    }
}

pub mod net {
    //! 接続元の判定など、両 OS の待受処理で共通の小物
    use std::net::IpAddr;

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
}

pub mod connect {
    //! 複数の接続候補(有線直結・LAN・Tailscale 等)へ同時に接続を試み、最初に
    //! 繋がったものを使う。遅延の小さい経路ほど早く繋がるため、自然に最速経路が選ばれ、
    //! 一つが使えなくなっても次回の接続で別経路へ切り替わる
    use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
    use std::time::Duration;

    /// "host1,host2:port" のようなカンマ区切りを解決する(ポート省略時は既定ポート)
    pub fn parse_hosts(list: &str, port: u16) -> Vec<SocketAddr> {
        list.split(',')
            .map(str::trim)
            .filter(|h| !h.is_empty())
            .flat_map(|h| {
                let with_port = if h.contains(':') && !h.starts_with('[') && h.matches(':').count() == 1 {
                    h.to_string()
                } else {
                    format!("{h}:{port}")
                };
                with_port.to_socket_addrs().ok().and_then(|mut it| it.next())
            })
            .collect()
    }

    pub fn first_reachable(addrs: &[SocketAddr], timeout: Duration) -> Option<(TcpStream, SocketAddr)> {
        let (tx, rx) = std::sync::mpsc::channel();
        for a in addrs.iter().copied() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                if let Ok(s) = TcpStream::connect_timeout(&a, timeout) {
                    let _ = tx.send((s, a));
                }
            });
        }
        drop(tx);
        rx.recv_timeout(timeout + Duration::from_millis(500)).ok()
    }
}

pub mod bulk {
    //! 大容量データ(ファイル・画像)専用の経路。本線(入力・制御の JSON Lines)と
    //! 別の TCP 接続に分けることで、転送中もマウス・キー・ping が詰まらない
    //! (旧方式は 3MB チャンクが入力と同じキューとソケットに並び、転送中は入力が止まり、
    //! 転送が 10 秒を超えると pong 遅延で切断と誤判定された)。
    //! フレーム: [種別 u8][長さ u32 LE][本体]。base64 を使わない(33% 増と変換 CPU の削減)
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};

    /// 本線ポートからの差分(24900 → 24902。24901 は音声)
    pub const PORT_OFFSET: u16 = 2;
    const HELLO: &str = "TSUNAGU-BULK1";
    /// 1 フレームの本体上限(DATA は CHUNK、その他は小さなメタ情報のみ)
    pub const MAX_FRAME: usize = 1024 * 1024;
    /// ファイル・画像データを分割する単位
    pub const CHUNK: usize = 256 * 1024;
    /// 1 回の一括送信の合計上限
    pub const MAX_TOTAL: u64 = 200 * 1024 * 1024;
    /// クリップボード画像の上限(生 DIB)
    pub const MAX_IMAGE: usize = 64 * 1024 * 1024;

    pub const KEEPALIVE: u8 = 0;
    pub const FILE_BEGIN: u8 = 1;
    pub const DATA: u8 = 2;
    pub const FILE_END: u8 = 3;
    pub const BATCH_END: u8 = 4;
    pub const DROP_BEGIN: u8 = 5;
    pub const DROP_END: u8 = 6;
    pub const IMAGE_BEGIN: u8 = 7;
    pub const IMAGE_END: u8 = 8;

    pub fn write_frame(w: &mut impl Write, kind: u8, body: &[u8]) -> std::io::Result<()> {
        let mut head = [0u8; 5];
        head[0] = kind;
        head[1..].copy_from_slice(&(body.len() as u32).to_le_bytes());
        w.write_all(&head)?;
        w.write_all(body)
    }

    /// 1 フレーム読む(本体は buf に入れ直す)。上限超過は不正として切断させる
    pub fn read_frame(r: &mut impl Read, buf: &mut Vec<u8>) -> std::io::Result<u8> {
        let mut head = [0u8; 5];
        r.read_exact(&mut head)?;
        let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
        if len > MAX_FRAME {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too large"));
        }
        buf.resize(len, 0);
        r.read_exact(buf)?;
        Ok(head[0])
    }

    pub fn total_size(paths: &[PathBuf]) -> u64 {
        paths
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok().filter(|m| m.is_file()).map(|m| m.len()))
            .sum()
    }

    /// ファイル群を送る。drop=true は「掴んだまま境界越え」(受信側は OLE ドラッグで渡す)。
    /// 戻り値は送った件数
    pub fn send_files(w: &mut impl Write, paths: &[PathBuf], drop: bool) -> std::io::Result<usize> {
        if drop {
            write_frame(w, DROP_BEGIN, &[])?;
        }
        let mut buf = vec![0u8; CHUNK];
        let mut sent = 0;
        for p in paths {
            let Ok(meta) = std::fs::metadata(p) else { continue };
            let Some(name) = p.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
            if !meta.is_file() {
                continue;
            }
            let Ok(mut f) = std::fs::File::open(p) else { continue };
            let head = serde_json::json!({ "name": name, "size": meta.len() }).to_string();
            write_frame(w, FILE_BEGIN, head.as_bytes())?;
            let mut remain = meta.len();
            while remain > 0 {
                let n = f.read(&mut buf[..(remain.min(CHUNK as u64) as usize)])?;
                if n == 0 {
                    break;
                }
                write_frame(w, DATA, &buf[..n])?;
                remain -= n as u64;
            }
            write_frame(w, FILE_END, &[])?;
            sent += 1;
        }
        write_frame(w, BATCH_END, &[])?;
        if drop {
            write_frame(w, DROP_END, &[])?;
        }
        w.flush()?;
        Ok(sent)
    }

    /// クリップボード画像(DIB)を送る
    pub fn send_image(w: &mut impl Write, dib: &[u8]) -> std::io::Result<()> {
        write_frame(w, IMAGE_BEGIN, &(dib.len() as u64).to_le_bytes())?;
        for c in dib.chunks(CHUNK) {
            write_frame(w, DATA, c)?;
        }
        write_frame(w, IMAGE_END, &[])?;
        w.flush()
    }

    /// 受信完了の単位
    #[derive(Debug)]
    pub enum Event {
        /// 一括送信が完了した(drop=true は掴みドラッグ)
        Files { paths: Vec<PathBuf>, drop: bool },
        Image(Vec<u8>),
    }

    enum Sink {
        None,
        File { f: std::fs::File, remain: u64 },
        Image { data: Vec<u8>, remain: usize },
    }

    /// 受信側の状態機械。フレームを順に与えると完了時に Event を返す
    pub struct Receiver {
        dir: PathBuf,
        sink: Sink,
        paths: Vec<PathBuf>,
        drop: bool,
    }

    impl Receiver {
        pub fn new(dir: &Path) -> Self {
            Self { dir: dir.to_path_buf(), sink: Sink::None, paths: Vec::new(), drop: false }
        }

        pub fn feed(&mut self, kind: u8, body: &[u8]) -> Option<Event> {
            match kind {
                DROP_BEGIN => self.drop = true,
                FILE_BEGIN => {
                    self.sink = Sink::None;
                    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
                    let name = v["name"].as_str().unwrap_or("file");
                    let size = v["size"].as_u64().unwrap_or(0);
                    if size > crate::files::MAX_FILE {
                        return None;
                    }
                    let (f, path) = crate::files::create_unique(&self.dir, name)?;
                    self.paths.push(path);
                    self.sink = Sink::File { f, remain: size };
                }
                DATA => match &mut self.sink {
                    Sink::File { f, remain } => {
                        // 宣言サイズを超える・書けないデータは、そのファイルごと破棄する
                        if body.len() as u64 > *remain || f.write_all(body).is_err() {
                            self.sink = Sink::None;
                            if let Some(p) = self.paths.pop() {
                                let _ = std::fs::remove_file(p);
                            }
                        } else {
                            *remain -= body.len() as u64;
                        }
                    }
                    Sink::Image { data, remain } => {
                        if body.len() > *remain {
                            self.sink = Sink::None;
                        } else {
                            data.extend_from_slice(body);
                            *remain -= body.len();
                        }
                    }
                    Sink::None => {}
                },
                FILE_END => {
                    // 途中で切れた(宣言サイズに満たない)ファイルは残さない
                    if let Sink::File { remain, .. } = &self.sink {
                        if *remain > 0 {
                            if let Some(p) = self.paths.pop() {
                                let _ = std::fs::remove_file(p);
                            }
                        }
                    }
                    self.sink = Sink::None;
                }
                BATCH_END => {
                    self.sink = Sink::None;
                    let paths = std::mem::take(&mut self.paths);
                    let drop = std::mem::replace(&mut self.drop, false);
                    if !paths.is_empty() {
                        return Some(Event::Files { paths, drop });
                    }
                }
                IMAGE_BEGIN => {
                    let n = u64::from_le_bytes(body.try_into().ok()?) as usize;
                    if n == 0 || n > MAX_IMAGE {
                        return None;
                    }
                    self.sink = Sink::Image { data: Vec::with_capacity(n), remain: n };
                }
                IMAGE_END => {
                    if let Sink::Image { data, remain: 0 } = std::mem::replace(&mut self.sink, Sink::None) {
                        return Some(Event::Image(data));
                    }
                }
                _ => {}
            }
            None
        }
    }

    /// 大容量経路の送信口。接続が張り替わるたびに差し替える。
    /// 送信中はロックを保持するため、複数の転送のフレームが混ざらない
    pub struct Link {
        /// (世代, 書き込み口)。世代は張り替えのたびに増え、古い受信スレッドが
        /// 新しい接続を誤って外さないための照合に使う
        w: std::sync::Mutex<Option<(u64, std::net::TcpStream)>>,
        gen: std::sync::atomic::AtomicU64,
    }

    impl Default for Link {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Link {
        pub const fn new() -> Self {
            Self { w: std::sync::Mutex::new(None), gen: std::sync::atomic::AtomicU64::new(0) }
        }
        fn slot(&self) -> std::sync::MutexGuard<'_, Option<(u64, std::net::TcpStream)>> {
            self.w.lock().unwrap_or_else(|e| e.into_inner())
        }
        /// 新しい接続を据える。戻り値は世代(受信スレッドの終了時に clear_if へ渡す)
        pub fn set(&self, s: std::net::TcpStream) -> u64 {
            let g = self.gen.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if let Some((_, old)) = self.slot().replace((g, s)) {
                let _ = old.shutdown(std::net::Shutdown::Both);
            }
            g
        }
        /// 接続を捨てる(本線の切断時など)。受信スレッドも読み出しエラーで終わる
        pub fn clear(&self) {
            if let Some((_, old)) = self.slot().take() {
                let _ = old.shutdown(std::net::Shutdown::Both);
            }
        }
        /// 指定世代の接続がまだ据わっている時だけ捨てる
        pub fn clear_if(&self, gen: u64) {
            let mut g = self.slot();
            if g.as_ref().is_some_and(|(n, _)| *n == gen) {
                if let Some((_, old)) = g.take() {
                    let _ = old.shutdown(std::net::Shutdown::Both);
                }
            }
        }
        pub fn is_up(&self) -> bool {
            self.slot().is_some()
        }
        /// 送信する。未接続ならエラー、書き込み失敗なら接続を捨ててエラー
        pub fn send<T>(
            &self,
            f: impl FnOnce(&mut std::net::TcpStream) -> std::io::Result<T>,
        ) -> std::io::Result<T> {
            let mut g = self.slot();
            let Some((_, s)) = g.as_mut() else {
                return Err(std::io::Error::new(std::io::ErrorKind::NotConnected, "bulk link down"));
            };
            let r = f(s);
            if r.is_err() {
                if let Some((_, old)) = g.take() {
                    let _ = old.shutdown(std::net::Shutdown::Both);
                }
            }
            r
        }
    }

    /// 経路ごとの設定(受信先・完了時の処理・ログ出力先)
    pub struct Endpoint {
        pub link: &'static Link,
        pub token: String,
        pub dir: PathBuf,
        pub on_event: fn(Event),
        pub log: fn(&str),
    }

    fn spawn_reader(ep: &'static Endpoint, s: std::net::TcpStream, gen: u64) {
        std::thread::spawn(move || {
            let mut r = std::io::BufReader::with_capacity(CHUNK + 16, s);
            let mut rx = Receiver::new(&ep.dir);
            let mut buf = Vec::new();
            while let Ok(kind) = read_frame(&mut r, &mut buf) {
                if let Some(e) = rx.feed(kind, &buf) {
                    (ep.on_event)(e);
                }
            }
            ep.link.clear_if(gen);
            (ep.log)("[bulk] 受信経路が切れました");
        });
    }

    /// 待受側: 認証を通った接続を送信口に据え、受信スレッドを起こす
    pub fn serve(ep: &'static Endpoint, bind: &str, port: u16, allow: fn(std::net::IpAddr) -> bool) {
        let listener = match std::net::TcpListener::bind((bind, port)) {
            Ok(l) => l,
            Err(e) => {
                (ep.log)(&format!("[bulk] listen {bind}:{port} 失敗: {e}(ファイル・画像の転送は不可)"));
                return;
            }
        };
        (ep.log)(&format!("[bulk] listening on {bind}:{port}"));
        for s in listener.incoming() {
            let Ok(mut s) = s else { continue };
            let Ok(peer) = s.peer_addr() else { continue };
            if !allow(peer.ip()) {
                (ep.log)(&format!("[bulk] rejected: {peer}"));
                continue;
            }
            s.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok();
            if !server_handshake(&mut s, &ep.token) {
                (ep.log)(&format!("[bulk] invalid handshake from {peer}"));
                continue;
            }
            // 接続側は 10 秒毎にキープアライブを送る。3 回分届かなければ死んだ経路
            s.set_read_timeout(Some(std::time::Duration::from_secs(35))).ok();
            s.set_write_timeout(Some(std::time::Duration::from_secs(20))).ok();
            let Ok(w) = s.try_clone() else { continue };
            let gen = ep.link.set(w);
            (ep.log)(&format!("[bulk] established ({peer})"));
            spawn_reader(ep, s, gen);
        }
    }

    /// 接続側: 本線が繋がっている間、大容量経路が無ければ張り直し、キープアライブを送る。
    /// 接続先は本線がいま使っている相手(複数経路のどれで繋がったか)に追従する
    pub fn connect_loop(
        ep: &'static Endpoint,
        addr: fn() -> Option<std::net::SocketAddr>,
        main_up: fn() -> bool,
    ) {
        let mut last_keepalive = std::time::Instant::now();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if !main_up() {
                ep.link.clear();
                continue;
            }
            if ep.link.is_up() {
                if last_keepalive.elapsed() >= std::time::Duration::from_secs(10) {
                    last_keepalive = std::time::Instant::now();
                    let _ = ep.link.send(|w| write_frame(w, KEEPALIVE, &[]));
                }
                continue;
            }
            let Some(addr) = addr() else { continue };
            let Ok(mut s) = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(3)) else {
                continue;
            };
            s.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok();
            if let Err(e) = client_handshake(&mut s, &ep.token) {
                (ep.log)(&format!("[bulk] handshake 失敗: {e}"));
                continue;
            }
            s.set_read_timeout(None).ok();
            s.set_write_timeout(Some(std::time::Duration::from_secs(20))).ok();
            let Ok(w) = s.try_clone() else { continue };
            let gen = ep.link.set(w);
            (ep.log)(&format!("[bulk] established (→{addr})"));
            spawn_reader(ep, s, gen);
        }
    }

    /// 接続側のハンドシェイク
    pub fn client_handshake(s: &mut std::net::TcpStream, token: &str) -> std::io::Result<()> {
        use std::io::BufRead;
        s.write_all(format!("{HELLO} {token}\n").as_bytes())?;
        let mut line = String::new();
        std::io::BufReader::new(s.try_clone()?.take(64)).read_line(&mut line)?;
        if line.trim() == "ok" {
            Ok(())
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "bulk handshake rejected"))
        }
    }

    /// 待受側のハンドシェイク(1 バイトずつ読むことで、後続のフレームを取りこぼさない)
    pub fn server_handshake(s: &mut std::net::TcpStream, token: &str) -> bool {
        let mut line = Vec::new();
        let mut b = [0u8; 1];
        while line.len() < 512 {
            if s.read_exact(&mut b).is_err() {
                return false;
            }
            if b[0] == b'\n' {
                break;
            }
            line.push(b[0]);
        }
        let ok = String::from_utf8_lossy(&line) == format!("{HELLO} {token}");
        let _ = s.write_all(if ok { b"ok\n" } else { b"ng\n" });
        ok
    }
}

pub mod b64 {
    /// 小さな base64 実装(依存追加なし。クリップボード画像の運搬用)
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    const R: [u8; 256] = {
        let mut t = [255u8; 256];
        let mut i = 0;
        while i < 64 {
            t[T[i] as usize] = i as u8;
            i += 1;
        }
        t
    };

    pub fn encode(data: &[u8]) -> String {
        let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
        for c in data.chunks(3) {
            let b = [*c.first().unwrap_or(&0), *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            out.push(T[(n >> 18 & 63) as usize] as char);
            out.push(T[(n >> 12 & 63) as usize] as char);
            out.push(if c.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
            out.push(if c.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
        }
        out
    }

    pub fn decode(s: &str) -> Option<Vec<u8>> {
        let b: Vec<u8> = s.bytes().filter(|c| *c != b'\n' && *c != b'\r').collect();
        if b.len() % 4 != 0 {
            return None;
        }
        let mut out = Vec::with_capacity(b.len() / 4 * 3);
        for c in b.chunks(4) {
            let mut n: u32 = 0;
            let mut pad = 0;
            for (i, ch) in c.iter().enumerate() {
                if *ch == b'=' {
                    n <<= 6;
                    pad += 1;
                } else {
                    let v = R[*ch as usize];
                    if v == 255 {
                        return None;
                    }
                    n = (n << 6) | v as u32;
                    let _ = i;
                }
            }
            out.push((n >> 16) as u8);
            if pad < 2 {
                out.push((n >> 8) as u8);
            }
            if pad < 1 {
                out.push(n as u8);
            }
        }
        Some(out)
    }
}

pub mod keymap {
    /// Mac keycode(HIToolbox)→ Windows 仮想キーコード(VK)
    pub fn mac_kc_to_win_vk(kc: u16) -> Option<u16> {
        let vk = match kc {
            // アルファベット(Mac keycode はレイアウト依存しない物理キー)
            0 => 0x41,   // A
            11 => 0x42,  // B
            8 => 0x43,   // C
            2 => 0x44,   // D
            14 => 0x45,  // E
            3 => 0x46,   // F
            5 => 0x47,   // G
            4 => 0x48,   // H
            34 => 0x49,  // I
            38 => 0x4A,  // J
            40 => 0x4B,  // K
            37 => 0x4C,  // L
            46 => 0x4D,  // M
            45 => 0x4E,  // N
            31 => 0x4F,  // O
            35 => 0x50,  // P
            12 => 0x51,  // Q
            15 => 0x52,  // R
            1 => 0x53,   // S
            17 => 0x54,  // T
            32 => 0x55,  // U
            9 => 0x56,   // V
            13 => 0x57,  // W
            7 => 0x58,   // X
            16 => 0x59,  // Y
            6 => 0x5A,   // Z
            // 数字row
            18 => 0x31, // 1
            19 => 0x32,
            20 => 0x33,
            21 => 0x34,
            23 => 0x35,
            22 => 0x36,
            26 => 0x37,
            28 => 0x38,
            25 => 0x39,
            29 => 0x30, // 0
            // 記号
            33 => 0xDB, // [
            30 => 0xDD, // ]
            39 => 0xBA, // ;
            41 => 0xDE, // '
            42 => 0xDC, // \
            43 => 0xBC, // ,
            47 => 0xBE, // .
            44 => 0xBF, // /
            50 => 0xC0, // `
            93 => 0xDC, // ¥(Mac JIS)→ Win バックスラッシュ/円記号
            27 => 0xBD, // -(US)/ー(JIS 長音)→ Win -[OEM_MINUS]
            94 => 0xBD, // _(Mac JIS)→ Win -
            // 制御・編集
            36 => 0x0D, // Return
            48 => 0x09, // Tab
            49 => 0x20, // Space
            51 => 0x08, // Delete(Backspace)
            53 => 0x1B, // Escape
            117 => 0x2E, // Forward Delete
            115 => 0x24, // Home
            119 => 0x23, // End
            116 => 0x21, // PageUp
            121 => 0x22, // PageDown
            123 => 0x25, // Left
            124 => 0x27, // Right
            125 => 0x28, // Down
            126 => 0x26, // Up
            // Fキー
            122 => 0x70, // F1
            120 => 0x71, // F2
            99 => 0x72,  // F3
            118 => 0x73, // F4
            96 => 0x74,  // F5
            97 => 0x75,  // F6
            98 => 0x76,  // F7
            100 => 0x77, // F8
            101 => 0x78, // F9
            109 => 0x79, // F10
            103 => 0x7A, // F11
            111 => 0x7B, // F12
            // テンキー
            82 => 0x60, // Num0
            83 => 0x61,
            84 => 0x62,
            85 => 0x63,
            86 => 0x64,
            87 => 0x65,
            88 => 0x66,
            89 => 0x67,
            91 => 0x68,
            92 => 0x69, // Num9
            65 => 0x6E, // Num .
            67 => 0x6A, // Num *
            69 => 0x6B, // Num +
            78 => 0x6D, // Num -
            75 => 0x6F, // Num /
            71 => 0x0C, // Clear
            76 => 0x0D, // テンキー Enter(Win 側で拡張キーフラグを付けて区別する)
            57 => 0x14, // Caps Lock(Mac 側は押下ごとに down+up の組で送る)
            114 => 0x2D, // Help(Mac の Ins 位置)→ Insert
            // F13〜F20(F13 は既定ホットキーのため通常は Mac 側で握られる)
            105 => 0x7C,
            107 => 0x7D,
            113 => 0x7E,
            106 => 0x7F,
            64 => 0x80,
            79 => 0x81,
            80 => 0x82,
            90 => 0x83,
            _ => return None,
        };
        Some(vk)
    }
}

pub mod charmap {
    /// 送信テスト用: Mac keycode → 表示文字(英数字・記号のみ)
    pub fn mac_kc_to_char(kc: u16) -> Option<char> {
        let c = match kc {
            0 => 'A', 11 => 'B', 8 => 'C', 2 => 'D', 14 => 'E', 3 => 'F', 5 => 'G',
            4 => 'H', 34 => 'I', 38 => 'J', 40 => 'K', 37 => 'L', 46 => 'M', 45 => 'N',
            31 => 'O', 35 => 'P', 12 => 'Q', 15 => 'R', 1 => 'S', 17 => 'T', 32 => 'U',
            9 => 'V', 13 => 'W', 7 => 'X', 16 => 'Y', 6 => 'Z',
            18 => '1', 19 => '2', 20 => '3', 21 => '4', 23 => '5', 22 => '6',
            26 => '7', 28 => '8', 25 => '9', 29 => '0',
            39 => ';', 41 => '\'', 43 => ',', 47 => '.', 44 => '/', 33 => '[', 30 => ']',
            49 => ' ', 36 => '\n', 48 => '\t',
            _ => return None,
        };
        Some(c)
    }
}

#[cfg(test)]
mod tests {
    use super::keymap::mac_kc_to_win_vk;
    use super::proto::*;

    #[test]
    fn keymap_covers_full_keyboard_and_numpad_enter() {
        assert_eq!(mac_kc_to_win_vk(76), Some(0x0D));
        assert_eq!(mac_kc_to_win_vk(57), Some(0x14));
        assert_eq!(mac_kc_to_win_vk(114), Some(0x2D));
        assert_eq!(mac_kc_to_win_vk(90), Some(0x83));
        // 数字row の取り違え実績(21=4, 23=5)の回帰防止
        assert_eq!(mac_kc_to_win_vk(21), Some(0x34));
        assert_eq!(mac_kc_to_win_vk(23), Some(0x35));
        assert_eq!(mac_kc_to_win_vk(200), None);
    }

    #[test]
    fn received_file_names_are_neutralized() {
        use super::files::sanitize;
        assert_eq!(sanitize("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(sanitize("a\\b:c.txt"), "a_b_c.txt");
        assert_eq!(sanitize("  .hidden  "), "hidden");
        assert_eq!(sanitize("CON.txt"), "_CON.txt");
        assert_eq!(sanitize("com1"), "_com1");
        assert_eq!(sanitize("console.txt"), "console.txt");
        assert_eq!(sanitize(""), "file");
        assert_eq!(sanitize("報告書.pdf"), "報告書.pdf");
    }

    #[test]
    fn same_name_is_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("tsunagu-files-{}", std::process::id()));
        let (_, a) = super::files::create_unique(&dir, "x.txt").unwrap();
        let (_, b) = super::files::create_unique(&dir, "x.txt").unwrap();
        assert_ne!(a, b);
        assert!(b.to_string_lossy().ends_with("x (1).txt"));
        #[cfg(target_os = "macos")]
        {
            let out = std::process::Command::new("xattr").arg("-p").arg("com.apple.quarantine").arg(&a).output().unwrap();
            assert!(String::from_utf8_lossy(&out.stdout).contains(";Tsunagu;"), "quarantine 属性が付いていない");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bulk_roundtrip_files_and_image() {
        use super::bulk::*;
        let base = std::env::temp_dir().join(format!("tsunagu-bulk-{}", std::process::id()));
        let src = base.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let big: Vec<u8> = (0..(CHUNK * 2 + 123)).map(|i| (i % 251) as u8).collect();
        std::fs::write(src.join("big.bin"), &big).unwrap();
        std::fs::write(src.join("a.txt"), b"hello").unwrap();
        let mut wire = Vec::new();
        let paths = vec![src.join("big.bin"), src.join("a.txt")];
        assert_eq!(send_files(&mut wire, &paths, true).unwrap(), 2);
        let dib: Vec<u8> = (0..(CHUNK + 7)).map(|i| (i % 13) as u8).collect();
        send_image(&mut wire, &dib).unwrap();

        let mut rx = Receiver::new(&base.join("dst"));
        let mut r = std::io::Cursor::new(wire);
        let mut buf = Vec::new();
        let mut events = Vec::new();
        while let Ok(k) = read_frame(&mut r, &mut buf) {
            if let Some(e) = rx.feed(k, &buf) {
                events.push(e);
            }
        }
        assert_eq!(events.len(), 2);
        match &events[0] {
            Event::Files { paths, drop } => {
                assert!(*drop);
                assert_eq!(std::fs::read(&paths[0]).unwrap(), big);
                assert_eq!(std::fs::read(&paths[1]).unwrap(), b"hello");
            }
            e => panic!("unexpected {e:?}"),
        }
        assert!(matches!(&events[1], Event::Image(d) if *d == dib));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn bulk_rejects_oversized_frames_and_overflowing_data() {
        use super::bulk::*;
        let mut wire = Vec::new();
        wire.push(DATA);
        wire.extend_from_slice(&((MAX_FRAME as u32) + 1).to_le_bytes());
        assert!(read_frame(&mut std::io::Cursor::new(wire), &mut Vec::new()).is_err());

        // 宣言サイズより多いデータを送られたらファイルごと破棄する
        let base = std::env::temp_dir().join(format!("tsunagu-bulk-bad-{}", std::process::id()));
        let mut rx = Receiver::new(&base);
        rx.feed(FILE_BEGIN, br#"{"name":"x.bin","size":3}"#);
        rx.feed(DATA, b"toolong");
        rx.feed(FILE_END, &[]);
        assert!(rx.feed(BATCH_END, &[]).is_none());
        assert!(!base.join("x.bin").exists());
        let _ = std::fs::remove_dir_all(base);
    }

    /// 実ソケットでの結合確認: 待受・接続・認証・送信・受信・誤トークン拒否
    #[test]
    fn bulk_link_over_real_tcp() {
        use super::bulk::*;
        use std::sync::{Mutex, OnceLock};
        static SERVER_LINK: Link = Link::new();
        static CLIENT_LINK: Link = Link::new();
        static SERVER: OnceLock<Endpoint> = OnceLock::new();
        static CLIENT: OnceLock<Endpoint> = OnceLock::new();
        static GOT: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
        let base = std::env::temp_dir().join(format!("tsunagu-link-{}", std::process::id()));
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let server = SERVER.get_or_init(|| Endpoint {
            link: &SERVER_LINK,
            token: "secret".into(),
            dir: base.join("srv"),
            on_event: |e| {
                if let Event::Files { paths, .. } = e {
                    GOT.lock().unwrap().push(std::fs::read(&paths[0]).unwrap());
                }
            },
            log: |_| {},
        });
        let client = CLIENT.get_or_init(|| Endpoint {
            link: &CLIENT_LINK,
            token: "secret".into(),
            dir: base.join("cli"),
            on_event: |_| {},
            log: |_| {},
        });
        std::thread::spawn(move || serve(server, "127.0.0.1", port, |_| true));
        static ADDR: OnceLock<std::net::SocketAddr> = OnceLock::new();
        let addr = *ADDR.get_or_init(|| format!("127.0.0.1:{port}").parse().unwrap());
        std::thread::spawn(move || connect_loop(client, || ADDR.get().copied(), || true));
        let t0 = std::time::Instant::now();
        while !CLIENT_LINK.is_up() && t0.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(CLIENT_LINK.is_up(), "接続が張れない");
        std::fs::create_dir_all(&base).unwrap();
        let src = base.join("payload.bin");
        std::fs::write(&src, b"over-the-wire").unwrap();
        CLIENT_LINK.send(|w| send_files(w, &[src.clone()], false)).unwrap();
        while GOT.lock().unwrap().is_empty() && t0.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(GOT.lock().unwrap().first().map(|v| v.as_slice()), Some(&b"over-the-wire"[..]));

        // 誤トークンは拒否される
        let mut bad = std::net::TcpStream::connect(addr).unwrap();
        assert!(client_handshake(&mut bad, "wrong").is_err());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn connect_picks_a_reachable_candidate() {
        use super::connect::*;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let good = l.local_addr().unwrap();
        // 閉じたポート(候補の 1 つ目)があっても、繋がる候補が選ばれる
        let dead = {
            let t = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            t.local_addr().unwrap()
        };
        let addrs = parse_hosts(&format!("127.0.0.1:{}, 127.0.0.1:{}", dead.port(), good.port()), 1);
        assert_eq!(addrs.len(), 2);
        let (_, picked) = first_reachable(&addrs, std::time::Duration::from_secs(2)).unwrap();
        assert_eq!(picked, good);
        assert_eq!(parse_hosts("127.0.0.1", 24900)[0].port(), 24900);
    }

    #[test]
    fn tailscale_range() {
        use super::net::is_tailscale;
        assert!(is_tailscale("100.64.0.1".parse().unwrap()));
        assert!(is_tailscale("100.127.255.254".parse().unwrap()));
        assert!(!is_tailscale("100.128.0.1".parse().unwrap()));
        assert!(!is_tailscale("192.168.0.2".parse().unwrap()));
    }

    #[test]
    fn version_negotiation_accepts_same_or_newer() {
        assert!(compatible(VERSION));
        assert!(compatible(VERSION + 5));
        assert!(!compatible(MIN_VERSION - 1));
    }

    #[test]
    fn unknown_message_is_ignored_not_fatal() {
        assert!(decode("{\"t\":\"future_feature\",\"x\":1}").is_none());
        assert!(matches!(decode(&encode(&Msg::Leave)), Some(Msg::Leave)));
        assert!(matches!(decode("{\"t\":\"return\"}"), Some(Msg::Return { .. })));
    }
}
