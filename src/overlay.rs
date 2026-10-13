use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::Instant;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, EndPaint,
    FillRect, FrameRect, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TextOutW,
    FW_BOLD, HBRUSH, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, PostMessageW,
    PostQuitMessage, RegisterClassExW, SetLayeredWindowAttributes, SetTimer, SetWindowPos,
    ShowWindow, HWND_TOPMOST, LWA_ALPHA, MSG, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
    WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_TIMER, WM_USER, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const WM_SHOW_DPI: u32 = WM_USER + 101;
const TIMER_FADE: usize = 1;

#[derive(Debug, Clone)]
pub struct DpiDisplayInfo {
    pub stage: usize,       // 1..=5
    pub dpi_val: u32,       // e.g. 1000
    pub color_rgb: [u8; 3], // RGB of stage
}

pub enum OsdCommand {
    Show(DpiDisplayInfo),
    #[allow(dead_code)]
    Quit,
}

#[derive(Clone)]
pub struct OsdController {
    tx: Sender<OsdCommand>,
}

impl OsdController {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        thread::Builder::new()
            .name("osd_overlay_thread".to_string())
            .spawn(move || {
                run_overlay_window(rx);
            })
            .expect("Failed to spawn OSD overlay thread");

        Self { tx }
    }

    pub fn show_dpi(&self, stage: usize, dpi_val: u32, color_rgb: [u8; 3]) {
        let _ = self.tx.send(OsdCommand::Show(DpiDisplayInfo {
            stage,
            dpi_val,
            color_rgb,
        }));
    }
}

// Thread-safe state for the window
struct WindowState {
    info: DpiDisplayInfo,
    show_time: Instant,
    alpha: u8,
}

static GLOBAL_STATE: Mutex<Option<WindowState>> = Mutex::new(None);

