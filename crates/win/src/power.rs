//! 起動時の電力スロットリング解除と優先度の引き上げ。
//! 仕様: docs/cleanroom/2026-10-29-win-power-throttling.md(クリーンルーム実装)
//!
//! トレイ常駐で表示中のウィンドウを持たないことが多いため、Windows 11 は Knit を
//! EcoQoS 扱いにしたりタイマー分解能の要求を無視したりすることがある(Microsoft Learn
//! 「Quality of Service」)。入力注入と生存確認の応答が間引かれないよう、起動時に一度だけ
//! 両方の自動制御を「オフ」に固定し、優先度クラスを一段だけ上げる。
//! どちらも失敗は致命的にしない(古い Windows では ProcessPowerThrottling が使えない)。

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, ProcessPowerThrottling, SetPriorityClass, SetProcessInformation,
    ABOVE_NORMAL_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
    PROCESS_POWER_THROTTLING_STATE,
};

/// 無効化用の環境変数(問題切り分け用)。"1" のときだけ何もしない
const DISABLE_ENV: &str = "KNIT_NO_POWER_TUNE";

/// 環境変数の値から「調整を無効化するか」を決める純関数。
/// 前後の空白は無視し、"1" のときだけ無効化する(未設定・空・その他の値は有効のまま)
fn tune_disabled(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.trim() == "1")
}

/// 渡す構造体の中身。ControlMask に両方のビットを立て、StateMask を 0 にすると
/// 「OS の自動判断をやめ、その機能をオフに固定する」意味になる
/// (Microsoft Learn の SetProcessInformation の HighQoS 例・タイマー分解能を常に尊重する例)
fn throttling_off_state() -> PROCESS_POWER_THROTTLING_STATE {
    PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        StateMask: 0,
    }
}

/// 起動時に一度だけ呼ぶ。スロットリング解除と優先度の引き上げを行い、結果を1行ずつログに出す
pub fn tune_at_startup() {
    if tune_disabled(knit_common::envutil::get(DISABLE_ENV).as_deref()) {
        println!("[power] {DISABLE_ENV}=1 のため電力調整を行いません");
        return;
    }
    let state = throttling_off_state();
    // SAFETY: GetCurrentProcess は擬似ハンドルを返すだけで閉じる必要が無い。
    // state は呼び出し中ずっと有効なスタック上の値で、サイズも構造体そのものを渡す
    let ok = unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            &state as *const PROCESS_POWER_THROTTLING_STATE as *const core::ffi::c_void,
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if ok != 0 {
        println!("[power] スロットリング解除: ok");
    } else {
        // SAFETY: 引数なしで直前の API のエラー値を読むだけ
        let err = unsafe { GetLastError() };
        println!("[power] スロットリング解除に失敗: err={err}(続行します)");
    }
    // HIGH / REALTIME は他のアプリを妨げるため使わず、一段だけ上げる
    // SAFETY: 自プロセスの擬似ハンドルと定義済みの定数を渡すだけ
    let ok = unsafe { SetPriorityClass(GetCurrentProcess(), ABOVE_NORMAL_PRIORITY_CLASS) };
    if ok != 0 {
        println!("[power] 優先度クラス ABOVE_NORMAL: ok");
    } else {
        // SAFETY: 引数なしで直前の API のエラー値を読むだけ
        let err = unsafe { GetLastError() };
        println!("[power] 優先度クラスの変更に失敗: err={err}(続行します)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_keeps_tuning_enabled() {
        assert!(!tune_disabled(None));
    }

    #[test]
    fn value_one_disables_tuning() {
        assert!(tune_disabled(Some("1")));
        assert!(tune_disabled(Some(" 1\n")));
    }

    #[test]
    fn other_values_keep_tuning_enabled() {
        for v in ["", "0", "true", "yes", "11", "01"] {
            assert!(!tune_disabled(Some(v)), "値 {v:?} で無効化されてしまった");
        }
    }

    #[test]
    fn state_turns_both_controls_off() {
        let s = throttling_off_state();
        assert_eq!(s.Version, PROCESS_POWER_THROTTLING_CURRENT_VERSION);
        assert_eq!(
            s.ControlMask,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION
        );
        assert_eq!(s.StateMask, 0);
    }
}
