//! この端末が相手と共有してよい範囲(信頼ゾーン)。
//! キーボード・ポインターの入力は常に共有する。クリップボード・ファイル・音声は
//! `KNIT_SHARE` で個別に許可する。会社の PC のように「入力だけ渡したい」端末で、
//! 相手の設定に関係なく、その端末の側で送受信を止めるために使う。
//!
//!   KNIT_SHARE 未設定 / all        すべて共有(既定)
//!   KNIT_SHARE=input               入力のみ
//!   KNIT_SHARE=clipboard,audio     入力 + 指定したもの(clipboard / files / audio。区切りは , + 空白)
//!
//! 読めない指定は「入力のみ」として扱う(綴り間違いで共有が開くことを防ぐ)。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub clip: bool,
    pub files: bool,
    pub audio: bool,
}

impl Scope {
    pub const ALL: Scope = Scope { clip: true, files: true, audio: true };
    pub const INPUT_ONLY: Scope = Scope { clip: false, files: false, audio: false };

    /// `None` は読めない指定。呼び出し側は `INPUT_ONLY` に倒す
    pub fn parse(spec: &str) -> Option<Scope> {
        let spec = spec.trim().to_lowercase();
        match spec.as_str() {
            "" | "all" => return Some(Scope::ALL),
            "input" | "none" => return Some(Scope::INPUT_ONLY),
            _ => {}
        }
        let mut s = Scope::INPUT_ONLY;
        for item in spec.split([',', '+', ' ']).filter(|i| !i.is_empty()) {
            match item {
                "clip" | "clipboard" => s.clip = true,
                "file" | "files" => s.files = true,
                "audio" | "sound" => s.audio = true,
                "input" => {}
                _ => return None,
            }
        }
        Some(s)
    }

    /// 利用者向けの短い説明
    pub fn describe(&self) -> String {
        if *self == Scope::ALL {
            return "すべて共有".into();
        }
        let mut on = vec!["入力"];
        if self.clip {
            on.push("クリップボード");
        }
        if self.files {
            on.push("ファイル");
        }
        if self.audio {
            on.push("音声");
        }
        if on.len() == 1 {
            "入力のみ".into()
        } else {
            on.join("・")
        }
    }
}

/// 指定文字列(未設定=None)からこの端末の範囲を決める。`bool` は指定が有効だったか
pub fn resolve(spec: Option<&str>) -> (Scope, bool) {
    match spec {
        None => (Scope::ALL, true),
        Some(s) => match Scope::parse(s) {
            Some(scope) => (scope, true),
            None => (Scope::INPUT_ONLY, false),
        },
    }
}

static ENV: OnceLock<(Scope, bool)> = OnceLock::new();
/// 設定画面での利用者の選択(クリップボード・ファイル)。既定は許可。環境変数の上限の内側でだけ効く。
/// 音声は既存の「音声の転送/再生」スイッチが利用者の選択にあたるため、ここでは持たない
static USER_CLIP: AtomicBool = AtomicBool::new(true);
static USER_FILES: AtomicBool = AtomicBool::new(true);

fn env_state() -> &'static (Scope, bool) {
    ENV.get_or_init(|| resolve(crate::envutil::get("KNIT_SHARE").as_deref()))
}

/// 環境変数 `KNIT_SHARE` が定める上限(起動中は変わらない)。設定画面ではこれを超えて許可できない
pub fn env_cap() -> Scope {
    env_state().0
}

pub fn set_user_clip(on: bool) {
    USER_CLIP.store(on, Ordering::Relaxed);
}
pub fn set_user_files(on: bool) {
    USER_FILES.store(on, Ordering::Relaxed);
}
pub fn user_clip() -> bool {
    USER_CLIP.load(Ordering::Relaxed)
}
pub fn user_files() -> bool {
    USER_FILES.load(Ordering::Relaxed)
}

/// いま有効な共有範囲(環境変数の上限 ∧ 利用者の選択)
pub fn local() -> Scope {
    effective(env_cap(), user_clip(), user_files())
}

/// 環境変数の上限と利用者の選択から、有効な範囲を決める
pub fn effective(cap: Scope, user_clip: bool, user_files: bool) -> Scope {
    Scope {
        clip: cap.clip && user_clip,
        files: cap.files && user_files,
        audio: cap.audio,
    }
}

/// `KNIT_SHARE` の指定が読めなかった(その場合の上限は入力のみ)
pub fn invalid_spec() -> bool {
    !env_state().1
}

pub fn allow_clip() -> bool {
    local().clip
}
pub fn allow_files() -> bool {
    local().files
}
pub fn allow_audio() -> bool {
    local().audio
}

/// 共有範囲が制限されている時の、起動ログ用の1行。制限が無ければ None
pub fn startup_note() -> Option<String> {
    let s = env_cap();
    if invalid_spec() {
        Some("[share] KNIT_SHARE を読めないため、入力のみにしました".into())
    } else if s != Scope::ALL {
        Some(format!("[share] この端末の共有範囲: {}", s.describe()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_and_all_share_everything() {
        assert_eq!(resolve(None), (Scope::ALL, true));
        assert_eq!(resolve(Some("all")), (Scope::ALL, true));
        assert_eq!(resolve(Some("  ALL ")), (Scope::ALL, true));
        assert_eq!(resolve(Some("")), (Scope::ALL, true));
    }

    #[test]
    fn input_only_and_lists() {
        assert_eq!(resolve(Some("input")).0, Scope::INPUT_ONLY);
        assert_eq!(resolve(Some("none")).0, Scope::INPUT_ONLY);
        let s = resolve(Some("clipboard, audio")).0;
        assert!(s.clip && s.audio && !s.files);
        let s = resolve(Some("Files+CLIP")).0;
        assert!(s.clip && s.files && !s.audio);
        assert_eq!(resolve(Some("input,files")).0, Scope { clip: false, files: true, audio: false });
    }

    #[test]
    fn unreadable_specs_fail_closed() {
        for bad in ["clipbord", "yes", "1", "clip;files", "allow"] {
            assert_eq!(resolve(Some(bad)), (Scope::INPUT_ONLY, false), "{bad}");
        }
    }

    #[test]
    fn describe_is_readable() {
        assert_eq!(Scope::ALL.describe(), "すべて共有");
        assert_eq!(Scope::INPUT_ONLY.describe(), "入力のみ");
        assert_eq!(Scope { clip: true, files: false, audio: true }.describe(), "入力・クリップボード・音声");
    }

    #[test]
    fn user_choice_narrows_within_the_environment_cap() {
        // 共有状態は触らない(並行する他のテストが送信を試すため)
        assert_eq!(effective(Scope::ALL, true, true), Scope::ALL);
        assert_eq!(effective(Scope::ALL, true, false), Scope { clip: true, files: false, audio: true });
        assert_eq!(effective(Scope::ALL, false, false), Scope { clip: false, files: false, audio: true });
        // 環境変数が禁じたものは、利用者が許可しても開かない
        let cap = Scope { clip: false, files: true, audio: false };
        assert_eq!(effective(cap, true, true), Scope { clip: false, files: true, audio: false });
        assert_eq!(effective(Scope::INPUT_ONLY, true, true), Scope::INPUT_ONLY);
    }
}
