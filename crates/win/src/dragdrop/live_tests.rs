//! 明示実行する対話セッション試験。専用ウィンドウと一時ファイルだけを使い、
//! 本番の共有入力経路(remote_mouse_*)で掴み越境ドラッグのドロップを検証する。
use super::*;
use std::time::{Duration, Instant};
use windows_sys::Win32::System::Ole::{RegisterDragDrop, RevokeDragDrop};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[link(name = "uuid", kind = "static")]
unsafe extern "system" {
    #[link_name = "IID_IShellItem"]
    static SHELL_ITEM: Guid;
    #[link_name = "IID_IDropTarget"]
    static DROP_TARGET: Guid;
    #[link_name = "BHID_SFUIObject"]
    static SHELL_UI: Guid;
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn SHCreateItemFromParsingName(
        name: *const u16,
        context: *mut c_void,
        iid: *const Guid,
        out: *mut *mut c_void,
    ) -> HRESULT;
}
#[repr(C)]
struct UnknownVtbl {
    qi: QiFn,
    add_ref: AddRefFn,
    release: ReleaseFn,
}
#[repr(C)]
struct ShellVtbl {
    unknown: UnknownVtbl,
    bind: unsafe extern "system" fn(
        *mut c_void,
        *mut c_void,
        *const Guid,
        *const Guid,
        *mut *mut c_void,
    ) -> HRESULT,
}
struct Ptr(*mut c_void);
impl Drop for Ptr {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                ((**(self.0 as *const *const UnknownVtbl)).release)(self.0);
            }
        }
    }
}
struct Cleanup {
    window: windows_sys::Win32::Foundation::HWND,
    foreground: windows_sys::Win32::Foundation::HWND,
    cursor: windows_sys::Win32::Foundation::POINT,
    dir: std::path::PathBuf,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        unsafe {
            // 押しっぱなし残留を本番経路の解放(=OLE中継を含む)で潰す
            crate::remote_mouse_button(0, false);
            RevokeDragDrop(self.window);
            DestroyWindow(self.window);
            SetCursorPos(self.cursor.x, self.cursor.y);
            SetForegroundWindow(self.foreground);
            let _ = std::fs::remove_dir_all(&self.dir);
            OleUninitialize();
        }
    }
}

#[test]
#[ignore = "Windows対話セッションで専用ウィンドウへマウス操作を注入する"]
fn shared_mouse_release_completes_native_shell_drop() {
    unsafe {
        let idle_deadline = Instant::now() + Duration::from_secs(10);
        while GetAsyncKeyState(1) < 0 && Instant::now() < idle_deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(GetAsyncKeyState(1) >= 0, "マウスを離して実行する");
        assert!(OleInitialize(std::ptr::null()) >= 0);
        let foreground = GetForegroundWindow();
        let mut cursor = std::mem::zeroed();
        GetCursorPos(&mut cursor);
        let dir = std::env::temp_dir().join(format!("tsunagu-live-drop-{}", std::process::id()));
        let dest = dir.join("destination");
        std::fs::create_dir_all(&dest).unwrap();
        let source = dir.join("操作テスト.txt");
        let contents = b"native mouse release must complete this exact copy\n";
        std::fs::write(&source, contents).unwrap();
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let title: Vec<u16> = "Tsunagu drag verification\0".encode_utf16().collect();
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_VISIBLE,
            80,
            80,
            400,
            300,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        assert!(!window.is_null());
        let _cleanup = Cleanup {
            window,
            foreground,
            cursor,
            dir,
        };
        SetForegroundWindow(window);
        assert_eq!(
            GetForegroundWindow(),
            window,
            "テスト用窓を前面にできる対話セッションが必要"
        );
        let name: Vec<u16> = dest.to_string_lossy().encode_utf16().chain([0]).collect();
        let mut item = Ptr(std::ptr::null_mut());
        assert_eq!(
            SHCreateItemFromParsingName(
                name.as_ptr(),
                std::ptr::null_mut(),
                &SHELL_ITEM,
                &mut item.0
            ),
            S_OK
        );
        let mut target = Ptr(std::ptr::null_mut());
        assert_eq!(
            ((**(item.0 as *const *const ShellVtbl)).bind)(
                item.0,
                std::ptr::null_mut(),
                &SHELL_UI,
                &DROP_TARGET,
                &mut target.0
            ),
            S_OK
        );
        assert_eq!(RegisterDragDrop(window, target.0), S_OK);
        SetCursorPos(150, 150);
        crate::CONNECTED.store(true, Ordering::Relaxed);
        edge::CONTROLLED.store(true, Ordering::Relaxed);
        // Mac が押下を転送した状態(掴んだまま)を本番経路で作る
        crate::remote_mouse_button(0, true);
        start(vec![source.to_string_lossy().into_owned()]);
        // ドライバも本番経路だけを使う: SendInput と OLE 中継がセットの
        // remote_mouse_* で移動を運び、ボタン解放でドロップさせる
        let driver = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(400));
            for _ in 0..40 {
                SetCursorPos(220, 220);
                crate::remote_mouse_move_rel(1, 1);
                std::thread::sleep(Duration::from_millis(50));
                if TEST_DRAG_READY.load(Ordering::Relaxed) {
                    break;
                }
            }
            println!(
                "[drag-test] native feedback ready={}",
                TEST_DRAG_READY.load(Ordering::Relaxed)
            );
            std::thread::sleep(Duration::from_millis(200));
            SetCursorPos(240, 240);
            crate::remote_mouse_move_rel(2, 2);
            std::thread::sleep(Duration::from_millis(200));
            crate::remote_mouse_button(0, false);
        });
        let until = Instant::now() + Duration::from_secs(12);
        let delivered = dest.join("操作テスト.txt");
        while Instant::now() < until && !delivered.exists() {
            let mut msg = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        driver.join().unwrap();
        assert!(
            delivered.exists(),
            "ボタン解放後もOSのドロップが完了していない"
        );
        assert_eq!(std::fs::read(delivered).unwrap(), contents);
        assert_eq!(std::fs::read(source).unwrap(), contents, "原本を保持する");
    }
}
