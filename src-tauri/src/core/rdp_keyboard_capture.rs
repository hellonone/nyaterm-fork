use crate::core::rdp::{RdpInputEvent, RdpSessionManager};
use crate::error::AppResult;
use std::sync::Arc;

#[cfg(windows)]
mod platform {
    use super::*;
    use crate::error::AppError;
    use std::collections::HashSet;
    use std::ptr::null_mut;
    use std::sync::{Mutex as StdMutex, OnceLock, Weak};
    use std::thread;
    use windows_sys::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_LWIN, VK_RWIN};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
        LLKHF_UP, MSG, SetWindowsHookExW, TranslateMessage, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
        WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    static CAPTURE: OnceLock<Arc<CaptureState>> = OnceLock::new();
    static HOOK_THREAD: OnceLock<()> = OnceLock::new();

    struct CaptureState {
        manager: StdMutex<Weak<RdpSessionManager>>,
        session_id: StdMutex<Option<String>>,
        win_key_down: StdMutex<bool>,
        captured_keys: StdMutex<HashSet<(u16, bool)>>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) struct CapturedKeyEvent {
        pub scan_code: u16,
        pub extended: bool,
        pub pressed: bool,
    }

    pub(super) fn set_keyboard_capture(
        manager: Arc<RdpSessionManager>,
        session_id: Option<String>,
    ) -> AppResult<()> {
        let next_session_id = session_id.clone();
        let state = CAPTURE
            .get_or_init(|| {
                Arc::new(CaptureState {
                    manager: StdMutex::new(Weak::new()),
                    session_id: StdMutex::new(None),
                    win_key_down: StdMutex::new(false),
                    captured_keys: StdMutex::new(HashSet::new()),
                })
            })
            .clone();

        *state.manager.lock().map_err(|_| {
            AppError::Channel("RDP keyboard capture state is poisoned".to_string())
        })? = Arc::downgrade(&manager);
        let previous_session_id = {
            let mut current = state.session_id.lock().map_err(|_| {
                AppError::Channel("RDP keyboard capture session state is poisoned".to_string())
            })?;
            let previous = current.clone();
            *current = session_id;
            previous
        };

        if previous_session_id != next_session_id {
            let should_release_previous = reset_pressed_state(&state)?;
            if should_release_previous {
                if let Some(previous_session_id) = previous_session_id {
                    let manager = manager.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = manager
                            .send_input(&previous_session_id, vec![RdpInputEvent::ReleaseAllKeys])
                            .await;
                    });
                }
            }
        }

        if next_session_id.is_some() {
            ensure_hook_thread_started();
        }
        Ok(())
    }

    fn reset_pressed_state(state: &CaptureState) -> AppResult<bool> {
        let mut win_key_down = state.win_key_down.lock().map_err(|_| {
            AppError::Channel("RDP keyboard capture key state is poisoned".to_string())
        })?;
        let mut captured_keys = state.captured_keys.lock().map_err(|_| {
            AppError::Channel("RDP keyboard capture captured-key state is poisoned".to_string())
        })?;
        let had_pressed_keys = *win_key_down || !captured_keys.is_empty();
        *win_key_down = false;
        captured_keys.clear();
        Ok(had_pressed_keys)
    }

    fn ensure_hook_thread_started() {
        HOOK_THREAD.get_or_init(|| {
            let _ = thread::Builder::new()
                .name("rdp-keyboard-capture".to_string())
                .spawn(hook_thread);
        });
    }

    fn hook_thread() {
        let hook = unsafe {
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(keyboard_hook_proc),
                null_mut::<std::ffi::c_void>() as HINSTANCE,
                0,
            )
        };
        if hook.is_null() {
            tracing::warn!("Failed to install RDP keyboard capture hook");
            return;
        }

        let mut message = MSG::default();
        while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    unsafe extern "system" fn keyboard_hook_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code < 0 {
            return unsafe {
                CallNextHookEx(
                    null_mut::<std::ffi::c_void>() as HHOOK,
                    code,
                    wparam,
                    lparam,
                )
            };
        }

        let Some(state) = CAPTURE.get() else {
            return unsafe {
                CallNextHookEx(
                    null_mut::<std::ffi::c_void>() as HHOOK,
                    code,
                    wparam,
                    lparam,
                )
            };
        };

        let Some(event) = (unsafe { (lparam as *const KBDLLHOOKSTRUCT).as_ref() })
            .and_then(|raw| captured_key_event(wparam as u32, raw.vkCode, raw.scanCode, raw.flags))
        else {
            return unsafe {
                CallNextHookEx(
                    null_mut::<std::ffi::c_void>() as HHOOK,
                    code,
                    wparam,
                    lparam,
                )
            };
        };

        let should_capture = update_win_key_state_and_should_capture(state, event.clone());
        if !should_capture {
            return unsafe {
                CallNextHookEx(
                    null_mut::<std::ffi::c_void>() as HHOOK,
                    code,
                    wparam,
                    lparam,
                )
            };
        }

        if let Some((manager, session_id)) = capture_target(state) {
            let input = if event.pressed {
                RdpInputEvent::KeyDown {
                    scan_code: event.scan_code,
                    extended: event.extended,
                    repeat: false,
                }
            } else {
                RdpInputEvent::KeyUp {
                    scan_code: event.scan_code,
                    extended: event.extended,
                    repeat: false,
                }
            };
            tauri::async_runtime::spawn(async move {
                let _ = manager.send_input(&session_id, vec![input]).await;
            });
            return 1;
        }

        unsafe {
            CallNextHookEx(
                null_mut::<std::ffi::c_void>() as HHOOK,
                code,
                wparam,
                lparam,
            )
        }
    }

    fn update_win_key_state_and_should_capture(
        state: &CaptureState,
        event: CapturedKeyEvent,
    ) -> bool {
        let Ok(mut win_key_down) = state.win_key_down.lock() else {
            return false;
        };
        let Ok(mut captured_keys) = state.captured_keys.lock() else {
            return false;
        };
        let was_win_down = *win_key_down;
        let is_win_key = is_windows_scan_code(event.scan_code, event.extended);
        if is_win_key {
            *win_key_down = event.pressed;
            return true;
        }
        let key = (event.scan_code, event.extended);
        if event.pressed && was_win_down {
            captured_keys.insert(key);
            return true;
        }
        if !event.pressed && captured_keys.remove(&key) {
            return true;
        }
        false
    }

    fn capture_target(state: &CaptureState) -> Option<(Arc<RdpSessionManager>, String)> {
        let session_id = state.session_id.lock().ok()?.clone()?;
        let manager = state.manager.lock().ok()?.upgrade()?;
        Some((manager, session_id))
    }

    fn captured_key_event(
        message: u32,
        vk_code: u32,
        raw_scan_code: u32,
        flags: u32,
    ) -> Option<CapturedKeyEvent> {
        let pressed = match message {
            WM_KEYDOWN | WM_SYSKEYDOWN => true,
            WM_KEYUP | WM_SYSKEYUP => false,
            _ => return None,
        };
        let (scan_code, forced_extended) = scan_code_from_vk(vk_code)?;
        let raw_scan_code = (raw_scan_code & 0xff) as u16;
        let scan_code = if raw_scan_code == 0 {
            scan_code
        } else {
            raw_scan_code
        };
        Some(CapturedKeyEvent {
            scan_code,
            extended: forced_extended || flags & LLKHF_EXTENDED != 0,
            pressed: pressed && flags & LLKHF_UP == 0,
        })
    }

    fn scan_code_from_vk(vk_code: u32) -> Option<(u16, bool)> {
        match vk_code as u16 {
            VK_LWIN => Some((0x5b, true)),
            VK_RWIN => Some((0x5c, true)),
            0x08 => Some((0x0e, false)),
            0x09 => Some((0x0f, false)),
            0x0d => Some((0x1c, false)),
            0x1b => Some((0x01, false)),
            0x20 => Some((0x39, false)),
            0x21 => Some((0x49, true)),
            0x22 => Some((0x51, true)),
            0x23 => Some((0x4f, true)),
            0x24 => Some((0x47, true)),
            0x25 => Some((0x4b, true)),
            0x26 => Some((0x48, true)),
            0x27 => Some((0x4d, true)),
            0x28 => Some((0x50, true)),
            0x2d => Some((0x52, true)),
            0x2e => Some((0x53, true)),
            0x30 => Some((0x0b, false)),
            0x31 => Some((0x02, false)),
            0x32 => Some((0x03, false)),
            0x33 => Some((0x04, false)),
            0x34 => Some((0x05, false)),
            0x35 => Some((0x06, false)),
            0x36 => Some((0x07, false)),
            0x37 => Some((0x08, false)),
            0x38 => Some((0x09, false)),
            0x39 => Some((0x0a, false)),
            0x41 => Some((0x1e, false)),
            0x42 => Some((0x30, false)),
            0x43 => Some((0x2e, false)),
            0x44 => Some((0x20, false)),
            0x45 => Some((0x12, false)),
            0x46 => Some((0x21, false)),
            0x47 => Some((0x22, false)),
            0x48 => Some((0x23, false)),
            0x49 => Some((0x17, false)),
            0x4a => Some((0x24, false)),
            0x4b => Some((0x25, false)),
            0x4c => Some((0x26, false)),
            0x4d => Some((0x32, false)),
            0x4e => Some((0x31, false)),
            0x4f => Some((0x18, false)),
            0x50 => Some((0x19, false)),
            0x51 => Some((0x10, false)),
            0x52 => Some((0x13, false)),
            0x53 => Some((0x1f, false)),
            0x54 => Some((0x14, false)),
            0x55 => Some((0x16, false)),
            0x56 => Some((0x2f, false)),
            0x57 => Some((0x11, false)),
            0x58 => Some((0x2d, false)),
            0x59 => Some((0x15, false)),
            0x5a => Some((0x2c, false)),
            0x70..=0x79 => Some(((vk_code - 0x70 + 0x3b) as u16, false)),
            0x7a => Some((0x57, false)),
            0x7b => Some((0x58, false)),
            0xa0 => Some((0x2a, false)),
            0xa1 => Some((0x36, false)),
            0xa2 => Some((0x1d, false)),
            0xa3 => Some((0x1d, true)),
            0xa4 => Some((0x38, false)),
            0xa5 => Some((0x38, true)),
            0xba => Some((0x27, false)),
            0xbb => Some((0x0d, false)),
            0xbc => Some((0x33, false)),
            0xbd => Some((0x0c, false)),
            0xbe => Some((0x34, false)),
            0xbf => Some((0x35, false)),
            0xc0 => Some((0x29, false)),
            0xdb => Some((0x1a, false)),
            0xdc => Some((0x2b, false)),
            0xdd => Some((0x1b, false)),
            0xde => Some((0x28, false)),
            _ => None,
        }
    }

    fn is_windows_scan_code(scan_code: u16, extended: bool) -> bool {
        extended && matches!(scan_code, 0x5b | 0x5c)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn captures_left_and_right_windows_keys_as_extended() {
            assert_eq!(
                captured_key_event(WM_KEYDOWN, u32::from(VK_LWIN), 0x5b, LLKHF_EXTENDED),
                Some(CapturedKeyEvent {
                    scan_code: 0x5b,
                    extended: true,
                    pressed: true,
                })
            );
            assert_eq!(
                captured_key_event(
                    WM_KEYUP,
                    u32::from(VK_RWIN),
                    0x5c,
                    LLKHF_EXTENDED | LLKHF_UP
                ),
                Some(CapturedKeyEvent {
                    scan_code: 0x5c,
                    extended: true,
                    pressed: false,
                })
            );
        }

        #[test]
        fn converts_common_win_combinations_to_rdp_scan_codes() {
            assert_eq!(
                captured_key_event(WM_KEYDOWN, u32::from(b'R'), 0x13, 0),
                Some(CapturedKeyEvent {
                    scan_code: 0x13,
                    extended: false,
                    pressed: true,
                })
            );
            assert_eq!(
                captured_key_event(WM_KEYUP, 0x25, 0x4b, LLKHF_EXTENDED | LLKHF_UP),
                Some(CapturedKeyEvent {
                    scan_code: 0x4b,
                    extended: true,
                    pressed: false,
                })
            );
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub(super) fn set_keyboard_capture(
        _manager: Arc<RdpSessionManager>,
        _session_id: Option<String>,
    ) -> AppResult<()> {
        Ok(())
    }
}

pub fn set_keyboard_capture(
    manager: Arc<RdpSessionManager>,
    session_id: Option<String>,
) -> AppResult<()> {
    platform::set_keyboard_capture(manager, session_id)
}
