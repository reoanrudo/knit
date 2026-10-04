    //! 大容量データ(ファイル・画像)専用の経路。本線(入力・制御の JSON Lines)と
    //! 別の TCP 接続に分けることで、転送中もマウス・キー・ping が詰まらない
    //! (旧方式は 3MB チャンクが入力と同じキューとソケットに並び、転送中は入力が止まり、
    //! 転送が 10 秒を超えると pong 遅延で切断と誤判定された)。
    //! フレーム: [種別 u8][長さ u32 LE][本体]。base64 を使わない(33% 増と変換 CPU の削減)
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};
    use blake2::Digest;

    /// 本線ポートからの差分(24900 → 24902。24901 は音声)
    pub const PORT_OFFSET: u16 = 2;
    /// ファイル指紋の再公開(利用側は bulk 経由で呼ぶことが多いため)
    pub use crate::files::key as files_key;
    /// 暗号化ハンドシェイクで経路を識別するラベル
    const LABEL: &[u8] = b"knit-bulk";
    /// 1 フレームの本体上限(DATA は CHUNK、その他は小さなメタ情報のみ)
    pub const MAX_FRAME: usize = 1024 * 1024;
    /// ファイル・画像データを分割する単位
    pub const CHUNK: usize = 256 * 1024;
    /// 1 回の一括送信の合計上限
    pub const MAX_TOTAL: u64 = crate::files::MAX_FILE;
    /// UI の表示も実際の制限値から作る(バイト数は 2 進単位)
    pub fn file_limit_label() -> String {
        format!("{}GiB", MAX_TOTAL / (1024 * 1024 * 1024))
    }
    /// バイト数を GiB 表記へ整形する(上限通知で使う。例: 10GiB・14.2GiB)。
    /// ちょうどの整数は ".0" を付けない(上限値の表記と揃える)
    pub fn human_gib(n: u64) -> String {
        let v = n as f64 / (1024.0 * 1024.0 * 1024.0);
        if (v - v.round()).abs() < 0.05 {
            format!("{}GiB", v.round() as u64)
        } else {
            format!("{v:.1}GiB")
        }
    }
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
    pub const DROP_ID_BEGIN: u8 = 9;

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
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "frame too large",
            ));
        }
        buf.resize(len, 0);
        r.read_exact(buf)?;
        Ok(head[0])
    }

    pub fn total_size(paths: &[PathBuf]) -> u64 {
        paths
            .iter()
            .filter_map(|p| {
                std::fs::metadata(p)
                    .ok()
                    .filter(|m| m.is_file())
                    .map(|m| m.len())
            })
            .fold(0, u64::saturating_add)
    }

    /// 送信 1 件(フォルダ展開後)。name は宛先での相対名(フォルダ内は dir/sub/file)
    #[derive(Clone, Debug)]
    pub struct OutFile {
        pub src: PathBuf,
        pub name: String,
        pub size: u64,
        /// 更新日時(unix 秒。0=未指定)。版 14 以降の受信側で復元される
        pub mtime: u64,
        /// 空フォルダ(版 15 以降)。受信側でディレクトリとして再現する
        pub is_dir: bool,
    }

    fn mtime_of(meta: &std::fs::Metadata) -> u64 {
        meta.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// パス群を送信用へ展開する。フォルダは中のファイル群を相対パス付きで列挙する
    ///(シンボリックリンクは辿らない。循環と意図しない外部参照を防ぐ)。
    /// strict=true は読めない項目をエラーにする(掴みドラッグは合計件数・容量が
    /// 事前の予告と一致しなければならないため)。allow_dirs=false は旧版相手向けに
    /// フォルダを拒否する。keep_empty_dirs=true(版 15 以降の相手)は空フォルダも
    /// ディレクトリ印付きで列挙し、受け側でフォルダ構造をそのまま再現する
    pub fn collect(
        paths: &[PathBuf],
        strict: bool,
        allow_dirs: bool,
        keep_empty_dirs: bool,
    ) -> std::io::Result<Vec<OutFile>> {
        collect_with_skips(paths, strict, allow_dirs, keep_empty_dirs).map(|r| r.entries)
    }

    /// collect の結果(展開後の項目と、読めずに除外した項目)。skipped は
    /// 呼び出し側が「N 件中 M 件をスキップ」の通知と履歴除外に使う
    #[derive(Debug, Default)]
    pub struct CollectResult {
        pub entries: Vec<OutFile>,
        pub skipped: Vec<PathBuf>,
    }

    /// strict=false のとき、フォルダ走査中の 1 項目の読み取り失敗でバッチ全体を
    /// 落とさずに skipped へ積む版の collect(トップレベルだけではなくフォルダ内も)
    pub fn collect_with_skips(
        paths: &[PathBuf],
        strict: bool,
        allow_dirs: bool,
        keep_empty_dirs: bool,
    ) -> std::io::Result<CollectResult> {
        fn reject(msg: &str) -> std::io::Error {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, msg)
        }
        let mut out: Vec<OutFile> = Vec::new();
        let mut total = 0u64;
        let mut skipped: Vec<PathBuf> = Vec::new();
        /// 1 つのディレクトリを走査する。戻り値は「このサブツリーから列挙した件数」。
        /// 0 件だったサブディレクトリは空フォルダとして扱う(keep_empty_dirs のとき)
        fn walk(
            src: &Path,
            prefix: &str,
            out: &mut Vec<OutFile>,
            total: &mut u64,
            keep_empty_dirs: bool,
            strict: bool,
            skipped: &mut Vec<PathBuf>,
        ) -> std::io::Result<usize> {
            if out.len() > crate::drag::MAX_BATCH_FILES {
                // 実際の値と上限をメッセージへ残す(通知が「何が・どれだけ・どの上限に」
                // 引っかかったかを伝える材料。走査はここで打ち切るため「以上」になる)
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "too many files in selection: {} (limit {})",
                        out.len(),
                        crate::drag::MAX_BATCH_FILES
                    ),
                ));
            }
            let mut names: Vec<(std::ffi::OsString, std::fs::FileType)> = Vec::new();
            let rd = match std::fs::read_dir(src) {
                Ok(rd) => rd,
                // strict=false ではフォルダ自体が読めなくてもバッチ全体を止めない
                Err(_) if !strict => {
                    skipped.push(src.to_path_buf());
                    return Ok(0);
                }
                Err(e) => return Err(e),
            };
            for entry in rd {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) if !strict => continue,
                    Err(e) => return Err(e),
                };
                let ft = match entry.file_type() {
                    Ok(f) => f,
                    Err(_) if !strict => continue,
                    Err(e) => return Err(e),
                };
                names.push((entry.file_name(), ft));
            }
            names.sort_by(|a, b| a.0.cmp(&b.0)); // 展開順を安定させる(表示・試験の一貫性)
            let mut emitted = 0usize;
            for (name, ft) in names {
                let child = src.join(&name);
                if ft.is_symlink() {
                    // リンクは辿らない・送らない(既存の安全設計)。ただし黙って
                    // 欠けると「送ったはずの項目が無い」になるため、strict
                    //(掴みドラッグ)でも skipped へ積んで呼び出し側から通知できるようにする
                    skipped.push(child);
                    continue;
                }
                let rel = if prefix.is_empty() {
                    name.to_string_lossy().into_owned()
                } else {
                    format!("{prefix}/{}", name.to_string_lossy().into_owned())
                };
                // 相手側で保存時に弾かれる長さの rel は、送る前に止めて skipped へ
                // 積む(フォルダは子の rel が必ず更长くなるため、サブツリーごと外す)。
                // バッチ全体をエラーに落とさない(深い階層の 1 フォルダだけの問題)
                if crate::files::utf16_len(&rel) > crate::files::MAX_REL_UNITS {
                    skipped.push(child);
                    continue;
                }
                if ft.is_dir() {
                    let skipped_before = skipped.len();
                    let sub = walk(&child, &rel, out, total, keep_empty_dirs, strict, skipped)?;
                    // 本当に空のフォルダだけをディレクトリ印で送る。
                    // 読み取りに失敗した(skipped に積んだ)フォルダを空フォルダと
                    // して送ると、受け側に中身が無いものが実体化して誤解を招く
                    if sub == 0 && keep_empty_dirs && skipped.len() == skipped_before {
                        // 中身のないフォルダも送る(受け側で mkdir される)
                        out.push(OutFile {
                            src: child,
                            name: rel,
                            size: 0,
                            mtime: 0,
                            is_dir: true,
                        });
                        emitted += 1;
                    } else {
                        emitted += sub;
                    }
                } else if ft.is_file() {
                    let meta = match std::fs::metadata(&child) {
                        Ok(m) => m,
                        Err(_) if !strict => {
                            skipped.push(child);
                            continue;
                        }
                        Err(e) => return Err(e),
                    };
                    *total = total.saturating_add(meta.len());
                    out.push(OutFile {
                        src: child,
                        name: rel,
                        size: meta.len(),
                        mtime: mtime_of(&meta),
                        is_dir: false,
                    });
                    emitted += 1;
                }
            }
            Ok(emitted)
        }
        for p in paths {
            let meta = match std::fs::metadata(p) {
                Ok(m) => m,
                Err(e) if strict => return Err(e),
                Err(_) => {
                    skipped.push(p.clone());
                    continue;
                }
            };
            if meta.is_dir() {
                if !allow_dirs {
                    if strict {
                        return Err(reject("folders are not supported by the peer"));
                    }
                    continue;
                }
                let Some(base) = p.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                    if strict {
                        return Err(reject("unnamed folder"));
                    }
                    continue;
                };
                // フォルダ名だけでも上限超過なら、受信側で保存時に弾かれるため
                // 送る前に skipped へ回す(walk 内のチェックは子の rel 基準)。
                // symlink と同じく strict(掴みドラッグ)でも skipped に積み、
                // 呼び出し側の「N 件をスキップ」通知で伝える
                if crate::files::utf16_len(&base) > crate::files::MAX_REL_UNITS {
                    skipped.push(p.clone());
                    continue;
                }
                let emitted = walk(p, &base, &mut out, &mut total, keep_empty_dirs, strict, &mut skipped)?;
                // フォルダ全体が空のときも(中身が空でも)フォルダ自体を送れる
                if emitted == 0 && keep_empty_dirs {
                    out.push(OutFile {
                        src: p.clone(),
                        name: base,
                        size: 0,
                        mtime: 0,
                        is_dir: true,
                    });
                }
            } else if meta.is_file() {
                let Some(name) = p.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                    if strict {
                        return Err(reject("unnamed file"));
                    }
                    continue;
                };
                // 受信側で保存時に弾かれる長さの名前は、送る前に skipped へ回す
                //(フォルダと同じく strict でも通知で伝える)
                if crate::files::utf16_len(&name) > crate::files::MAX_REL_UNITS {
                    skipped.push(p.clone());
                    continue;
                }
                total = total.saturating_add(meta.len());
                out.push(OutFile {
                    src: p.clone(),
                    name,
                    size: meta.len(),
                    mtime: mtime_of(&meta),
                    is_dir: false,
                });
            } else if strict {
                return Err(reject("unsupported entry type"));
            }
            if out.len() > crate::drag::MAX_BATCH_FILES {
                return Err(reject(&format!(
                    "too many files in selection: {} (limit {})",
                    out.len(),
                    crate::drag::MAX_BATCH_FILES
                )));
            }
            if total > MAX_TOTAL {
                return Err(reject(&format!(
                    "file batch exceeds size limit: {total} (limit {MAX_TOTAL})"
                )));
            }
        }
        if total > MAX_TOTAL {
            return Err(reject(&format!(
                "file batch exceeds size limit: {total} (limit {MAX_TOTAL})"
            )));
        }
        Ok(CollectResult { entries: out, skipped })
    }

    /// 展開後の合計サイズ(表示・予告用)
    pub fn entries_total(entries: &[OutFile]) -> u64 {
        entries.iter().map(|e| e.size).fold(0, u64::saturating_add)
    }

    /// 最終パスのファイル名が相手の指定名と違う(= 同名衝突で「名前 (n)」へ
    /// ずれた)か。リネーム件数の集計に使う
    fn is_renamed(rel: &str, final_path: &Path) -> bool {
        let want = rel.rsplit('/').next().unwrap_or(rel);
        let got = final_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        got != want
    }

    /// 一括送信の結果。skipped は読み取れずに送らなかった項目のパス
    ///(ドラッグ以外の経路では、壊れた 1 件でバッチ全体を止めないために黙って
    /// 除外される。呼び出し側が「N 件中 M 件をスキップ」を通知し、履歴からも除く)
    #[derive(Debug, Default)]
    pub struct SendReport {
        pub sent: usize,
        pub skipped: Vec<PathBuf>,
    }

    /// 送信エラーを利用者向けの短い理由へ分類する(通知の文言分けに使う)。
    /// 上限超過(件数・容量)は「何が・どれだけ・どの上限に」引っかかったかを
    /// 実際の値付きで返す(例: 「展開後 640 件以上(上限 512 件)」「合計 14.2GiB(上限 10GiB)」)
    pub fn send_error_label(e: &std::io::Error) -> String {
        use std::io::ErrorKind::*;
        match e.kind() {
            // 中止は接続切断として伝播する設計のため、直後の再送がこの窓で失敗
            // しやすい。回復手段を文言に含める
            NotConnected => {
                "ファイル転送経路が未接続(接続の回復を待ってから再送してください)".into()
            }
            PermissionDenied => "ファイルの共有が許可されていません".into(),
            UnexpectedEof => "転送中にファイルが変わりました".into(),
            Interrupted => "中止しました".into(),
            // collect は複数の理由に InvalidInput を使うため、フォルダ未対応は
            // メッセージで区別する(旧版相手へのフォルダ送信は通常運用で起きる)
            InvalidInput if e.to_string().contains("folders are not supported") => {
                "相手のアプリが古く、フォルダの受け渡しに対応していません(ファイル単体を選んでください)".into()
            }
            InvalidInput => {
                let msg = e.to_string();
                match limit_values(&msg) {
                    // collect は上限超過で走査を打ち切るため、件数は「以上」になる
                    Some((actual, limit)) if msg.starts_with("too many files") => {
                        format!("展開後 {actual} 件以上(上限 {limit} 件)")
                    }
                    Some((actual, limit)) => {
                        // 上限を超えているのに human_gib の丸めで同表記(例: 上限+数
                        // バイトが「10GiB」)になると「合計 10GiB(上限 10GiB)」と
                        // 意味をなさないため、超過時は小数第1位を強制する
                        let actual_label = if actual > limit {
                            format!("{:.1}GiB", actual as f64 / (1024.0 * 1024.0 * 1024.0))
                        } else {
                            human_gib(actual)
                        };
                        format!("合計 {actual_label}(上限 {})", human_gib(limit))
                    }
                    None => format!(
                        "件数または容量が上限を超えています(1回は展開後 {} 件・合計 {} まで)",
                        crate::drag::MAX_BATCH_FILES,
                        file_limit_label()
                    ),
                }
            }
            TimedOut => "転送が時間切れになりました".into(),
            ConnectionReset | BrokenPipe | ConnectionAborted => "接続が切れました".into(),
            _ => "転送に失敗しました".into(),
        }
    }

    /// 上限超過エラーのメッセージから(実際値, 上限値)を取り出す。
    /// 形式は "…: {actual} (limit {limit})"(collect / send_entries が埋める)
    fn limit_values(msg: &str) -> Option<(u64, u64)> {
        let tail = msg.split(':').nth(1)?;
        let mut parts = tail.split("(limit");
        let actual = parts.next()?.trim().parse::<u64>().ok()?;
        let limit = parts.next()?.trim_end_matches(')').trim().parse::<u64>().ok()?;
        Some((actual, limit))
    }

    /// 展開済みのファイル群を送る本体。cancel は毎チャンク、progress は
    /// (送信済みバイト, 宣言合計, 現在のファイル名) をチャンク毎に呼ぶ。
    /// 中止はエラー(Interrupted)として返り、呼び出し元で接続を捨てて相手側の
    /// 切断時クリーンアップに委ねる。戻り値は送信結果(件数とスキップ)
    #[allow(clippy::too_many_arguments)]
    pub fn send_entries(
        w: &mut impl Write,
        entries: &[OutFile],
        drop: bool,
        drag_id: Option<u64>,
        cancel: &mut dyn FnMut() -> bool,
        progress: &mut dyn FnMut(u64, u64, &str),
    ) -> std::io::Result<SendReport> {
        if !crate::share::allow_files() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "file sharing is disabled on this device",
            ));
        }
        let declared = entries_total(entries);
        if declared > MAX_TOTAL {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("file batch exceeds size limit: {declared} (limit {MAX_TOTAL})"),
            ));
        }
        if drag_id.is_some() && cancel() {
            return Err(cancelled_err());
        }
        if let Some(id) = drag_id {
            write_frame(w, DROP_ID_BEGIN, &id.to_le_bytes())?;
        } else if drop {
            write_frame(w, DROP_BEGIN, &[])?;
        }
        let mut buf = vec![0u8; CHUNK];
        let mut report = SendReport::default();
        let mut sent_bytes = 0u64;
        for e in entries {
            if cancel() {
                return Err(cancelled_err());
            }
            progress(sent_bytes, declared, &e.name);
            if e.is_dir {
                // 空フォルダ。データは無く、受け側がディレクトリを作る(版 15 以降)
                let head = serde_json::json!({ "name": e.name, "size": 0, "mtime": 0, "dir": 1 })
                    .to_string();
                write_frame(w, FILE_BEGIN, head.as_bytes())?;
                write_frame(w, FILE_END, &[])?;
                report.sent += 1;
                continue;
            }
            let mut f = match std::fs::File::open(&e.src) {
                Ok(file) => file,
                Err(error) if drag_id.is_some() => return Err(error),
                Err(error) => {
                    eprintln!("[bulk] 読めずスキップ: {} ({error})", e.name);
                    report.skipped.push(e.src.clone());
                    continue;
                }
            };
            let size = e.size;
            // 事前検査の後にファイルが変わっていても、宣言サイズ(collect 時)を送る。
            // 縮んでいれば読み取りが EOF に当たり失敗に、増えても宣言分だけ送る
            if size > crate::files::MAX_FILE || size > MAX_TOTAL - sent_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "file batch exceeds size limit: {} (limit {MAX_TOTAL})",
                        sent_bytes.saturating_add(size)
                    ),
                ));
            }
            let head = serde_json::json!({ "name": e.name, "size": size, "mtime": e.mtime })
                .to_string();
            write_frame(w, FILE_BEGIN, head.as_bytes())?;
            let mut hasher = blake2::Blake2s256::new();
            let mut remain = size;
            while remain > 0 {
                let n = f.read(&mut buf[..(remain.min(CHUNK as u64) as usize)])?;
                if n == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "file shortened during transfer",
                    ));
                }
                hasher.update(&buf[..n]);
                write_frame(w, DATA, &buf[..n])?;
                remain -= n as u64;
                sent_bytes += n as u64;
                progress(sent_bytes, declared, &e.name);
                if cancel() {
                    return Err(cancelled_err());
                }
            }
            // 内容ハッシュ(BLAKE2s 256bit)を FILE_END に載せる。旧版受信側は
            // body を無視し、新版はこれで受信結果を検証する
            let digest: [u8; 32] = hasher.finalize().into();
            write_frame(w, FILE_END, &digest)?;
            report.sent += 1;
        }
        write_frame(w, BATCH_END, &[])?;
        if drop {
            write_frame(w, DROP_END, &[])?;
        }
        w.flush()?;
        Ok(report)
    }

    fn cancelled_err() -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::Interrupted, "transfer cancelled")
    }

    /// ファイル群を送る。drop=true は「掴んだまま境界越え」(受信側は OLE ドラッグで渡す)。
    /// 戻り値は送信結果(件数とスキップ)。on_progress は (送信済みバイト, 宣言済み合計) がチャンク毎に呼ばれる
    pub fn send_files_with_progress(
        w: &mut impl Write,
        paths: &[PathBuf],
        drop: bool,
        mut on_progress: impl FnMut(u64, u64),
    ) -> std::io::Result<SendReport> {
        let entries = collect(paths, false, false, false)?;
        send_entries(
            w,
            &entries,
            drop,
            None,
            &mut || false,
            &mut |sent, total, _| on_progress(sent, total),
        )
    }

    /// ファイル群を送る(進捗不要版)
    pub fn send_files(w: &mut impl Write, paths: &[PathBuf], drop: bool) -> std::io::Result<SendReport> {
        let entries = collect(paths, false, false, false)?;
        send_entries(w, &entries, drop, None, &mut || false, &mut |_, _, _| {})
    }

    /// 掴みドラッグのファイルを送る(旧版互換の入口: フォルダは拒否する)。
    /// フォルダを含むドラッグは呼び出し側で版を確認の上 collect + send_entries を使う
    pub fn send_drag_files(
        w: &mut impl Write,
        paths: &[PathBuf],
        id: u64,
        cancelled: impl Fn() -> bool,
    ) -> std::io::Result<SendReport> {
        if id == 0
            || paths.is_empty()
            || paths.len() > crate::drag::MAX_FILES
            || paths.iter().any(|p| !p.is_file())
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unsupported drag files",
            ));
        }
        let entries = collect(paths, true, false, false)?;
        send_entries(w, &entries, true, Some(id), &mut || cancelled(), &mut |_, _, _| {})
    }

    /// クリップボード画像(DIB)を送る
    pub fn send_image(w: &mut impl Write, dib: &[u8]) -> std::io::Result<()> {
        if !crate::share::allow_clip() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "clipboard sharing is disabled on this device",
            ));
        }
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
        /// 一括送信が完了した(drop=true は掴みドラッグ)。
        /// failed は保存できなかったファイル(名前, 理由)。途中の切断では
        /// このイベント自体が来ないため、failed に入るのはバッチを完走した分だけ。
        /// renamed は同名衝突で「名前 (n)」として保存した件数(通知の案内用)。
        /// denied はこの端末の共有設定で受け取りを拒否したファイル数(版 15 以降。
        /// 呼び出し側は相手へ XferAck で知らせる)
        Files {
            paths: Vec<PathBuf>,
            drop: bool,
            drag_id: Option<u64>,
            failed: Vec<(String, String)>,
            renamed: usize,
            denied: usize,
        },
        /// バッチ全体がこの端末の共有設定で拒否された(files=false)。
        /// 受け側は何も保存していない。呼び出し側は相手へ XferAck で知らせる
        Denied { files: usize },
        /// バッチの途中で切断・中止された(Receiver の Drop で発火)。
        /// saved は FILE_END まで完了して保存済みの件数。ドラッグ経路は
        /// Drop 時に全削除されるため発火しない
        Interrupted { saved: usize },
        Image(Vec<u8>),
    }

    enum Sink {
        None,
        File {
            f: std::fs::File,
            remain: u64,
            /// 書き込み中の一時ファイル。完了時に最終名へ原子的にリネームする
            tmp: PathBuf,
            /// 無害化済みの相対名(失敗報告・ログ用)
            rel: String,
            mtime: u64,
            hasher: blake2::Blake2s256,
        },
        Image { data: Vec<u8>, remain: usize },
    }

    /// 受信側の状態機械。フレームを順に与えると完了時に Event を返す。
    /// ファイルは一時名へ書き、FILE_END で内容ハッシュを検証してから最終名へ
    /// 公開する(不完全な実体が受信フォルダに見える時間を作らない)
    pub struct Receiver {
        dir: PathBuf,
        sink: Sink,
        paths: Vec<PathBuf>,
        drop: bool,
        drag_id: Option<u64>,
        /// 一括転送内の累積書き込みバイト。正常な BATCH_END でリセットする。
        /// 接続の寿命で累積すると、正当な連続転送も過去の転送量で拒否されてしまう。
        written: u64,
        limit: u64,
        /// 上限超過後はバッチを問わず受け付けない(FILE_BEGIN の連打で上限を
        /// 回避できないようにする)
        tainted: bool,
        /// このバッチで受け付けた FILE_BEGIN の件数(0 バイトファイル・空フォルダの
        /// 連打によるディスクエントリ・メモリの枯渇を防ぐ。BATCH_END でリセット)
        batch_files: usize,
        /// ファイル群・画像のバッチが進行中か(BATCH_END/IMAGE_END で閉じる)。
        /// 端末切替時の経路張替延期(受信中は切らない)と履歴ラベルの固定に使う
        batch_open: bool,
        /// 保存できなかったファイル(名前, 理由)。BATCH_END で UI へ報告する
        failed: Vec<(String, String)>,
        /// 同名衝突で「名前 (n)」へ保存した件数(BATCH_END で UI へ報告する)
        renamed: usize,
        /// バッチの途中で切断(Drop)したときの部分報告先(spawn_reader が
        /// Endpoint の on_event を渡す。保存済み件数を Event::Interrupted で伝える)
        on_interrupted: Option<fn(Event)>,
        /// 受信バイト数の通知(進捗表示。バッチ累計)
        rx_hook: Option<fn(u64)>,
    }

    impl Receiver {
        pub fn new(dir: &Path) -> Self {
            Self {
                dir: dir.to_path_buf(),
                sink: Sink::None,
                paths: Vec::new(),
                drop: false,
                drag_id: None,
                written: 0,
                limit: MAX_TOTAL,
                tainted: false,
                batch_files: 0,
                batch_open: false,
                failed: Vec::new(),
                renamed: 0,
                on_interrupted: None,
                rx_hook: None,
            }
        }

        /// 受信バイト毎に呼ぶ進捗通知を付ける(接続側スレッドで呼ばれるため軽く保つ)
        pub fn with_rx_progress(mut self, f: fn(u64)) -> Self {
            self.rx_hook = Some(f);
            self
        }

        /// バッチの途中で切断・中止されたときに呼ぶフックを付ける(Drop から
        /// Event::Interrupted { saved } を渡す。保存済みファイルが残る経路の
        /// 「一部だけ届いた」を通知で可視化する)
        pub fn with_interrupted_report(mut self, f: fn(Event)) -> Self {
            self.on_interrupted = Some(f);
            self
        }

        /// 上限超過等でこの接続の受信を拒否している状態か(ログ・通知の判定用)
        pub fn is_tainted(&self) -> bool {
            self.tainted
        }

        /// ファイル群・画像のバッチが進行中か。進行中に bulk 経路を切ると
        /// FILE_END までの分しか残らないため、切替側が張替を延期する判断に使う
        pub fn batch_open(&self) -> bool {
            self.batch_open
        }

        /// テスト用: 上限を縮小する
        #[cfg(test)]
        pub fn set_limit_for_test(&mut self, v: u64) {
            self.limit = v;
        }

        /// 書き込み途中のファイルを破棄する(Drop・境界から呼ぶ。完了済みは残す)。
        /// reason が空でなければ失敗報告へ積む
        fn discard_open_file(&mut self, reason: &str) {
            if let Sink::File { f, tmp, rel, .. } = std::mem::replace(&mut self.sink, Sink::None) {
                drop(f); // Windows でも削除できるよう、ハンドルを先に閉じる
                let _ = std::fs::remove_file(&tmp);
                self.paths.pop(); // 予約済みの最終名も取り下げる
                if !reason.is_empty() {
                    self.failed.push((rel, reason.to_string()));
                }
            }
        }

        pub fn feed(&mut self, kind: u8, body: &[u8]) -> Option<Event> {
            match kind {
                DROP_ID_BEGIN => {
                    let id = u64::from_le_bytes(body.try_into().ok()?);
                    if id == 0
                        || !self.paths.is_empty()
                        || !matches!(self.sink, Sink::None)
                        || self.drag_id.is_some()
                    {
                        self.tainted = true;
                        return None;
                    }
                    self.drop = true;
                    self.drag_id = Some(id);
                    self.batch_open = true;
                }
                DROP_BEGIN => {
                    self.drop = true;
                    self.batch_open = true;
                }
                FILE_BEGIN => {
                    self.discard_open_file("");
                    self.batch_open = true;
                    if self.tainted {
                        return None;
                    }
                    // 件数上限(送信側と同じ上限)。0 バイト・空フォルダも数える
                    self.batch_files += 1;
                    if self.batch_files > crate::drag::MAX_BATCH_FILES {
                        self.tainted = true;
                        return None;
                    }
                    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
                    let name = v["name"].as_str().unwrap_or("file");
                    let size = v["size"].as_u64().unwrap_or(0);
                    let mtime = v["mtime"].as_u64().unwrap_or(0);
                    // 空フォルダ(版 15 以降)。ディレクトリとして作って公開する。
                    // DATA は続かない(送信側が出さない)
                    if v["dir"].as_u64() == Some(1) {
                        let fail = |me: &mut Self, name: &str, reason: &str| {
                            me.failed.push((name.to_string(), reason.to_string()));
                        };
                        // 上限超過とそれ以外(空コンポーネント・遷移等)で理由を分ける
                        let too_long =
                            crate::files::utf16_len(name) > crate::files::MAX_REL_UNITS;
                        let Some(rel) = crate::files::sanitize_rel(name) else {
                            fail(
                                self,
                                name,
                                if too_long {
                                    "ファイル名またはパスが長すぎる"
                                } else {
                                    "無効なフォルダ名"
                                },
                            );
                            return None;
                        };
                        let Some(final_path) =
                            crate::files::unique_final(&self.dir, &rel, &self.paths)
                        else {
                            fail(self, &rel, "保存名の衝突を解消できません");
                            return None;
                        };
                        match std::fs::create_dir_all(&final_path) {
                            Ok(()) => {
                                if is_renamed(&rel, &final_path) {
                                    self.renamed += 1;
                                }
                                self.paths.push(final_path);
                            }
                            Err(_) => fail(self, &rel, "フォルダを作成できません"),
                        }
                        return None;
                    }
                    // 書いてから超過分を消す方式では、一時に上限を超えてしまう。
                    if size > crate::files::MAX_FILE
                        || size > self.limit.saturating_sub(self.written)
                    {
                        self.tainted = true;
                        return None;
                    }
                    let fail = |me: &mut Self, name: &str, reason: &str| {
                        me.failed.push((name.to_string(), reason.to_string()));
                    };
                    // 上限超過(正当だが長すぎる)と不正な名前(遷移・空部等)で理由を
                    // 分ける。「無効なファイル名」だと正当な長い名前の対処を邪魔する
                    let too_long =
                        crate::files::utf16_len(name) > crate::files::MAX_REL_UNITS;
                    let Some(rel) = crate::files::sanitize_rel(name) else {
                        fail(
                            self,
                            name,
                            if too_long {
                                "ファイル名またはパスが長すぎる"
                            } else {
                                "無効なファイル名"
                            },
                        );
                        return None; // このファイルの DATA は空読みし、次へ続ける
                    };
                    // 空き容量(余裕 1MiB)。取得できない環境は確認なしで続ける
                    if let Some(avail) = crate::files::available_bytes(&self.dir) {
                        if avail.saturating_sub(1024 * 1024) < size {
                            fail(self, &rel, "保存先の空き容量が不足");
                            return None;
                        }
                    }
                    let Some(final_path) =
                        crate::files::unique_final(&self.dir, &rel, &self.paths)
                    else {
                        fail(self, &rel, "保存名の衝突を解消できません");
                        return None;
                    };
                    let parent = final_path.parent().unwrap_or(&self.dir).to_path_buf();
                    let Some((f, tmp)) = crate::files::create_temp(&parent) else {
                        fail(self, &rel, "一時ファイルを作成できません");
                        return None;
                    };
                    self.paths.push(final_path);
                    self.sink = Sink::File {
                        f,
                        remain: size,
                        tmp,
                        rel,
                        mtime,
                        hasher: blake2::Blake2s256::new(),
                    };
                }
                DATA => match &mut self.sink {
                    Sink::File {
                        f,
                        remain,
                        hasher,
                        ..
                    } => {
                        // 宣言サイズを超える・書けないデータは、そのファイルごと破棄する
                        if body.len() as u64 > self.limit.saturating_sub(self.written) {
                            self.tainted = true;
                            self.discard_open_file("転送上限を超過");
                        } else if body.len() as u64 > *remain || f.write_all(body).is_err() {
                            self.discard_open_file("書き込みに失敗");
                        } else {
                            *remain -= body.len() as u64;
                            self.written += body.len() as u64;
                            hasher.update(body);
                            if let Some(hook) = self.rx_hook {
                                hook(self.written);
                            }
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
                    // 途中で切れた(宣言サイズに満たない)ファイルは残さない。
                    // 完走したら内容ハッシュを検証し、通れば最終名へ原子的に公開する
                    if let Sink::File {
                        f,
                        remain,
                        tmp,
                        rel,
                        mtime,
                        hasher,
                    } = std::mem::replace(&mut self.sink, Sink::None)
                    {
                        if remain > 0 {
                            drop(f);
                            let _ = std::fs::remove_file(&tmp);
                            self.paths.pop(); // 予約していた最終名も取り下げる
                            self.failed.push((rel, "完全な形で届きませんでした".into()));
                        } else {
                            let digest: [u8; 32] = hasher.finalize().into();
                            let hash_ok = body.len() != 32 || digest.as_slice() == body;
                            drop(f); // リネームの前に閉じる
                            if !hash_ok {
                                let _ = std::fs::remove_file(&tmp);
                                self.paths.pop();
                                self.failed.push((rel, "内容の検証に失敗".into()));
                            } else {
                                if mtime > 0 {
                                    crate::files::set_mtime(&tmp, mtime);
                                }
                                crate::files::mark_untrusted(&tmp);
                                let desired = self.paths.last().cloned();
                                match desired.and_then(|d| crate::files::publish(&tmp, &d).ok()) {
                                    Some(actual) => {
                                        // 同名衝突で「名前 (n)」へずれたら通知用に数える
                                        if is_renamed(&rel, &actual) {
                                            self.renamed += 1;
                                        }
                                        if let Some(last) = self.paths.last_mut() {
                                            *last = actual;
                                        }
                                    }
                                    None => {
                                        let _ = std::fs::remove_file(&tmp);
                                        self.paths.pop();
                                        self.failed.push((rel, "保存を完了できません".into()));
                                    }
                                }
                            }
                        }
                    }
                }
                BATCH_END => {
                    // FILE_END が無いファイルは完了として公開しない。
                    self.discard_open_file("");
                    self.batch_open = false;
                    if !self.tainted {
                        self.written = 0;
                        self.batch_files = 0;
                    }
                    let paths = std::mem::take(&mut self.paths);
                    let failed = std::mem::take(&mut self.failed);
                    let renamed = std::mem::take(&mut self.renamed);
                    let drop = std::mem::replace(&mut self.drop, false);
                    let drag_id = self.drag_id.take();
                    // 受信失敗で0件でも、操作IDを返して準備待ちを取り消せるようにする。
                    // denied(範囲外で捨てた件数)は受信ループが上書きして渡す
                    if !paths.is_empty() || drag_id.is_some() || !failed.is_empty() {
                        return Some(Event::Files {
                            paths,
                            drop,
                            drag_id,
                            failed,
                            renamed,
                            denied: 0,
                        });
                    }
                }
                IMAGE_BEGIN => {
                    self.discard_open_file("");
                    let n = u64::from_le_bytes(body.try_into().ok()?) as usize;
                    if n == 0 || n > MAX_IMAGE {
                        return None;
                    }
                    self.batch_open = true;
                    self.sink = Sink::Image {
                        data: Vec::with_capacity(n),
                        remain: n,
                    };
                }
                IMAGE_END => {
                    self.batch_open = false;
                    if let Sink::Image { data, remain: 0 } =
                        std::mem::replace(&mut self.sink, Sink::None)
                    {
                        return Some(Event::Image(data));
                    }
                }
                _ => {}
            }
            None
        }
    }

    impl Drop for Receiver {
        /// 切断(接続断・中止)時に書き込み途中のファイルが受信フォルダへ残留するのを
        /// 防ぐ。完了済み(BATCH_END を迎えた)ファイルは保持する
        fn drop(&mut self) {
            if !matches!(self.sink, Sink::None) {
                self.discard_open_file("");
            }
            if self.drag_id.is_some() {
                for p in self.paths.drain(..) {
                    // 空フォルダ(版 15)もキャンセル時には片付ける
                    if std::fs::remove_file(&p).is_err() {
                        let _ = std::fs::remove_dir_all(&p);
                    }
                }
            } else if !self.paths.is_empty() {
                // ドラッグ以外の中断(⌘C 送信中の切断・中止): FILE_END まで完了した
                // 分は保持されるが、BATCH_END を迎えていないため完了通知は出ない。
                // 保存済み件数を部分報告して「一部だけ届いた」を黙らせない
                let saved = self.paths.len();
                if let Some(report) = self.on_interrupted {
                    report(Event::Interrupted { saved });
                }
                eprintln!("[bulk] 未完了の転送がありました(残った受信ファイル: {saved} 件)");
            }
        }
    }

    /// 大容量経路の送信口。接続が張り替わるたびに差し替える。
    /// 送信中はロックを保持するため、複数の転送のフレームが混ざらない
    pub struct Link {
        /// (世代, 書き込み口)。世代は張り替えのたびに増え、古い受信スレッドが
        /// 新しい接続を誤って外さないための照合に使う
        w: std::sync::Mutex<Option<(u64, crate::secure::Writer)>>,
        gen: std::sync::atomic::AtomicU64,
        /// 切断用の複製ハンドル(世代付き)。send が w のロックを長時間保持して
        /// いても、これを先に切ることでロック待ちせず即座に切断できる
        sd: std::sync::Mutex<Option<(u64, std::net::TcpStream)>>,
    }

    impl Default for Link {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Link {
        pub const fn new() -> Self {
            Self {
                w: std::sync::Mutex::new(None),
                gen: std::sync::atomic::AtomicU64::new(0),
                sd: std::sync::Mutex::new(None),
            }
        }
        fn slot(&self) -> std::sync::MutexGuard<'_, Option<(u64, crate::secure::Writer)>> {
            self.w.lock().unwrap_or_else(|e| e.into_inner())
        }
        /// 新しい接続を据える。戻り値は世代(受信スレッドの終了時に clear_if へ渡す)
        pub fn set(&self, s: crate::secure::Writer) -> u64 {
            let g = self.gen.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            // 複製に失敗しても接続は据える(その場合は旧来どおり slot のロック待ちになる)
            let handle = s.shutdown_handle().ok();
            // ロック順序は常に sd → slot(clear 系と統一し、逆順待ちを生まない)
            let mut sd = self.sd.lock().unwrap_or_else(|e| e.into_inner());
            let old = self.slot().replace((g, s));
            *sd = handle.map(|h| (g, h));
            drop(sd);
            if let Some((_, old)) = old {
                old.shutdown();
            }
            g
        }
        /// 接続を捨てる(本線の切断時など)。受信スレッドも読み出しエラーで終わる。
        /// 送信が slot のロックを長時間握っていても、先にハンドルを切ることで
        /// ロックを待たずに即座に切断できる(切られた書き込みは send のエラー処理で
        /// slot から取り除かれる)
        pub fn clear(&self) {
            let mut sd = self.sd.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((_, h)) = sd.take() {
                let _ = h.shutdown(std::net::Shutdown::Both);
            }
            if let Some((_, old)) = self.slot().take() {
                old.shutdown();
            }
        }
        /// 指定世代の接続がまだ据わっている時だけ捨てる
        pub fn clear_if(&self, gen: u64) {
            let mut sd = self.sd.lock().unwrap_or_else(|e| e.into_inner());
            if sd.as_ref().is_some_and(|(n, _)| *n == gen) {
                if let Some((_, h)) = sd.take() {
                    let _ = h.shutdown(std::net::Shutdown::Both);
                }
            }
            let mut g = self.slot();
            if g.as_ref().is_some_and(|(n, _)| *n == gen) {
                if let Some((_, old)) = g.take() {
                    old.shutdown();
                }
            }
        }
        pub fn is_up(&self) -> bool {
            self.slot().is_some()
        }
        /// ロックを待たずに接続の有無だけを知る。tap スレッドなど、進行中の
        /// 転送が slot のロックを握っている間に呼ばれても止まらない
        /// (握られている=誰かがその接続を使っている)
        pub fn is_up_fast(&self) -> bool {
            match self.w.try_lock() {
                Ok(g) => g.is_some(),
                Err(_) => true,
            }
        }
        // Reply on the authenticated socket that received the heartbeat. A
        // retired reader must never write to a replacement peer's connection.
        //
        // ロックは try_lock で取り、送信中(slot のロック保持中)は応答をスキップ
        // する: ここでブロックすると受信ループの read_frame が止まり、双方向の
        // 同時転送で逆方向のデータが吸われなくなり、相手の書き込みタイムアウト
        // (20 秒)で転送が失敗する。ロックを掴む者がいる=今まさにデータが流れて
        // いる=経路は生きているため、生存確認を急ぐ必要は無い(is_up_fast と
        // 同じ判断)。切断判定はこの応答ではなく受信側の読み出しタイムアウトと
        // 相手側の書き込み失敗で行われるため、スキップは切断扱いに波及しない
        fn reply_keepalive(&self, gen: u64) -> std::io::Result<()> {
            let mut slot = match self.w.try_lock() {
                Ok(g) => g,
                // ポイズン回復は slot() と同じ方針(ロック取得済みのまま続行)
                Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => return Ok(()),
            };
            let Some((current, writer)) = slot.as_mut() else { return Ok(()) };
            if *current != gen { return Ok(()) }
            let result = write_frame(writer, KEEPALIVE, &[]).and_then(|_| writer.flush());
            if result.is_err() {
                if let Some((_, old)) = slot.take() { old.shutdown(); }
            }
            result
        }
        /// 送信する。未接続ならエラー、書き込み失敗なら接続を捨ててエラー
        pub fn send<T>(
            &self,
            f: impl FnOnce(&mut crate::secure::Writer) -> std::io::Result<T>,
        ) -> std::io::Result<T> {
            let mut g = self.slot();
            let Some((_, s)) = g.as_mut() else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotConnected,
                    "bulk link down",
                ));
            };
            let r = f(s);
            if r.is_err() {
                if let Some((_, old)) = g.take() {
                    old.shutdown();
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
        /// 受信進捗(バッチ累計バイト)。掴みドラッグ中にユーザーが見ている
        /// 画面(主に Windows)へ進捗を出すために使う
        pub on_rx_bytes: fn(u64),
        /// ファイル群・画像の受信バッチが始まった瞬間に呼ぶ(毎回・接続側スレッドで
        /// 軽く保つ)。バッチ開始時点の状態を記録したい利用側のためのフック
        pub on_batch_begin: fn(),
        /// 待受(bind)に失敗した初回に限り呼ぶ(ユーザーへの通知用)。
        /// 本線(入力・テキスト)は生きていてファイル・画像だけが届かなくなる
        /// 状態を、ログだけでなく画面にも出すために使う
        pub on_bind_error: fn(&str),
    }

    /// このプロセスの bulk 受信でファイル群・画像のバッチが進行中か。
    /// 端末切替時の経路張替延期(受信中は clear を遅らせる)に使う。
    /// 受信スレッド(spawn_reader)だけが書き、true の間はバッチの完走が近い
    static RX_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// bulk 受信のバッチ(ファイル群・画像)が進行中か
    pub fn rx_active() -> bool {
        RX_ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 待受側の bulk 経路が今動いているか(bind に成功して accept 中か)。
    /// 診断(diagnose)が「ファイル・画像の待受」の実測として読む
    static SERVING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// 待受側の bulk 経路(ファイル・画像の受信口)が稼働中か
    pub fn serving() -> bool {
        SERVING.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 受信フレームを処理してよいか。画像はクリップボード、それ以外(ファイル・ドラッグ)はファイルの範囲
    pub(crate) fn frame_allowed(kind: u8, in_image: bool, scope: crate::share::Scope) -> bool {
        match kind {
            KEEPALIVE => true,
            IMAGE_BEGIN | IMAGE_END => scope.clip,
            DATA if in_image => scope.clip,
            _ => scope.files,
        }
    }

    fn spawn_reader(ep: &'static Endpoint, s: crate::secure::Reader, gen: u64, reply_keepalive: bool) {
        std::thread::spawn(move || {
            let mut r = std::io::BufReader::with_capacity(CHUNK + 16, s);
            let mut rx = Receiver::new(&ep.dir)
                .with_rx_progress(ep.on_rx_bytes)
                .with_interrupted_report(ep.on_event);
            let mut buf = Vec::new();
            let mut in_image = false;
            let mut last_reply = std::time::Instant::now();
            // この端末の共有設定で受け取りを拒否したファイル数(バッチ毎に集計し、
            // 完了時に相手へ XferAck で知らせる材料にする)
            let mut denied_files = 0usize;
            let mut tainted_logged = false;
            while let Ok(kind) = read_frame(&mut r, &mut buf) {
                // 共有範囲の外の受信は、ディスクへ書かず読み捨てる(接続は保つ)
                if kind == IMAGE_BEGIN {
                    in_image = true;
                }
                let allowed = frame_allowed(kind, in_image, crate::share::local());
                if kind == IMAGE_END {
                    in_image = false;
                }
                if !allowed {
                    if kind == FILE_BEGIN {
                        denied_files += 1;
                    }
                    if kind == BATCH_END {
                        // バッチ全体がこの端末の設定で拒否された。保存は 0 件。
                        // 相手が誤って「送信しました」と扱わないよう通知する
                        let denied = denied_files;
                        denied_files = 0;
                        if denied > 0 {
                            (ep.on_event)(Event::Denied { files: denied });
                        }
                    }
                    continue;
                }
                if kind == BATCH_END {
                    denied_files = 0;
                }
                // Only the server replies, so desktop clients cannot create a
                // ping-pong loop. Also confirm liveness during a long upload:
                // the sender may hold its writer lock and defer its heartbeat.
                if reply_keepalive && ((kind == KEEPALIVE && buf.is_empty())
                    || last_reply.elapsed() >= std::time::Duration::from_secs(10)) {
                    if ep.link.reply_keepalive(gen).is_err() { break; }
                    last_reply = std::time::Instant::now();
                }
                if let Some(e) = rx.feed(kind, &buf) {
                    let e = match e {
                        // 範囲外で捨てた分(運用上は転送中に設定が変わった場合)を
                        // 完了イベントに添えて呼び出し側へ渡す
                        Event::Files { paths, drop, drag_id, failed, renamed, .. } => {
                            Event::Files { paths, drop, drag_id, failed, renamed, denied: denied_files }
                        }
                        other => other,
                    };
                    denied_files = 0;
                    (ep.on_event)(e);
                } else if kind == BATCH_END && denied_files > 0 {
                    // すべての FILE_BEGIN が拒否され、BATCH_END は受理された
                    //(転送中に設定が戻った場合)。保存 0 件でも受理結果を返す機会を
                    // 作らないと、相手は Ack を待ってタイムアウトする
                    let denied = denied_files;
                    denied_files = 0;
                    (ep.on_event)(Event::Denied { files: denied });
                } else if rx.is_tainted() && !tainted_logged {
                    // 上限超過の疑いで受信を拒否し始めた(この後の FILE_BEGIN は
                    // 黙って捨てられる)。旧版混在で上限解釈がずれた場合の唯一の手がかり
                    (ep.log)("[bulk] 受信を拒否しています(上限超過の疑い。相手のログを確認してください)");
                    tainted_logged = true;
                }
                // バッチ進行中の状態を公開(端末切替の経路張替延期の判断材料)。
                // 開始(false→true)の遷移だけ利用側へ知らせ、完了時点の状態を
                // 記録したい用途(履歴ラベルの固定など)に使う
                if RX_ACTIVE.swap(rx.batch_open(), std::sync::atomic::Ordering::Relaxed)
                    != rx.batch_open()
                    && rx.batch_open()
                {
                    (ep.on_batch_begin)();
                }
            }
            ep.link.clear_if(gen);
            // スレッドの寿命とバッチ進行フラグの寿命を一致させる: 切断でバッチの
            // 途中が終わった場合も、張替延期の待ちがこの後ずっと続かないように
            RX_ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
            (ep.log)("[bulk] 受信経路が切れました");
        });
    }

    /// 待受側: 認証を通った接続を送信口に据え、受信スレッドを起こす。
    /// bind に失敗しても諦めない: 本線(入力・テキスト)は生きていてファイル・
    /// 画像だけが届かない状態が無通知で続くのを防ぐため、一定間隔で再試行する
    pub fn serve(
        ep: &'static Endpoint,
        bind: &str,
        port: u16,
        allow: fn(std::net::IpAddr) -> bool,
    ) {
        /// bind の再試行間隔。短すぎるとログとCPUの無駄、長すぎると占有解除後の
        /// 復帰が遅れる。connect_loop(2秒)より長めの 5 秒にする
        const BIND_RETRY_SECS: u64 = 5;
        let mut bind_failed_logged = false;
        let listener = loop {
            match std::net::TcpListener::bind((bind, port)) {
                Ok(l) => {
                    if bind_failed_logged {
                        (ep.log)(&format!(
                            "[bulk] listen {bind}:{port} の再試行に成功しました"
                        ));
                    }
                    break l;
                }
                Err(e) => {
                    // 失敗の詳細と通知は初回だけ(以降は黙って再試行し、ログを
                    // 洗い流さない)。成功時に「再試行に成功」を出して復帰を見せる
                    if !bind_failed_logged {
                        bind_failed_logged = true;
                        (ep.log)(&format!(
                            "[bulk] listen {bind}:{port} 失敗: {e}(ファイル・画像の転送は不可。{BIND_RETRY_SECS}秒毎に再試行します)"
                        ));
                        (ep.on_bind_error)(&format!(
                            "ファイル転送を開始できません(ポート {port} が使用中)。再試行しています"
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_secs(BIND_RETRY_SECS));
                }
            }
        };
        SERVING.store(true, std::sync::atomic::Ordering::Relaxed);
        (ep.log)(&format!("[bulk] listening on {bind}:{port}"));
        let mut throttle = crate::secure::FailThrottle::new();
        for s in listener.incoming() {
            let Ok(s) = s else { continue };
            let Ok(peer) = s.peer_addr() else { continue };
            if !allow(peer.ip()) {
                (ep.log)(&format!("[bulk] rejected: {peer}"));
                std::thread::sleep(throttle.fail());
                continue;
            }
            let Ok(ctl) = s.try_clone() else { continue };
            ctl.set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .ok();
            let (r, w) = match crate::secure::accept(s, &ep.token, LABEL) {
                Ok(x) => x,
                Err(e) => {
                    (ep.log)(&format!("[bulk] handshake 失敗 ({peer}): {e}"));
                    std::thread::sleep(throttle.fail());
                    continue;
                }
            };
            // 接続側は 10 秒毎にキープアライブを送る。3 回分届かなければ死んだ経路
            ctl.set_read_timeout(Some(std::time::Duration::from_secs(35)))
                .ok();
            ctl.set_write_timeout(Some(std::time::Duration::from_secs(20)))
                .ok();
            let gen = ep.link.set(w);
            throttle.success();
            (ep.log)(&format!("[bulk] established ({peer})"));
            spawn_reader(ep, r, gen, true);
        }
    }

    /// 接続側: 本線が繋がっている間、大容量経路が無ければ張り直し、キープアライブを送る。
    /// 接続先は本線がいま使っている相手(複数経路のどれで繋がったか)に追従する
    pub fn connect_loop(
        ep: &'static Endpoint,
        addr: impl Fn() -> Option<std::net::SocketAddr>,
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
                    let _ = ep
                        .link
                        .send(|w| write_frame(w, KEEPALIVE, &[]).and_then(|_| w.flush()));
                }
                continue;
            }
            let Some(addr) = addr() else { continue };
            let Ok(s) =
                std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(3))
            else {
                continue;
            };
            crate::net::tune_tcp(&s);
            let Ok(ctl) = s.try_clone() else { continue };
            ctl.set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .ok();
            let (r, w) = match crate::secure::connect(s, &ep.token, LABEL) {
                Ok(x) => x,
                Err(e) => {
                    (ep.log)(&format!("[bulk] handshake 失敗: {e}"));
                    continue;
                }
            };
            ctl.set_read_timeout(None).ok();
            ctl.set_write_timeout(Some(std::time::Duration::from_secs(20)))
                .ok();
            let gen = ep.link.set(w);
            (ep.log)(&format!("[bulk] established (→{addr})"));
            spawn_reader(ep, r, gen, false);
        }
    }

#[cfg(test)]
mod frame_allowed_tests {
    use super::frame_allowed;
    use crate::share::Scope;

    #[test]
    fn keepalive_passes_in_any_scope() {
        for scope in [Scope::ALL, Scope::INPUT_ONLY] {
            assert!(frame_allowed(super::KEEPALIVE, false, scope));
        }
    }

    #[test]
    fn image_frames_follow_clip_and_data_follows_context() {
        let clip_only = Scope { clip: true, files: false, audio: true };
        assert!(frame_allowed(super::IMAGE_BEGIN, false, clip_only));
        assert!(frame_allowed(super::IMAGE_END, true, clip_only));
        // 画像中の DATA は clip の管轄
        assert!(frame_allowed(super::DATA, true, clip_only));
        // 画像中でも無い DATA は files の管轄(この scope では拒否)
        assert!(!frame_allowed(super::DATA, false, clip_only));
        assert!(!frame_allowed(super::IMAGE_BEGIN, false, Scope::INPUT_ONLY));
    }

    #[test]
    fn file_frames_follow_files_scope() {
        let files_only = Scope { clip: false, files: true, audio: false };
        assert!(frame_allowed(super::FILE_BEGIN, false, files_only));
        assert!(frame_allowed(super::BATCH_END, false, files_only));
        assert!(!frame_allowed(super::FILE_BEGIN, false, Scope::INPUT_ONLY));
        assert!(!frame_allowed(super::BATCH_END, false, Scope::INPUT_ONLY));
    }
}

#[cfg(test)]
mod limit_label_tests {
    use super::{human_gib, limit_values, send_error_label, MAX_TOTAL};

    #[test]
    fn limit_values_parses_actual_and_limit() {
        assert_eq!(
            limit_values("too many files in selection: 513 (limit 512)"),
            Some((513, 512))
        );
        assert_eq!(
            limit_values("file batch exceeds size limit: 15204352000 (limit 10737418240)"),
            Some((15204352000, 10737418240))
        );
        // 形式に合わないメッセージ(他の InvalidInput)は None
        assert_eq!(limit_values("folders are not supported by the peer"), None);
        assert_eq!(limit_values("unsupported entry type"), None);
    }

    #[test]
    fn oversized_labels_carry_actual_and_limit() {
        let files = std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "too many files in selection: 640 (limit 512)",
        );
        assert_eq!(send_error_label(&files), "展開後 640 件以上(上限 512 件)");
        let bytes = std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "file batch exceeds size limit: 15204352000 (limit 10737418240)",
        );
        assert_eq!(send_error_label(&bytes), "合計 14.2GiB(上限 10GiB)");
    }

    #[test]
    fn other_labels_keep_short_form() {
        let nc = std::io::Error::new(std::io::ErrorKind::NotConnected, "bulk link down");
        assert_eq!(
            send_error_label(&nc),
            "ファイル転送経路が未接続(接続の回復を待ってから再送してください)"
        );
        // 値の無い InvalidInput は従来の分類に上限の目安を添える
        let other = std::io::Error::new(std::io::ErrorKind::InvalidInput, "unsupported entry type");
        assert_eq!(
            send_error_label(&other),
            format!(
                "件数または容量が上限を超えています(1回は展開後 {} 件・合計 {} まで)",
                crate::drag::MAX_BATCH_FILES,
                super::file_limit_label()
            )
        );
    }

    #[test]
    fn human_gib_formats_compactly() {
        assert_eq!(human_gib(MAX_TOTAL), "10GiB");
        assert_eq!(human_gib(15_204_352_000), "14.2GiB");
        assert_eq!(human_gib(0), "0GiB");
    }

    /// 超過時の文言は丸めで上限と同表記にならない(「合計 10GiB(上限 10GiB)」と
    /// 意味をなさなくなるため、超過時は小数第1位を強制する)
    #[test]
    fn oversized_total_label_never_rounds_to_the_limit_label() {
        let label = |actual: u64| {
            send_error_label(&std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("file batch exceeds size limit: {actual} (limit {MAX_TOTAL})"),
            ))
        };
        // 上限+1バイト(旧: 丸めで「合計 10GiB(上限 10GiB)」になっていた)
        assert_eq!(label(MAX_TOTAL + 1), "合計 10.0GiB(上限 10GiB)");
        // 上限+0.05GiB 未満(human_gib 単体では「10GiB」へ丸まる範囲)でも強制
        let just_under_round = MAX_TOTAL + 50_000_000; // ≈ +0.047GiB
        assert_eq!(label(just_under_round), "合計 10.0GiB(上限 10GiB)");
        // 0.1GiB 超なら小数第1位で違いが見える
        let over = MAX_TOTAL + 1024 * 1024 * 1024 / 2;
        assert_eq!(label(over), "合計 10.5GiB(上限 10GiB)");
    }
}

