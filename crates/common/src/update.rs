//! 自動更新の検証コア。ネットワーク取得・インストールは持たず、
//! 「署名された更新情報を信用してよいか」「成果物が改変されていないか」だけを判定する。
//! 設計は docs/update-design.md。

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;

/// 更新情報の置き場(GitHub Releases の最新版)。`update.json` と `update.json.sig` を置く。
pub const MANIFEST_URL: &str =
    "https://github.com/reoanrudo/knit/releases/latest/download/update.json";

/// Android アプリ用。アプリの版はデスクトップと独立して進むため、固定タグの Release に置く
/// (`releases/latest` は別の Release を指すことがある)。
pub const ANDROID_MANIFEST_URL: &str =
    "https://github.com/reoanrudo/knit/releases/download/android-latest/update-android.json";

/// 更新情報を検証する公開鍵(1行に1本の16進。`#` 以降はコメント)。
/// 鍵の生成と保管は所有者が行い(`knit-sign keygen`)、公開鍵をこのファイルへ登録する。
/// 空の間は全ての更新情報が拒否される(未設定のまま更新が有効になる事故を防ぐ)。
/// 鍵の交代に備えて複数登録でき、いずれか1つで検証できれば有効。
const TRUSTED_KEYS_FILE: &str = include_str!("../update-keys.txt");

pub fn trusted_keys() -> Vec<[u8; 32]> {
    parse_keys(TRUSTED_KEYS_FILE)
}

fn parse_keys(text: &str) -> Vec<[u8; 32]> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .filter_map(|l| from_hex(l)?.try_into().ok())
        .collect()
}

/// 更新情報1件あたりの上限。巨大な入力でメモリを使わせない。
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum UpdateError {
    TooLarge,
    NoTrustedKey,
    BadSignature,
    BadManifest,
    WrongChannel,
    /// 現在の版以下。ダウングレード攻撃と再配信を拒否する
    NotNewer,
    InsecureUrl,
    /// 新しい版はあるが、この環境向けの成果物が無い(「最新」と誤って表示しない)
    NoArtifact,
    SizeMismatch,
    HashMismatch,
    Io,
}