fn run_overlay_window(rx: Receiver<OsdCommand>) {
    unsafe {
        let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)
            .map(|h| HINSTANCE(h.0))
            .unwrap_or(HINSTANCE(std::ptr::null_mut()));

        let class_name = w!("RedragonDpiOsdClass");
        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: windows::Win32::UI::WindowsAndMessaging::WNDCLASS_STYLES(0),
            lpfnWndProc: Some(osd_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: windows::Win32::UI::WindowsAndMessaging::HICON(std::ptr::null_mut()),
            hCursor: windows::Win32::UI::WindowsAndMessaging::HCURSOR(std::ptr::null_mut()),
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszMenuName: windows::core::PCWSTR::null(),
            lpszClassName: class_name,
            hIconSm: windows::Win32::UI::WindowsAndMessaging::HICON(std::ptr::null_mut()),
        };

        RegisterClassExW(&wnd_class);

        let width = 210;
        let height = 95;
        let pos_x = 40;
        let pos_y = 60; // 60px from top, 40px from left (left side of screen like OEM)

        let ex_style = WS_EX_TOPMOST
            | WS_EX_TOOLWINDOW
            | WS_EX_LAYERED
            | WS_EX_NOACTIVATE
            | WS_EX_TRANSPARENT;
        let style = WS_POPUP;

        let hwnd = match CreateWindowExW(
            ex_style,
            class_name,
            w!("Redragon DPI Overlay"),
            style,
            pos_x,
            pos_y,
            width,
            height,
            HWND(std::ptr::null_mut()),
            windows::Win32::UI::WindowsAndMessaging::HMENU(std::ptr::null_mut()),
            hinstance,
            None,
        ) {
            Ok(h) => h,
            Err(_) => return,
        };

        // Apply smooth rounded corners (14px)
        let rgn = CreateRoundRectRgn(0, 0, width, height, 14, 14);
        if !rgn.0.is_null() {
            SetWindowRgn(hwnd, rgn, true);
        }

        // Initially hidden
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
        let _ = ShowWindow(hwnd, SW_HIDE);

        // Bridge commands from receiver into Win32 messages
        let hwnd_raw = hwnd.0 as usize;
        thread::spawn(move || {
            let hwnd = HWND(hwnd_raw as *mut _);
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    OsdCommand::Show(info) => {
                        if let Ok(mut lock) = GLOBAL_STATE.lock() {
                            *lock = Some(WindowState {
                                info,
                                show_time: Instant::now(),
                                alpha: 235,
                            });
                        }
                        let _ = PostMessageW(hwnd, WM_SHOW_DPI, WPARAM(0), LPARAM(0));
                    }
                    OsdCommand::Quit => {
                        let _ = PostMessageW(hwnd, WM_DESTROY, WPARAM(0), LPARAM(0));
                        break;
                    }
                }
            }
        });

        // Win32 Message Loop
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND(std::ptr::null_mut()), 0, 0).as_bool() {
            let _ = DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn osd_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_SHOW_DPI => {
            let mut alpha = 0u8;
            if let Ok(mut lock) = GLOBAL_STATE.lock() {
                if let Some(state) = lock.as_mut() {
                    state.show_time = Instant::now();
                    state.alpha = 235;
                    alpha = state.alpha;
                }
            }

            if alpha > 0 {
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
                let _ = SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    40,
                    60,
                    210,
                    95,
                    SWP_SHOWWINDOW | SWP_NOACTIVATE,
                );
                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(hwnd, None, false);
                let _ = SetTimer(hwnd, TIMER_FADE, 25, None);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_FADE {
                let mut should_hide = false;
                let mut new_alpha = 0u8;

                if let Ok(mut lock) = GLOBAL_STATE.lock() {
                    if let Some(state) = lock.as_mut() {
                        let elapsed = state.show_time.elapsed().as_secs_f32();
                        if elapsed > 1.6 {
                            // Fade out over 0.5s
                            let fade_elapsed = elapsed - 1.6;
                            if fade_elapsed >= 0.5 {
                                state.alpha = 0;
                                should_hide = true;
                            } else {
                                let ratio = 1.0 - (fade_elapsed / 0.5);
                                state.alpha = ((235.0 * ratio).clamp(0.0, 235.0)) as u8;
                                new_alpha = state.alpha;
                            }
                        } else {
                            new_alpha = state.alpha;
                        }
                    }
                }

                if should_hide {
                    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    let _ = KillTimer(hwnd, TIMER_FADE);
                } else if new_alpha > 0 {
                    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), new_alpha, LWA_ALPHA);
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            if let Ok(lock) = GLOBAL_STATE.lock() {
                if let Some(state) = lock.as_ref() {
                    let rect = RECT {
                        left: 0,
                        top: 0,
                        right: 210,
                        bottom: 95,
                    };

                    // 1. Dark sleek background #14161C
                    let bg_brush = CreateSolidBrush(COLORREF(0x001C1614)); // BGR format
                    FillRect(hdc, &rect, bg_brush);
                    let _ = DeleteObject(bg_brush);

                    // 2. Stage accent color border
                    let [r, g, b] = state.info.color_rgb;
                    let bgr = (b as u32) << 16 | (g as u32) << 8 | (r as u32);
                    let border_brush = CreateSolidBrush(COLORREF(bgr));
                    FrameRect(hdc, &rect, border_brush);

                    // 3. Left indicator color stripe (width 6px)
                    let stripe = RECT {
                        left: 0,
                        top: 0,
                        right: 6,
                        bottom: 95,
                    };
                    FillRect(hdc, &stripe, border_brush);
                    let _ = DeleteObject(border_brush);

                    // 4. Draw Text
                    SetBkMode(hdc, TRANSPARENT);

                    // Stage Title: "DPI STAGE X"
                    let title_font = CreateFontW(
                        -14, // height
                        0,
                        0,
                        0,
                        FW_BOLD.0 as i32,
                        0,
                        0,
                        0,
                        1, // DEFAULT_CHARSET
                        0,
                        0,
                        0,
                        0,
                        w!("Segoe UI"),
                    );
                    let old_font = SelectObject(hdc, title_font);
                    SetTextColor(hdc, COLORREF(bgr)); // Use stage accent color

                    let title_text = format!("DPI STAGE {}", state.info.stage);
                    let title_utf16: Vec<u16> = title_text.encode_utf16().collect();
                    let _ = TextOutW(hdc, 20, 14, &title_utf16);

                    // DPI Value: "XXXX DPI"
                    let val_font = CreateFontW(
                        -26, // large font
                        0,
                        0,
                        0,
                        FW_BOLD.0 as i32,
                        0,
                        0,
                        0,
                        1,
                        0,
                        0,
                        0,
                        0,
                        w!("Segoe UI"),
                    );
                    SelectObject(hdc, val_font);
                    SetTextColor(hdc, COLORREF(0x00FFFFFF)); // White text

                    let val_text = format!("{} DPI", state.info.dpi_val);
                    let val_utf16: Vec<u16> = val_text.encode_utf16().collect();
                    let _ = TextOutW(hdc, 20, 36, &val_utf16);

                    // 5. Five-segment stage visualizer bars
                    for s in 1..=5 {
                        let seg_x = 20 + ((s - 1) as i32 * 34);
                        let seg_rect = RECT {
                            left: seg_x,
                            top: 74,
                            right: seg_x + 28,
                            bottom: 80,
                        };
                        let seg_bgr = if s == state.info.stage {
                            bgr // highlighted active stage
                        } else {
                            0x00403830 // inactive gray bar
                        };
                        let bar_brush = CreateSolidBrush(COLORREF(seg_bgr));
                        FillRect(hdc, &seg_rect, bar_brush);
                        let _ = DeleteObject(bar_brush);
                    }

                    // Cleanup fonts
                    SelectObject(hdc, old_font);
                    let _ = DeleteObject(title_font);
                    let _ = DeleteObject(val_font);
                }
            }

            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1), // Avoid flicker
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
