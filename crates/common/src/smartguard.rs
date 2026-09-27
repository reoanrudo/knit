//! クリップボード送信前の機密検出ガード(実験的・ollaya 併用)。
//! ローカルで動く decision model ランタイム(ollaya、既定ポート 11435)に
//! 「このテキストは機密に見えるか」を 1 問だけ聞き、高確度なら送信を止める。
//! OS の秘匿印(Concealed/Transient 等)だけでは拾えない意味レベルの漏えいを
//! 追加で防ぐ。原則:
//! - 判定は完全にローカル(テキストが外へ出ることはない)
//! - ollaya が無い・タイムアウト・解析不能はすべて None=現行どおり送る
//!   (追加ガードなので、基盤が無くても挙動を変えない)
//! - 呼び出し元は同期用の別スレッドに限る(入力経路を絶対に塞がない)

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub const PORT_DEFAULT: u16 = 11435;
/// 検査に掛けるテキストの上限(判定モデルの文脈は短い。頭から切る)
const MAX_CHECK_CHARS: usize = 6000;
/// この確度(P=機密クレデンシャル)以上で送信を止める。
/// 実測(laya・choice 形式): 機密 0.96〜0.99 / 通常文 0.03〜0.18 で明瞭に分離。
/// noul 形式は日本語の通常文が 0.97 を出して全滅したため choice 分類にする(実績)
pub const BLOCK_THRESHOLD: f64 = 0.8;
/// 判定 1 回の時間上限。超えたら打ち切って現行動作(送る)へ戻る
const DECIDE_TIMEOUT: Duration = Duration::from_millis(1_200);

pub fn enabled() -> bool {
    crate::envutil::get("TSUNAGU_SMART_SECRET").as_deref() != Some("0")
}

fn port() -> u16 {
    crate::envutil::get("TSUNAGU_OLLAYA_PORT")
        .and_then(|v| v.parse().ok())
        .unwrap_or(PORT_DEFAULT)
}

/// 生 TCP で HTTP POST /api/decide を叩く(依存追加なしの規約どおり)。
/// Connection: close で 1 往復。チャンク転送も最小限デコードする。
/// 失敗(接続拒否・タイムアウト・非 200・JSON 不正)はすべて None
pub fn decide_at(
    addr: SocketAddr,
    model: &str,
    state: &str,
    questions: &serde_json::Value,
    timeout: Duration,
) -> Option<serde_json::Value> {
    let body = serde_json::json!({
        "model": model,
        "state": state,
        "questions": questions,
    })
    .to_string();
    let mut s =
        TcpStream::connect_timeout(&addr, timeout.min(Duration::from_millis(500))).ok()?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(timeout)).ok();
    let req = format!(
        "POST /api/decide HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).ok()?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).ok()?;
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
        return None;
    }
    let chunked = head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked");
    let body = if chunked { dechunk(body)? } else { body };
    let start = body.find('{')?;
    let end = body.rfind('}')?;
    serde_json::from_str(body[start..=end].trim()).ok()
}

/// チャンク転送の最小デコード(サイズ行+本体+改行の繰り返し)。壊れていれば None
fn dechunk(body: &str) -> Option<&str> {
    let mut end = 0usize;
    loop {
        let rest = &body[end..];
        let line_len = rest.find("\r\n")? + 2;
        let size_str = rest[..line_len - 2].split(';').next()?.trim();
        let size = usize::from_str_radix(size_str, 16).ok()?;
        if size == 0 {
            return body.get(..end).filter(|s| !s.is_empty());
        }
        end += line_len + size + 2;
        if end > body.len() {
            return None;
        }
    }
}

/// decide 応答から機密確度(P=secret_credential)を取り出す
fn secret_prob(v: &serde_json::Value) -> Option<f64> {
    v.get("answers")?
        .get("kind")?
        .get("probabilities")?
        .get("secret_credential")?
        .as_f64()
}