impl UpdateError {
    /// 利用者向けの説明(メニューの通知にそのまま出す)
    pub fn message(&self) -> &'static str {
        match self {
            Self::TooLarge | Self::BadManifest => "更新情報の形式が不正です",
            Self::NoTrustedKey => "更新の署名鍵が未登録です",
            Self::BadSignature => "更新情報の署名を確認できませんでした。更新は行いません",
            Self::WrongChannel => "更新情報のチャンネルが一致しません",
            Self::NotNewer => "すでに最新です",
            Self::InsecureUrl => "安全でない更新元のため中止しました",
            Self::NoArtifact => "新しい版はありますが、この環境向けの更新はまだ公開されていません",
            Self::SizeMismatch | Self::HashMismatch => {
                "ダウンロードした内容が更新情報と一致しません。更新は行いません"
            }
            Self::Io => "更新ファイルを読み込めませんでした",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Artifact {
    /// 例: `macos-arm64` / `windows-x64`
    pub platform: String,
    pub url: String,
    pub size: u64,
    /// 小文字16進の SHA-256
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    /// `stable` / `beta`。取り違えた更新情報を拒否するために署名対象へ含める
    pub channel: String,
    pub version: String,
    /// 新版の通信プロトコル版。旧版の相手が残る場合に案内するための情報
    pub protocol: u32,
    pub notes_url: Option<String>,
    pub artifacts: Vec<Artifact>,
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

pub fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// `MAJOR.MINOR.PATCH`(先頭の `v` と `-pre` 以降は無視しない: 拡張付きは不正として扱う)。
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let mut it = s.strip_prefix('v').unwrap_or(s).split('.');
    let v = (
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    );
    it.next().is_none().then_some(v)
}

/// 署名を検証してから更新情報を読む。署名対象は配布された JSON のバイト列そのもの
/// (正規化に依存しない)。署名の検証前に中身を解釈しない。
pub fn verify_manifest(
    manifest: &[u8],
    signature_hex: &str,
    keys: &[[u8; 32]],
) -> Result<Manifest, UpdateError> {
    if manifest.len() > MAX_MANIFEST_BYTES {
        return Err(UpdateError::TooLarge);
    }
    if keys.is_empty() {
        return Err(UpdateError::NoTrustedKey);
    }
    let sig_bytes = from_hex(signature_hex.trim()).ok_or(UpdateError::BadSignature)?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|_| UpdateError::BadSignature)?;
    let verified = keys.iter().any(|k| {
        VerifyingKey::from_bytes(k)
            .map(|vk| vk.verify(manifest, &sig).is_ok())
            .unwrap_or(false)
    });
    if !verified {
        return Err(UpdateError::BadSignature);
    }
    serde_json::from_slice(manifest).map_err(|_| UpdateError::BadManifest)
}

/// 適用してよい成果物を選ぶ。チャンネル違い・現在版以下・https 以外・未対応 OS を拒否する。
/// 新しい版があるのに対象 OS の成果物が無い時は `NoArtifact`。
pub fn select<'a>(
    m: &'a Manifest,
    channel: &str,
    current: &str,
    platform: &str,
) -> Result<Option<&'a Artifact>, UpdateError> {
    if m.schema_version != 1 {
        return Err(UpdateError::BadManifest);
    }
    if m.channel != channel {
        return Err(UpdateError::WrongChannel);
    }
    let new = parse_version(&m.version).ok_or(UpdateError::BadManifest)?;
    let cur = parse_version(current).ok_or(UpdateError::BadManifest)?;
    if new <= cur {
        return Err(UpdateError::NotNewer);
    }
    let Some(a) = m.artifacts.iter().find(|a| a.platform == platform) else {
        return Err(UpdateError::NoArtifact);
    };
    if !secure_url(&a.url) {
        return Err(UpdateError::InsecureUrl);
    }
    Ok(Some(a))
}

/// https のみ許可する。ループバック(この Mac 自身)への http だけは試験用に許す。
/// 通信経路を攻撃者が握っていても署名と SHA-256 が検証されるため、これは防御の中心ではない。
pub fn secure_url(url: &str) -> bool {
    // `http://127.0.0.1:@evil.com/` のように、接頭辞だけ似せた別ホストを弾く
    if url.split('/').nth(2).is_some_and(|authority| authority.contains('@')) {
        return false;
    }
    if url.starts_with("https://") {
        return true;
    }
    ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"]
        .iter()
        .any(|p| url.starts_with(p))
}

/// 取得した成果物の大きさとハッシュを検査する。宣言サイズを超えて読まない。
pub fn verify_artifact(mut data: impl Read, a: &Artifact) -> Result<(), UpdateError> {
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = data.read(&mut buf).map_err(|_| UpdateError::Io)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > a.size {
            return Err(UpdateError::SizeMismatch);
        }
        hasher.update(&buf[..n]);
    }
    if total != a.size {
        return Err(UpdateError::SizeMismatch);
    }
    let got = to_hex(&hasher.finalize());
    if got.eq_ignore_ascii_case(&a.sha256) {
        Ok(())
    } else {
        Err(UpdateError::HashMismatch)
    }
}

/// 見つかった更新
pub struct Available {
    pub version: String,
    pub artifact: Artifact,
}

pub struct CheckConfig<'a> {
    pub manifest_url: &'a str,
    pub keys: &'a [[u8; 32]],
    pub channel: &'a str,
    pub current: &'a str,
    pub platform: &'a str,
}

/// 取得手段: `fetch(url, 上限バイト)` が本文を返す
pub type Fetch<'a> = &'a dyn Fn(&str, u64) -> Result<Vec<u8>, String>;

