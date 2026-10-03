//! Android タブレットの状態と「操作する/しない」の選択(端末ごとの登録と取消)。
//!
//! 製品定義の方針に合わせ、見つけたタブレットを黙って操作対象にしない。利用者が端末ごとに
//! 選び、いつでも取り消せる。状態は「許可待ち・選択待ち・接続中・接続できない」を区別して
//! メニューへ出す(同じ「接続できません」にまとめない)

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    Usb,
    Wifi,
}

impl Link {
    pub fn of(serial: &str) -> Self {
        if serial.contains(':') || serial.contains("._adb-tls-connect.") {
            Link::Wifi
        } else {
            Link::Usb
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// タブレット側で「USB デバッグを許可」がまだ押されていない
    Unauthorized,
    /// adb からは見えるが応答しない
    Offline,
    /// 操作するかどうか未選択
    AskUser,
    /// 利用者が「操作しない」を選んだ
    Declined,
    Connecting,
    Connected,
    /// 操作する設定だが繋がらない(理由付き)
    Failed(String),
}

impl Phase {
    /// 設定ウィンドウの接続ページに並べる、状態の要約(メニューの文言の短縮版)
    pub fn summary(&self) -> String {
        match self {
            Phase::Unauthorized => "USB デバッグの許可待ち".into(),
            Phase::Offline => "応答ありません".into(),
            Phase::AskUser => "メニューから「操作する」を選べます".into(),
            Phase::Declined => "操作しない".into(),
            Phase::Connecting => "接続しています…".into(),
            Phase::Connected => "操作できます".into(),
            Phase::Failed(e) => format!("接続できません({e})"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tablet {
    /// 端末ごとの安定した識別子(選択の保存と接続先の再接続に使う)
    pub key: String,
    pub name: String,
    pub link: Link,
    pub phase: Phase,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// adb が無い等、全端末に共通する問題
    pub problem: Option<String>,
    pub tablets: Vec<Tablet>,
}

static STATUS: Mutex<Status> = Mutex::new(Status {
    problem: None,
    tablets: Vec::new(),
});
/// 状態が変わるたびに増える(メニューは変化した時だけ作り直す)
static GEN: AtomicU64 = AtomicU64::new(0);

pub fn publish(s: Status) {
    let mut cur = STATUS.lock().unwrap_or_else(|e| e.into_inner());
    if *cur != s {
        *cur = s;
        GEN.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn snapshot() -> (u64, Status) {
    let s = STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    (GEN.load(Ordering::Relaxed), s)
}

pub fn bump() {
    GEN.fetch_add(1, Ordering::Relaxed);
}

// ---- 操作する/しないの選択(端末ごとに保存する) ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Allow,
    Deny,
}

fn store_path() -> Option<PathBuf> {
    // KNIT_ANDROID_STORE は試験用(実機試験で利用者の選択を書き換えないため)
    if let Some(p) = knit_common::envutil::get("KNIT_ANDROID_STORE") {
        return Some(PathBuf::from(p));
    }
    knit_common::envutil::data_dir().map(|d| d.join("android-tablets.txt"))
}

/// 1 行 1 端末: `allow|deny <TAB> 端末キー <TAB> 表示名`
pub fn parse_choices(text: &str) -> HashMap<String, (Choice, String)> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.splitn(3, '\t');
            let c = match it.next()? {
                "allow" => Choice::Allow,
                "deny" => Choice::Deny,
                _ => return None,
            };
            let key = it.next()?.trim();
            (!key.is_empty()).then(|| (key.to_string(), (c, it.next().unwrap_or("").to_string())))
        })
        .collect()
}

static CHOICES: Mutex<Option<HashMap<String, (Choice, String)>>> = Mutex::new(None);

fn with_choices<T>(f: impl FnOnce(&mut HashMap<String, (Choice, String)>) -> T) -> T {
    let mut g = CHOICES.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(|| {
        store_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| parse_choices(&t))
            .unwrap_or_default()
    });
    f(m)
}

/// この端末を操作してよいか。保存された選択があればそれに従い、無ければ環境変数
/// `KNIT_ANDROID_ADB=1` の時だけ許可する(adb での操作は開発者向けの精密モード。通常はタブレットの Knit アプリを使う)
pub fn choice(key: &str) -> Option<Choice> {
    with_choices(|m| m.get(key).map(|(c, _)| *c))
        .or_else(|| (crate::envutil::get("KNIT_ANDROID_ADB").as_deref() == Some("1")).then_some(Choice::Allow))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_are_parsed_and_broken_lines_are_skipped() {
        let back = parse_choices("allow\tandroid-1\tPad6\ndeny\tandroid-2\tPhone\n");
        assert_eq!(back["android-1"], (Choice::Allow, "Pad6".to_string()));
        assert_eq!(back["android-2"].0, Choice::Deny);
        // 壊れた行は読み飛ばす
        assert!(parse_choices("maybe\tx\ty\nallow\t\tz\n").is_empty());
    }

    #[test]
    fn link_is_told_apart_by_serial() {
        assert_eq!(Link::of("e569c535"), Link::Usb);
        assert_eq!(Link::of("192.168.1.6:40001"), Link::Wifi);
        assert_eq!(Link::of("adb-R52-Ab._adb-tls-connect._tcp"), Link::Wifi);
    }
}