/// keepalive 応答の try_lock 化(双方向同時転送で逆方向が止まる回帰の防止)
#[cfg(test)]
mod reply_keepalive_tests {
    use super::{Link, LABEL};
    use crate::secure;
    use std::time::{Duration, Instant};

    /// 送信中(slot のロックを誰かが握っている)に keepalive 応答を求められても、
    /// ブロックせずスキップして Ok を返すこと。従来は slot() の取得で転送終了まで
    /// 待たされ、受信ループが止まって相手の書き込みタイムアウトを誘発した
    #[test]
    fn reply_keepalive_skips_while_the_slot_lock_is_held() {
        let link = Link::new();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        // 相手側はハンドシェイクだけ済ませて読み続ける(小さな応答なら書ける)
        std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let (_r, _w) = secure::accept(sock, "ka-test", LABEL).unwrap();
            std::thread::sleep(Duration::from_secs(30));
        });
        let socket = std::net::TcpStream::connect(addr).unwrap();
        let (_reader, writer) = secure::connect(socket, "ka-test", LABEL).unwrap();
        let gen = link.set(writer);

        // 送信中を再現: slot のロックを掴んだまま応答を試みる
        let held = link.slot();
        let started = Instant::now();
        assert!(
            link.reply_keepalive(gen).is_ok(),
            "ロック保持中はスキップして Ok を返す"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "ロック待ちでブロックしてはいけない"
        );
        drop(held);

        // ロックが空いていれば従来どおり書き込める
        let started = Instant::now();
        assert!(link.reply_keepalive(gen).is_ok());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    /// ロック保持中のスキップで接続が捨てられないこと(スキップは無害な No-op)
    #[test]
    fn reply_keepalive_skip_keeps_the_link_up() {
        let link = Link::new();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let (_r, _w) = secure::accept(sock, "ka-test2", LABEL).unwrap();
            std::thread::sleep(Duration::from_secs(30));
        });
        let socket = std::net::TcpStream::connect(addr).unwrap();
        let (_reader, writer) = secure::connect(socket, "ka-test2", LABEL).unwrap();
        let gen = link.set(writer);
        let held = link.slot();
        for _ in 0..3 {
            assert!(link.reply_keepalive(gen).is_ok());
        }
        // is_up は slot のロックを取りに来るため、保持中に呼ぶと自分自身を
        // 待ってしまう(デッドロック)。ロックが取れない=誰かが使っている、を
        // 見る is_up_fast で確かめる
        assert!(
            link.is_up_fast(),
            "スキップしただけで接続が捨てられてはいけない"
        );
        drop(held);
        assert!(link.is_up());
    }
}