/// 取得済みの更新情報と署名を検証し、適用できる更新を返す。`Ok(None)` は最新。
pub fn evaluate(
    cfg: &CheckConfig,
    manifest: &[u8],
    signature_hex: &str,
) -> Result<Option<Available>, String> {
    let m = verify_manifest(manifest, signature_hex, cfg.keys).map_err(|e| e.message().to_string())?;
    match select(&m, cfg.channel, cfg.current, cfg.platform) {
        Ok(Some(a)) => Ok(Some(Available {
            version: m.version.clone(),
            artifact: a.clone(),
        })),
        Ok(None) | Err(UpdateError::NotNewer) => Ok(None),
        Err(e) => Err(e.message().into()),
    }
}

/// 更新情報と署名を取得して検証し、適用できる更新を返す。
/// 取得手段(`fetch(url, 上限バイト)`)は呼び出し側が渡す(OS ごとに異なるため)。
pub fn check_with(
    cfg: &CheckConfig,
    fetch: Fetch,
) -> Result<Option<Available>, String> {
    if cfg.keys.is_empty() {
        return Err(UpdateError::NoTrustedKey.message().into());
    }
    let manifest = fetch(cfg.manifest_url, MAX_MANIFEST_BYTES as u64)?;
    let sig = fetch(&format!("{}.sig", cfg.manifest_url), 1024)?;
    let sig = String::from_utf8(sig).map_err(|_| UpdateError::BadSignature.message().to_string())?;
    evaluate(cfg, &manifest, &sig)
}

/// curl で取得する(macOS の `/usr/bin/curl`、Windows 10 以降の `curl.exe`)。
/// https のみ(試験用のループバック http を除く)、リダイレクトも https のみ。
/// `dest` があればファイルへ、無ければ標準出力の内容を返す。
pub fn curl(
    bin: &str,
    url: &str,
    dest: Option<&std::path::Path>,
    max: u64,
    user_agent: &str,
) -> Result<Vec<u8>, String> {
    use std::process::{Command, Stdio};
    if !secure_url(url) {
        return Err(UpdateError::InsecureUrl.message().into());
    }
    let protos = if url.starts_with("https://") { "=https" } else { "=http" };
    let mut c = Command::new(bin);
    // 更新情報(メモリへ)は短く、成果物(ファイルへ)は長く待つ
    let max_time = if dest.is_some() { "900" } else { "60" };
    c.args(["-fsSL", "--proto", protos, "--proto-redir", "=https"])
        .args(["--connect-timeout", "15", "--max-time", max_time])
        .args(["--max-filesize", &max.to_string()])
        .args(["-A", user_agent]);
    if let Some(d) = dest {
        c.arg("-o").arg(d);
    }
    let out = c
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "通信を開始できません".to_string())?;
    if !out.status.success() {
        return Err("更新サーバーに接続できません".into());
    }
    if out.stdout.len() as u64 > max {
        return Err(UpdateError::TooLarge.message().into());
    }
    Ok(out.stdout)
}

/// 展開先(`extracted`)直下にある配布物のフォルダを1つ特定する
/// (`__MACOSX` と `._*` は読み飛ばす。ファイルや複数フォルダは拒否)。
pub fn find_package_dir(extracted: &std::path::Path) -> Result<std::path::PathBuf, String> {
    use std::fs;
    let bad = || Err("更新ファイルの構成が不正です".to_string());
    let mut dirs = Vec::new();
    for e in fs::read_dir(extracted).map_err(|_| "更新ファイルを展開できません".to_string())? {
        let e = e.map_err(|_| "更新ファイルを展開できません".to_string())?;
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with("__MACOSX") || name.starts_with("._") {
            continue;
        }
        // シンボリックリンクは辿らない
        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            dirs.push(e.path());
        } else {
            return bad();
        }
    }
    match &dirs[..] {
        [pkg] => Ok(pkg.clone()),
        _ => bad(),
    }
}

