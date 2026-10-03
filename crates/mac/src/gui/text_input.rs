//! Main-thread native text editor: macOS IME commits Unicode before transfer.
use super::*;
pub(super) unsafe extern "C" fn show(_s: ID, _c: SEL, _sender: ID) {
    if UI_PREVIEW.load(Ordering::Relaxed) {
        return;
    }
    let target = {
        let peers = crate::PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let active = *crate::ACTIVE_PEER.lock().unwrap_or_else(|e| e.into_inner());
        peers
            .get(active)
            .filter(|p| p.id.starts_with("android-app-"))
            .map(|p| (p.id.clone(), p.gen))
    };
    let Some((id, generation)) = target else {
        setup::error("文字入力はAndroid専用アプリへの接続中に使えます。");
        return;
    };
    crate::leave_win_mode_cursor_unlock(None);
    let app = msg0(
        objc_getClass(c"NSApplication".as_ptr()),
        sel(c"sharedApplication"),
    );
    msg1_void_u8(app, sel(c"activateIgnoringOtherApps:"), 1);
    let alert = msg0(objc_getClass(c"NSAlert".as_ptr()), sel(c"new"));
    msg1_void_id(
        alert,
        sel(c"setMessageText:"),
        nsstring("タブレットへ文字を入力"),
    );
    msg1_void_id(alert,sel(c"setInformativeText:"),nsstring("タブレットの入力欄を先に選び、Knitキーボードを使ってください。\nここではMacの日本語変換を使えます。変換を確定してから送信してください。"));
    crate::msg1_id(alert, sel(c"addButtonWithTitle:"), nsstring("送信"));
    crate::msg1_id(alert, sel(c"addButtonWithTitle:"), nsstring("戻る"));
    let init: unsafe extern "C" fn(ID, SEL, NSRect) -> ID =
        std::mem::transmute(crate::objc_msgSend as *const () as usize);
    let editor = init(
        msg0(objc_getClass(c"NSTextView".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:"),
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 500.0,
            h: 180.0,
        },
    );
    msg1_void_u8(editor, sel(c"setRichText:"), 0);
    msg1_void_u8(editor, sel(c"setEditable:"), 1);
    let font = msg1_id_f64(
        objc_getClass(c"NSFont".as_ptr()),
        sel(c"systemFontOfSize:"),
        16.0,
    );
    msg1_void_id(editor, sel(c"setFont:"), font);
    let scroll = init(
        msg0(objc_getClass(c"NSScrollView".as_ptr()), sel(c"alloc")),
        sel(c"initWithFrame:"),
        NSRect {
            x: 0.0,
            y: 0.0,
            w: 500.0,
            h: 180.0,
        },
    );
    msg1_void_u8(scroll, sel(c"setHasVerticalScroller:"), 1);
    msg1_void_id(scroll, sel(c"setDocumentView:"), editor);
    msg1_void_id(alert, sel(c"setAccessoryView:"), scroll);
    let window = msg0(alert, sel(c"window"));
    msg1_void_id(window, sel(c"setInitialFirstResponder:"), editor);
    let choice = crate::msg0_isize(alert, sel(c"runModal"));
    let raw = msg0_cstr(msg0(editor, sel(c"string")), sel(c"UTF8String"));
    if choice == 1000 && !raw.is_null() {
        let text = std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned();
        let valid = {
            let _change = crate::PEER_CHANGES
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let valid = crate::is_active_peer(&id, generation);
            if valid && !text.is_empty() && text.len() <= 1024 * 1024 {
                crate::send_msg(&knit_common::proto::Msg::Text { text });
            }
            valid
        };
        if !valid {
            setup::error("入力中に接続先が変わったため送信しませんでした。");
        }
    }
    msg0_void(editor, sel(c"release"));
    msg0_void(scroll, sel(c"release"));
    msg0_void(alert, sel(c"release"));
}