/// テスト差し替え口。実本体はこちらを呼ぶ
pub fn looks_secret_with(addr: SocketAddr, text: &str, timeout: Duration) -> Option<bool> {
    // noul(はい/いいえ)ではなく 3 択分類にする: 小型モデルは open な yes/no より
    // 選択肢分類が得意(実測)。source_code を選択肢に含めるのは、コード断片を
    // 「機密ではない」と明示的に拾わせるため(鍵の貼り間違いは choice が拾う)
    let questions = serde_json::json!({
        "kind": {
            "type": "choice",
            "instructions": "What kind of text is this?",
            "options": ["secret_credential", "normal_text", "source_code"],
            "criteria": {
                "secret_credential": "contains a password, API key, token, private key or card number",
                "normal_text": "ordinary prose such as email, chat or notes",
                "source_code": "programming code or config file content"
            }
        }
    });
    let cut = text
        .char_indices()
        .nth(MAX_CHECK_CHARS)
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let v = decide_at(addr, "laya", &text[..cut], &questions, timeout)?;
    let p = secret_prob(&v)?;
    Some(p >= BLOCK_THRESHOLD)
}

/// 本体。ollaya(127.0.0.1:既定 11435)へ問い、機密なら true。
/// 無効化・未起動・失敗は None(呼び出し側は現行動作を続ける)
pub fn looks_secret(text: &str) -> Option<bool> {
    if !enabled() {
        return None;
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], port()));
    looks_secret_with(addr, text, DECIDE_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// 1 リクエストだけ受け、用意したレスポンスを返すモックサーバー
    fn mock_server(response: String) -> SocketAddr {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf); // リクエスト本文は検査しない
            s.write_all(response.as_bytes()).unwrap();
        });
        addr
    }

    fn body_chunked(json: &str) -> String {
        // チャンクサイズは 16 進の実バイト数(マルチバイト文字も正しく数える)
        let n = json.len();
        format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{n:x}\r\n{json}\r\n0\r\n\r\n")
    }

    #[test]
    fn high_confidence_blocks_and_low_confidence_passes() {
        let json = r#"{"answers":{"kind":{"choice":"secret_credential","probabilities":{"secret_credential":0.97,"normal_text":0.02,"source_code":0.01}}}}"#;
        let addr = mock_server(body_chunked(json));
        assert_eq!(looks_secret_with(addr, "token", Duration::from_secs(2)), Some(true));

        let json = r#"{"answers":{"kind":{"choice":"normal_text","probabilities":{"secret_credential":0.18,"normal_text":0.55,"source_code":0.27}}}}"#;
        let addr = mock_server(body_chunked(json));
        assert_eq!(looks_secret_with(addr, "hello", Duration::from_secs(2)), Some(false));
        // コード断片(secret でない)=送る
        let json = r#"{"answers":{"kind":{"choice":"source_code","probabilities":{"secret_credential":0.01,"normal_text":0.02,"source_code":0.97}}}}"#;
        let addr = mock_server(body_chunked(json));
        assert_eq!(looks_secret_with(addr, "code", Duration::from_secs(2)), Some(false));
    }

    #[test]
    fn plain_response_and_invalid_json_are_handled() {
        let addr = mock_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 94\r\n\r\n{\"answers\":{\"kind\":{\"choice\":\"secret_credential\",\"probabilities\":{\"secret_credential\":0.95}}}}".into(),
        );
        assert_eq!(looks_secret_with(addr, "x", Duration::from_secs(2)), Some(true));
        // 非 200・JSON 壊れは None(=現行動作にフォールバック)
        let addr = mock_server("HTTP/1.1 500 Err\r\n\r\n".into());
        assert_eq!(looks_secret_with(addr, "x", Duration::from_secs(2)), None);
        let addr = mock_server("HTTP/1.1 200 OK\r\n\r\nnot json".into());
        assert_eq!(looks_secret_with(addr, "x", Duration::from_secs(2)), None);
    }

    #[test]
    fn dead_port_falls_back_to_none() {
        // 誰も受けないポート=ollaya 無しの環境。None で素通し(起動毎に別ポート)
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        drop(l);
        assert_eq!(looks_secret_with(addr, "x", Duration::from_millis(300)), None);
    }

    #[test]
    fn broken_chunked_encoding_is_rejected() {
        let addr = mock_server(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n{}}\r\n0\r\n\r\n".into(),
        );
        let v = decide_at(addr, "laya", "x", &serde_json::json!({}), Duration::from_secs(2));
        assert!(v.is_none());
    }
}
