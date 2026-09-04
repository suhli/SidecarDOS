use crate::config::Position;
use anyhow::{Result, ensure};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::mpsc;
use windows::{
    Win32::{
        Foundation::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, w},
};
#[derive(Clone, Copy)]
pub enum Action {
    Disconnect,
    Position(Position),
    Quality(u8),
    Exit,
}
#[derive(Clone)]
pub struct Ui {
    pub exit: Arc<AtomicBool>,
    pub display_changed: Arc<AtomicBool>,
    text: Arc<Mutex<String>>,
    pair: Arc<Mutex<Option<String>>>,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            exit: Arc::new(AtomicBool::new(false)),
            display_changed: Arc::new(AtomicBool::new(false)),
            text: Arc::new(Mutex::new("SidecarDOS — Available".into())),
            pair: Arc::new(Mutex::new(None)),
        }
    }
}
impl Ui {
    pub fn status(&self, text: &str) {
        if let Ok(mut t) = self.text.lock() {
            *t = text.into();
        }
    }
    pub fn pairing(&self, name: &str, code: &str) {
        if let Ok(mut p) = self.pair.lock() {
            *p = Some(format!(
                "Pair with {name}\n\nEnter this one-time code on your iPad:\n\n{code}\n\nThis request expires after 90 seconds."
            ));
        }
    }
    pub fn clear_pairing(&self) {
        if let Ok(mut p) = self.pair.lock() {
            *p = None;
        }
    }
}
struct Tray {
    ui: Ui,
    actions: mpsc::Sender<Action>,
    last_pair: Option<String>,
    pair_window: Option<HWND>,
    config_path: Vec<u16>,
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}
unsafe extern "system" fn procedure(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Tray;
        if msg == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        if !ptr.is_null() {
            let t = &mut *ptr;
            match msg {
                WM_DISPLAYCHANGE => {
                    t.ui.display_changed.store(true, Ordering::Relaxed);
                    return LRESULT(0);
                }
                WM_TIMER => {
                    let pairing = t.ui.pair.lock().ok().and_then(|p| p.clone());
                    if pairing != t.last_pair {
                        if let Some(old) = t.pair_window.take() {
                            let _ = DestroyWindow(old);
                        }
                        t.last_pair = pairing.clone();
                        if let Some(p) = pairing {
                            let text = wide(&p);
                            // Non-modal owned window avoids blocking network / tray event dispatch.
                            t.pair_window = CreateWindowExW(
                                WS_EX_TOPMOST,
                                w!("STATIC"),
                                PCWSTR(text.as_ptr()),
                                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                                CW_USEDEFAULT,
                                CW_USEDEFAULT,
                                600,
                                240,
                                Some(hwnd),
                                None,
                                None,
                                None,
                            )
                            .ok();
                        }
                    }
                    if t.ui.exit.load(Ordering::Relaxed) {
                        let _ = DestroyWindow(hwnd);
                    }
                    return LRESULT(0);
                }
                m if m == WM_APP + 1
                    && (lparam.0 as u32 == WM_RBUTTONUP || lparam.0 as u32 == WM_LBUTTONUP) =>
                {
                    if let Ok(menu) = CreatePopupMenu() {
                        let label = wide(&t.ui.text.lock().map(|s| s.clone()).unwrap_or_default());
                        let _ =
                            AppendMenuW(menu, MF_STRING | MF_DISABLED, 0, PCWSTR(label.as_ptr()));
                        for (id, label) in [
                            (1, "Disconnect"),
                            (10, "Display: Left"),
                            (11, "Display: Right"),
                            (12, "Display: Above"),
                            (13, "Display: Below"),
                            (20, "Quality: Auto"),
                            (21, "Quality: Performance"),
                            (22, "Quality: Quality"),
                            (30, "Settings"),
                            (99, "Exit"),
                        ] {
                            let s = wide(label);
                            let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(s.as_ptr()));
                        }
                        let mut point = POINT::default();
                        let _ = GetCursorPos(&mut point);
                        let _ = SetForegroundWindow(hwnd);
                        let choice =
                            TrackPopupMenu(menu, TPM_RETURNCMD, point.x, point.y, None, hwnd, None)
                                .0 as u32;
                        let _ = DestroyMenu(menu);
                        let action = match choice {
                            1 => Some(Action::Disconnect),
                            10 => Some(Action::Position(Position::Left)),
                            11 => Some(Action::Position(Position::Right)),
                            12 => Some(Action::Position(Position::Above)),
                            13 => Some(Action::Position(Position::Below)),
                            20..=22 => Some(Action::Quality((choice - 20) as u8)),
                            99 => {
                                t.ui.exit.store(true, Ordering::Relaxed);
                                Some(Action::Exit)
                            }
                            30 => {
                                ShellExecuteW(
                                    Some(hwnd),
                                    w!("open"),
                                    w!("notepad.exe"),
                                    PCWSTR(t.config_path.as_ptr()),
                                    None,
                                    SW_SHOW,
                                );
                                None
                            }
                            _ => None,
                        };
                        if let Some(a) = action {
                            let _ = t.actions.try_send(a);
                        }
                    }
                    return LRESULT(0);
                }
                WM_DESTROY => {
                    let data = NOTIFYICONDATAW {
                        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                        hWnd: hwnd,
                        uID: 1,
                        ..Default::default()
                    };
                    let _ = Shell_NotifyIconW(NIM_DELETE, &data);
                    PostQuitMessage(0);
                    return LRESULT(0);
                }
                _ => {}
            }
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
pub fn run(ui: Ui, actions: mpsc::Sender<Action>, config_path: &std::path::Path) -> Result<()> {
    unsafe {
        let module = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: module.into(),
            lpszClassName: w!("SidecarDOS.Tray"),
            ..Default::default()
        };
        ensure!(
            RegisterClassW(&class) != 0,
            "could not register tray window"
        );
        let mut tray = Box::new(Tray {
            ui,
            actions,
            last_pair: None,
            pair_window: None,
            config_path: wide(&format!("\"{}\"", config_path.display())),
        });
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("SidecarDOS.Tray"),
            w!("SidecarDOS"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
            Some((&mut *tray as *mut Tray).cast()),
        )?;
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_APP + 1,
            hIcon: LoadIconW(None, IDI_APPLICATION)?,
            ..Default::default()
        };
        let tip = wide("SidecarDOS");
        data.szTip[..tip.len()].copy_from_slice(&tip);
        ensure!(
            Shell_NotifyIconW(NIM_ADD, &data).as_bool(),
            "could not add tray icon"
        );
        SetTimer(Some(hwnd), 1, 250, None);
        let mut msg = MSG::default();
        loop {
            let result = GetMessageW(&mut msg, None, 0, 0).0;
            if result == 0 {
                break;
            }
            ensure!(result != -1, "tray message loop failed");
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok(())
    }
}