/// 配布物の `release-manifest.json` が、署名済み更新情報の宣言と同じ版・プラットフォームかを確認する
pub fn check_release_manifest(
    pkg: &std::path::Path,
    version: &str,
    platform: &str,
) -> Result<(), String> {
    let info: serde_json::Value = std::fs::read(pkg.join("release-manifest.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or("更新ファイルに版の情報がありません")?;
    if info["schema_version"] != 1 || info["version"] != version || info["platform"] != platform {
        return Err("更新ファイルの版が更新情報と一致しません".into());
    }
    Ok(())
}

/// 展開先(`extracted`)にある配布物のフォルダを特定して検査し、更新対象外のファイルを除く。
/// `release-manifest.json` の版・プラットフォームが署名済み更新情報の宣言と一致することを確認する
/// (署名済みのハッシュで内容は保証済みだが、取り違えた成果物の適用を二重に防ぐ)。
/// `keep` の先頭のファイル(実行ファイル)が無ければ拒否する。返り値は配布物のフォルダ。
pub fn prepare_package(
    extracted: &std::path::Path,
    version: &str,
    platform: &str,
    keep: &[&str],
) -> Result<std::path::PathBuf, String> {
    use std::fs;
    let bad = |m: &str| Err(m.to_string());
    let pkg = &find_package_dir(extracted)?;
    check_release_manifest(pkg, version, platform)?;
    match keep.first() {
        Some(exe) if pkg.join(exe).is_file() => {}
        _ => return bad("更新ファイルに実行ファイルがありません"),
    }
    for e in fs::read_dir(pkg).map_err(|_| "更新ファイルを展開できません".to_string())? {
        let e = e.map_err(|_| "更新ファイルを展開できません".to_string())?;
        let name = e.file_name().to_string_lossy().to_string();
        let is_file = e.file_type().map(|t| t.is_file()).unwrap_or(false);
        if !is_file {
            return bad("更新ファイルの構成が不正です");
        }
        if !keep.contains(&name.as_str()) {
            // 新版が KEEP 未登録の新規ファイル(DLL 等)を同梱すると、ここで消されて
            // 新版が起動失敗する。事故の発見可能化のため削除対象を残す
            eprintln!("[update] KEEP 未登録のため削除: {name}");
            fs::remove_file(e.path()).map_err(|_| "更新ファイルを整理できません".to_string())?;
        }
    }
    Ok(pkg.clone())
}

/// `swap_and_start` が起動を頼む理由。新版の起動と、元の版へ戻した後の起動を区別する
/// (呼び出し側が利用者へ伝える文言を変えられる)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Launch {
    New,
    /// 元の版を起動し直す。`restored` は全ファイルを戻せたか
    Restore { restored: bool },
}

/// 展開済みの新しい配布物(`staged` 直下のファイル)を `target` へ入れ替える。
/// 各ファイルは `<名前>.knit-old` へ退避してから置き、`start` で起動した新版が `health` の間
/// 生き続けなければ、全て元に戻して旧版を起動し直す(Windows の更新で使う)。
/// 利用者の設定・鍵・ログを上書きしないよう、そのような名前を含む配布物は拒否する。
pub fn swap_and_start(
    staged: &std::path::Path,
    target: &std::path::Path,
    start: &dyn Fn(Launch) -> std::io::Result<std::process::Child>,
    health: std::time::Duration,
) -> Result<(), String> {
    use std::fs;
    let mut names: Vec<String> = Vec::new();
    for e in fs::read_dir(staged).map_err(|_| "更新ファイルを読み込めません".to_string())? {
        let e = e.map_err(|_| "更新ファイルを読み込めません".to_string())?;
        let name = e.file_name().to_string_lossy().to_string();
        if !e.file_type().map(|t| t.is_file()).unwrap_or(false) {
            return Err(format!("更新ファイルに想定外の項目があります: {name}"));
        }
        let lower = name.to_lowercase();
        let protected = lower == ".env"
            || lower.starts_with(".env.")
            || lower == "preferences.json"
            || lower == "paired-host.txt"
            || lower.contains(".log")
            || lower.ends_with(".dpapi")
            || lower.ends_with(".knit-old");
        if protected {
            return Err(format!("更新ファイルに設定・ログが含まれています: {name}"));
        }
        names.push(name);
    }
    if names.is_empty() {
        return Err("更新ファイルが空です".into());
    }
    // 実行ファイルは最後に置く。途中で電源が落ちても、本体は旧版のまま残る
    names.sort_by_key(|n| n.to_lowercase().ends_with(".exe"));
    let old_of = |n: &str| target.join(format!("{n}.knit-old"));
    let mut done: Vec<(String, bool)> = Vec::new(); // (名前, 退避した旧ファイルがあったか)
    // 戻せたかを返す(戻せなかったのに「戻しました」と表示しないため)
    let rollback = |done: &[(String, bool)]| -> bool {
        let mut ok = true;
        for (n, had_old) in done.iter().rev() {
            if target.join(n).exists() && fs::remove_file(target.join(n)).is_err() {
                ok = false;
            }
            if *had_old && fs::rename(old_of(n), target.join(n)).is_err() {
                ok = false;
            }
        }
        ok
    };
    for n in &names {
        let dest = target.join(n);
        let _ = fs::remove_file(old_of(n));
        let had_old = dest.exists();
        if had_old && fs::rename(&dest, old_of(n)).is_err() {
            let restored = rollback(&done);
            let _ = start(Launch::Restore { restored }); // 旧プロセスは終了済み。旧版を起動し直す
            return Err(format!("旧版を退避できません: {n}"));
        }
        if fs::rename(staged.join(n), &dest).is_err() && fs::copy(staged.join(n), &dest).is_err() {
            if had_old {
                let _ = fs::rename(old_of(n), &dest);
            }
            let restored = rollback(&done);
            let _ = start(Launch::Restore { restored });
            return Err(format!("新しいファイルを配置できません: {n}"));
        }
        done.push((n.clone(), had_old));
    }
    let survived = match start(Launch::New) {
        Ok(mut child) => {
            let deadline = std::time::Instant::now() + health;
            let mut alive = true;
            while std::time::Instant::now() < deadline {
                if child.try_wait().map(|s| s.is_some()).unwrap_or(true) {
                    alive = false;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            if !alive {
                let _ = child.kill();
            }
            alive
        }
        Err(_) => false,
    };
    if survived {
        for (n, had_old) in &done {
            if *had_old {
                let _ = fs::remove_file(old_of(n));
            }
        }
        Ok(())
    } else {
        let restored = rollback(&done);
        let _ = start(Launch::Restore { restored });
        if restored {
            Err("新しい版が起動しなかったため、元の版へ戻しました".into())
        } else {
            Err("新しい版が起動せず、元の版へ完全には戻せませんでした。インストール先の「.knit-old」が付いたファイルを元の名前に戻してください".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn manifest_json(version: &str, channel: &str, url: &str, sha: &str, size: u64) -> String {
        format!(
            r#"{{"schema_version":1,"channel":"{channel}","version":"{version}","protocol":13,
"artifacts":[{{"platform":"macos-arm64","url":"{url}","size":{size},"sha256":"{sha}"}}]}}"#
        )
    }

    fn sign(sk: &SigningKey, m: &str) -> String {
        to_hex(&sk.sign(m.as_bytes()).to_bytes())
    }

    fn sha_of(b: &[u8]) -> String {
        to_hex(&Sha256::digest(b))
    }

    #[test]
    fn accepts_valid_signature_and_selects_newer_artifact() {
        let sk = key();
        let m = manifest_json("0.27.0", "stable", "https://example.com/k.zip", "00", 1);
        let keys = [sk.verifying_key().to_bytes()];
        let parsed = verify_manifest(m.as_bytes(), &sign(&sk, &m), &keys).unwrap();
        let a = select(&parsed, "stable", "0.26.0", "macos-arm64").unwrap();
        assert_eq!(a.unwrap().url, "https://example.com/k.zip");
        // 新しい版はあるのに対象 OS の成果物が無い時は「最新」と言わない
        assert_eq!(
            select(&parsed, "stable", "0.26.0", "windows-x64").unwrap_err(),
            UpdateError::NoArtifact
        );
    }

    #[test]
    fn empty_key_list_rejects_everything() {
        let sk = key();
        let m = manifest_json("0.27.0", "stable", "https://e.com/k.zip", "00", 1);
        assert_eq!(
            verify_manifest(m.as_bytes(), &sign(&sk, &m), &[]).unwrap_err(),
            UpdateError::NoTrustedKey
        );
    }

    #[test]
    fn tampered_manifest_wrong_key_and_garbage_signature_are_rejected() {
        let sk = key();
        let m = manifest_json("0.27.0", "stable", "https://e.com/k.zip", "00", 1);
        let keys = [sk.verifying_key().to_bytes()];
        let sig = sign(&sk, &m);
        let tampered = m.replace("0.27.0", "9.0.0");
        assert_eq!(
            verify_manifest(tampered.as_bytes(), &sig, &keys).unwrap_err(),
            UpdateError::BadSignature
        );
        let other = SigningKey::from_bytes(&[9u8; 32]);
        assert_eq!(
            verify_manifest(m.as_bytes(), &sign(&other, &m), &keys).unwrap_err(),
            UpdateError::BadSignature
        );
        for bad in ["", "zz", "abcd"] {
            assert_eq!(
                verify_manifest(m.as_bytes(), bad, &keys).unwrap_err(),
                UpdateError::BadSignature
            );
        }
    }

    #[test]
    fn rotated_key_set_accepts_either_key() {
        let old = SigningKey::from_bytes(&[1u8; 32]);
        let new = key();
        let m = manifest_json("0.27.0", "stable", "https://e.com/k.zip", "00", 1);
        let keys = [old.verifying_key().to_bytes(), new.verifying_key().to_bytes()];
        assert!(verify_manifest(m.as_bytes(), &sign(&new, &m), &keys).is_ok());
        assert!(verify_manifest(m.as_bytes(), &sign(&old, &m), &keys).is_ok());
    }

    #[test]
    fn oversized_manifest_is_rejected_before_parsing() {
        let big = vec![b' '; MAX_MANIFEST_BYTES + 1];
        assert_eq!(
            verify_manifest(&big, "00", &[[0u8; 32]]).unwrap_err(),
            UpdateError::TooLarge
        );
    }

    #[test]
    fn select_rejects_downgrade_same_version_wrong_channel_and_http() {
        let mk = |v: &str, c: &str, u: &str| {
            serde_json::from_str::<Manifest>(&manifest_json(v, c, u, "00", 1)).unwrap()
        };
        let ok = "https://e.com/k.zip";
        assert_eq!(
            select(&mk("0.26.0", "stable", ok), "stable", "0.26.0", "macos-arm64").unwrap_err(),
            UpdateError::NotNewer
        );
        assert_eq!(
            select(&mk("0.25.9", "stable", ok), "stable", "0.26.0", "macos-arm64").unwrap_err(),
            UpdateError::NotNewer
        );
        assert_eq!(
            select(&mk("0.27.0", "beta", ok), "stable", "0.26.0", "macos-arm64").unwrap_err(),
            UpdateError::WrongChannel
        );
        assert_eq!(
            select(&mk("0.27.0", "stable", "http://e.com/k.zip"), "stable", "0.26.0", "macos-arm64")
                .unwrap_err(),
            UpdateError::InsecureUrl
        );
        assert!(select(&mk("0.100.0", "stable", ok), "stable", "0.26.0", "macos-arm64").is_ok());
    }

    #[test]
    fn only_https_or_loopback_http_is_allowed() {
        assert!(secure_url("https://github.com/x"));
        assert!(secure_url("http://127.0.0.1:8000/k.zip"));
        assert!(!secure_url("http://github.com/x"));
        assert!(!secure_url("http://127.0.0.1.evil.com/x"));
        assert!(!secure_url("http://localhost.evil.com:80/x"));
        assert!(!secure_url("ftp://e.com/x"));
        // 接頭辞だけ似せた別ホスト(userinfo)は拒否
        assert!(!secure_url("http://127.0.0.1:@evil.com/x"));
        assert!(!secure_url("https://github.com@evil.com/x"));
    }

    #[test]
    fn key_file_ignores_comments_and_malformed_lines() {
        let good = "ab".repeat(32);
        let text = format!("# 公開鍵\n\n{good} # 2026\nnot-hex\n{}\n", "cd".repeat(31));
        assert_eq!(parse_keys(&text), vec![[0xab; 32]]);
    }

    #[test]
    fn version_parsing_is_strict_and_numeric() {
        assert_eq!(parse_version("0.26.0"), Some((0, 26, 0)));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("1.2.3-beta"), None);
    }

    #[test]
    fn artifact_hash_and_size_are_enforced() {
        let body = b"knit release bytes".to_vec();
        let a = Artifact {
            platform: "macos-arm64".into(),
            url: "https://e.com/k.zip".into(),
            size: body.len() as u64,
            sha256: sha_of(&body),
        };
        assert!(verify_artifact(&body[..], &a).is_ok());
        let mut flipped = body.clone();
        flipped[0] ^= 1;
        assert_eq!(
            verify_artifact(&flipped[..], &a).unwrap_err(),
            UpdateError::HashMismatch
        );
        assert_eq!(
            verify_artifact(&body[..body.len() - 1], &a).unwrap_err(),
            UpdateError::SizeMismatch
        );
        let mut longer = body.clone();
        longer.push(0);
        assert_eq!(
            verify_artifact(&longer[..], &a).unwrap_err(),
            UpdateError::SizeMismatch
        );
    }

    fn cfg<'a>(keys: &'a [[u8; 32]], current: &'a str) -> CheckConfig<'a> {
        CheckConfig {
            manifest_url: "https://e.com/update.json",
            keys,
            channel: "stable",
            current,
            platform: "macos-arm64",
        }
    }

    #[test]
    fn check_with_returns_update_latest_or_error() {
        let sk = key();
        let keys = [sk.verifying_key().to_bytes()];
        let m = manifest_json("0.27.0", "stable", "https://e.com/k.zip", "00", 1);
        let sig = sign(&sk, &m);
        let fetch = |url: &str, _max: u64| -> Result<Vec<u8>, String> {
            Ok(if url.ends_with(".sig") { sig.clone().into_bytes() } else { m.clone().into_bytes() })
        };
        let a = check_with(&cfg(&keys, "0.26.0"), &fetch).unwrap().unwrap();
        assert_eq!(a.version, "0.27.0");
        assert!(check_with(&cfg(&keys, "0.27.0"), &fetch).unwrap().is_none());
        assert!(check_with(&cfg(&[], "0.26.0"), &fetch).is_err());
        let other = [SigningKey::from_bytes(&[3u8; 32]).verifying_key().to_bytes()];
        assert!(check_with(&cfg(&other, "0.26.0"), &fetch).is_err());
        let failing = |_: &str, _: u64| -> Result<Vec<u8>, String> { Err("通信失敗".into()) };
        assert_eq!(check_with(&cfg(&keys, "0.26.0"), &failing).err().unwrap(), "通信失敗");
    }

    #[cfg(unix)]
    mod swap {
        use super::super::*;
        use std::path::{Path, PathBuf};
        use std::process::Command;
        use std::time::Duration;

        fn exe(dir: &Path, name: &str, body: &str) {
            use std::os::unix::fs::PermissionsExt;
            let p = dir.join(name);
            std::fs::write(&p, format!("#!/bin/bash\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        fn setup(tag: &str, new_body: &str) -> (PathBuf, PathBuf) {
            let root = std::env::temp_dir().join(format!("knit-swap-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let (target, staged) = (root.join("t"), root.join("s"));
            std::fs::create_dir_all(&target).unwrap();
            std::fs::create_dir_all(&staged).unwrap();
            exe(&target, "knit", "sleep 30 # old");
            std::fs::write(target.join(".env"), "SECRET").unwrap();
            std::fs::write(target.join("knit.log"), "log").unwrap();
            exe(&staged, "knit", new_body);
            std::fs::write(staged.join("release-manifest.json"), "{}").unwrap();
            (target, staged)
        }

        fn run(target: &Path, staged: &Path) -> Result<(), String> {
            let exe_path = target.join("knit");
            swap_and_start(
                staged,
                target,
                &|_| Command::new("/bin/bash").arg(&exe_path).spawn(),
                Duration::from_millis(600),
            )
        }

        fn kill_all(root: &Path) {
            let _ = Command::new("/usr/bin/pkill").args(["-f", "--", &root.to_string_lossy()]).status();
            let _ = std::fs::remove_dir_all(root);
        }

        #[test]
        fn swap_installs_new_files_and_keeps_user_data() {
            let (t, s) = setup("ok", "sleep 30 # new");
            assert!(run(&t, &s).is_ok());
            assert!(std::fs::read_to_string(t.join("knit")).unwrap().contains("# new"));
            assert!(t.join("release-manifest.json").exists());
            assert_eq!(std::fs::read_to_string(t.join(".env")).unwrap(), "SECRET");
            assert!(t.join("knit.log").exists());
            assert!(!t.join("knit.knit-old").exists());
            kill_all(t.parent().unwrap());
        }

        #[test]
        fn swap_rolls_back_when_new_version_exits() {
            let (t, s) = setup("rb", "exit 3 # new");
            let e = run(&t, &s).unwrap_err();
            assert!(e.contains("戻しました"));
            assert!(std::fs::read_to_string(t.join("knit")).unwrap().contains("# old"));
            assert!(!t.join("release-manifest.json").exists());
            assert!(!t.join("knit.knit-old").exists());
            kill_all(t.parent().unwrap());
        }

        #[test]
        fn swap_refuses_packages_carrying_settings_or_directories() {
            let (t, s) = setup("prot", "sleep 30");
            std::fs::write(s.join(".env"), "EVIL").unwrap();
            assert!(run(&t, &s).is_err());
            assert_eq!(std::fs::read_to_string(t.join(".env")).unwrap(), "SECRET");
            assert!(std::fs::read_to_string(t.join("knit")).unwrap().contains("# old"));
            std::fs::remove_file(s.join(".env")).unwrap();
            std::fs::create_dir(s.join("sub")).unwrap();
            assert!(run(&t, &s).is_err());
            kill_all(t.parent().unwrap());
        }
    }

    #[test]
    fn prepare_package_checks_version_platform_and_prunes_extras() {
        let root = std::env::temp_dir().join(format!("knit-prep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let pkg = root.join("Knit-win-0.27.0");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("knit-win.exe"), "x").unwrap();
        std::fs::write(pkg.join("install.bat"), "x").unwrap();
        std::fs::write(
            pkg.join("release-manifest.json"),
            r#"{"schema_version":1,"version":"0.27.0","platform":"windows-x64"}"#,
        )
        .unwrap();
        let keep = ["knit-win.exe", "release-manifest.json"];
        assert!(prepare_package(&root, "0.28.0", "windows-x64", &keep).is_err());
        assert!(prepare_package(&root, "0.27.0", "macos-arm64", &keep).is_err());
        let got = prepare_package(&root, "0.27.0", "windows-x64", &keep).unwrap();
        assert_eq!(got, pkg);
        assert!(!pkg.join("install.bat").exists());
        assert!(pkg.join("knit-win.exe").exists());
        // 実行ファイルが無い/フォルダが2つ/直下にファイルがある構成は拒否
        std::fs::remove_file(pkg.join("knit-win.exe")).unwrap();
        assert!(prepare_package(&root, "0.27.0", "windows-x64", &keep).is_err());
        std::fs::write(pkg.join("knit-win.exe"), "x").unwrap();
        std::fs::create_dir(root.join("other")).unwrap();
        assert!(prepare_package(&root, "0.27.0", "windows-x64", &keep).is_err());
        std::fs::remove_dir(root.join("other")).unwrap();
        std::fs::write(root.join("stray.txt"), "x").unwrap();
        assert!(prepare_package(&root, "0.27.0", "windows-x64", &keep).is_err());
        std::fs::remove_dir_all(&root).ok();
    }
}
