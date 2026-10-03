//! ファイル転送のキャンセル要求を、UI(メニュー操作・Esc)や相手からの
//! 中止メッセージ(DragCancel)から送信スレッドへ届ける。
//! 要求は転送IDで区別し、送信ループは毎チャンク確認する。中止は接続の切断として
//! 伝播する(受信側は既存の切断時クリーンアップで部分ファイルを全て削除する)。

use std::sync::Mutex;

static REQUESTS: Mutex<Vec<u64>> = Mutex::new(Vec::new());

/// 転送 id の中止を要求する(重複して登録しない)
pub fn request(id: u64) {
    let mut v = REQUESTS.lock().unwrap_or_else(|e| e.into_inner());
    if !v.contains(&id) {
        v.push(id);
        // 消費する送信スレッドを失った id(旧版や片方向の中止要求)が一覧を
        // 占め続けないよう上限を置く
        if v.len() > 64 {
            v.remove(0);
        }
    }
}

/// id の中止要求があれば消費して true。送信ループの毎チャンク判定に使う
pub fn take(id: u64) -> bool {
    let mut v = REQUESTS.lock().unwrap_or_else(|e| e.into_inner());
    match v.iter().position(|&x| x == id) {
        Some(i) => {
            v.swap_remove(i);
            true
        }
        None => false,
    }
}

/// 転送が完了・失敗した時点で残った要求を掃除する(次の同名 id は無いが、
/// 一覧が伸び続けないようにする)
pub fn discard(id: u64) {
    REQUESTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|&x| x != id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_is_consumed_once_and_discarded_on_finish() {
        assert!(!take(7), "要求なしは false");
        request(7);
        request(7);
        assert!(take(7), "要求ありは true");
        assert!(!take(7), "要求は一度だけ消費される");
        request(8);
        discard(8);
        assert!(!take(8), "完了時の掃除で残要求は消える");
    }
}