/// 受信バッチの進行判定(batch_open): ファイル群は BATCH_END まで・画像は
/// IMAGE_END までが「進行中」。端末切替側の経路張替延期はこの判定を信じて
/// 待つため、閉じるタイミングの回帰を防ぐ
#[cfg(test)]
mod batch_open_tests {
    use super::{Receiver, BATCH_END, DATA, FILE_BEGIN, FILE_END, IMAGE_BEGIN, IMAGE_END};
    use blake2::Digest;

    fn scratch() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "knit-batch-open-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn file_batch_stays_open_until_batch_end() {
        let dir = scratch();
        let mut rx = Receiver::new(&dir);
        assert!(!rx.batch_open(), "何も受信していなければ閉じている");
        let head = serde_json::json!({"name":"a.txt","size":3}).to_string();
        rx.feed(FILE_BEGIN, head.as_bytes());
        assert!(rx.batch_open(), "FILE_BEGIN でバッチが開く");
        rx.feed(DATA, b"abc");
        let digest: [u8; 32] = blake2::Blake2s256::digest(b"abc").into();
        rx.feed(FILE_END, &digest);
        assert!(
            rx.batch_open(),
            "FILE_END の後もバッチは継続(次のファイルが来る)"
        );
        rx.feed(BATCH_END, &[]);
        assert!(!rx.batch_open(), "BATCH_END でバッチが閉じる");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn image_batch_spans_begin_to_end() {
        let dir = scratch();
        let mut rx = Receiver::new(&dir);
        rx.feed(IMAGE_BEGIN, &(3u64.to_le_bytes()));
        assert!(rx.batch_open(), "IMAGE_BEGIN でバッチが開く");
        rx.feed(DATA, b"abc");
        rx.feed(IMAGE_END, &[]);
        assert!(!rx.batch_open(), "IMAGE_END でバッチが閉じる");
        std::fs::remove_dir_all(&dir).ok();
    }
}
