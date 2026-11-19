//! Native controls remain accessible to keyboard navigation and Windows UI Automation.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::{
    ble::{Engine, Wake},
    config::{self, SavedPeer},
    i18n::{pw, t, tf, tn},
    model::{Model, Peer, Shared, State, notice_balloon},
    theme::{self, Theme},
    tray::Artwork,
};
use std::{
    cell::RefCell,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{LibraryLoader::GetModuleHandleW, Registry::*, Threading::CreateMutexW},
        UI::{
            Controls::*,
            HiDpi::*,
            Input::KeyboardAndMouse::{EnableWindow, GetFocus, IsWindowEnabled, SetFocus},
            Shell::*,
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, w},
};

const CLASS: PCWSTR = w!("NearLink.NativeWindow.v1");
const WAKE: u32 = WM_APP + 1;
const TRAY: u32 = WM_APP + 2;
const SHOW: u32 = WM_APP + 3;
const REDRAW: u32 = WM_APP + 4;
const ADD: usize = 100;
const RETRY: usize = 101;
const STOP: usize = 102;
const ALIAS: usize = 103;
const FORGET: usize = 104;
const SETTINGS: usize = 105;
const LIST: usize = 110;
const OPEN_MAIN: usize = 106;
const QUIT: usize = 107;
const ACTIONS: usize = 108;
const CONNECT: usize = 201;
const SEARCH: usize = 202;
const BT_SETTINGS: usize = 203;
const SAVE: usize = 204;
const CANCEL: usize = 205;
const INPUT: usize = 210;
const ALERTS: usize = 211;
const FULL: usize = 212;
const STARTUP: usize = 213;
// STATIC control's documented SS_CENTER style (winuser.h).
const SS_CENTER: u32 = 1;

/// Logical height of a device row: name and status lines beside the battery glyph.
const ROW_HEIGHT: i32 = 68;

/// Whether GDI matched the requested face instead of substituting another font.
unsafe fn font_has_face(font: HFONT, face: &str) -> bool {
    let dc = GetDC(None);
    let old = SelectObject(dc, font.into());
    let mut name = [0u16; 64];
    let length = GetTextFaceW(dc, Some(&mut name));
    SelectObject(dc, old);
    ReleaseDC(None, dc);
    length > 0 && String::from_utf16_lossy(&name[..(length as usize).saturating_sub(1)]) == face
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
unsafe fn send(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    SendMessageW(hwnd, message, Some(wparam), Some(lparam))
}
unsafe fn set_text_if_changed(hwnd: HWND, text: &str) {
    let text = wide(text);
    let mut current = vec![0; GetWindowTextLengthW(hwnd) as usize + 1];
    GetWindowTextW(hwnd, &mut current);
    if current == text {
        return;
    }
    // STATIC can paint synchronously during WM_SETTEXT, while App is borrowed.
    // Defer that paint so the parent's theme callbacks can borrow App normally.
    // Only restore redraw for visible controls: WM_SETREDRAW(TRUE) adds WS_VISIBLE.
    let visible = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_VISIBLE.0 != 0;
    if visible {
        send(hwnd, WM_SETREDRAW, WPARAM(0), LPARAM(0));
    }
    let _ = SetWindowTextW(hwnd, PCWSTR(text.as_ptr()));
    if visible {
        send(hwnd, WM_SETREDRAW, WPARAM(1), LPARAM(0));
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

unsafe fn enable_if_changed(hwnd: HWND, enabled: bool) {
    if IsWindowEnabled(hwnd).as_bool() == enabled {
        return;
    }
    let visible = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_VISIBLE.0 != 0;
    if visible {
        send(hwnd, WM_SETREDRAW, WPARAM(0), LPARAM(0));
    }
    let _ = EnableWindow(hwnd, enabled);
    if visible {
        send(hwnd, WM_SETREDRAW, WPARAM(1), LPARAM(0));
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}
fn copy_wide<const N: usize>(target: &mut [u16; N], value: &str) {
    target.fill(0);
    let mut offset = 0;
    for ch in value.chars() {
        let mut buf = [0u16; 2];
        let units = ch.encode_utf16(&mut buf);
        if offset + units.len() >= N {
            break;
        }
        target[offset..offset + units.len()].copy_from_slice(units);
        offset += units.len();
    }
}

pub fn fatal(message: &str) {
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(wide(message).as_ptr()),
            w!("NearLink"),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// Discovery rows say when a device is already on this PC's list, so it is not added twice.
fn discovered_label(model: &Model, peer: &crate::model::Discovered) -> String {
    match model.peers.get(&peer.address) {
        Some(saved) if saved.saved.enabled => tf("{0} · Already added", &[&peer.name]),
        Some(_) => tf("{0} · Added, receiving stopped", &[&peer.name]),
        None => peer.name.clone(),
    }
}

/// Recoverable input problems stay attached to the dialog the user is working in.
unsafe fn warn(owner: HWND, message: &str) {
    MessageBoxW(
        Some(owner),
        PCWSTR(wide(message).as_ptr()),
        w!("NearLink"),
        MB_OK | MB_ICONWARNING,
    );
}

enum Kind {
    Add,
    Alias(u64),
    Settings,
}
struct Dialog {
    hwnd: HWND,
    kind: Kind,
    list: HWND,
    input: HWND,
}

struct App {
    host: HWND,
    main: HWND,
    list: HWND,
    quick: HWND,
    quick_list: HWND,
    quick_closed: Option<std::time::Instant>,
    info: HWND,
    font: HFONT,
    heading: HFONT,
    name_font: HFONT,
    battery_font: HFONT,
    caption_font: HFONT,
    icon_font: HFONT,
    row_images: HIMAGELIST,
    theme: Theme,
    text_scale: u32,
    preview: bool,
    icon: HICON,
    tray_icon: HICON,
    tray_artwork: Option<Artwork>,
    instance: HINSTANCE,
    taskbar_message: u32,
    model: Shared,
    engine: Engine,
    directory: PathBuf,
    dialog: Option<Dialog>,
    rows: Vec<u64>,
    dpi: u32,
    callback: *const RefCell<App>,
}

pub fn run() -> Result<(), String> {
    unsafe {
        let preview = theme::preview();
        if preview && !std::env::args().any(|a| a == "--data-dir") {
            return Err(t("Preview needs a separate --data-dir folder.").into());
        }
        let directory = config::directory()?;
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        // A separate profile is also a separate instance, useful for development and verification.
        let mut hash = 14695981039346656037u64;
        for byte in directory.as_os_str().to_string_lossy().bytes() {
            hash = (hash ^ byte as u64).wrapping_mul(1099511628211);
        }
        let mutex = CreateMutexW(
            None,
            false,
            PCWSTR(wide(&format!("Local\\NearLink-{hash:x}")).as_ptr()),
        )
        .map_err(|e| e.to_string())?;
        let duplicate = GetLastError() == ERROR_ALREADY_EXISTS;
        if duplicate {
            if let Ok(host) = FindWindowW(
                CLASS,
                PCWSTR(wide(&format!("NearLink host {hash:x}")).as_ptr()),
            ) {
                let _ = PostMessageW(Some(host), SHOW, WPARAM(0), LPARAM(0));
            }
            let _ = CloseHandle(mutex);
            return Ok(());
        }
        let settings = config::load(&directory)?;
        let mut initial = Model::new(settings);
        if preview {
            preview_model(&mut initial);
        }
        let model = Arc::new(Mutex::new(initial));
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES,
        });
        let instance: HINSTANCE = GetModuleHandleW(None).map_err(|e| e.to_string())?.into();
        let icon = battery_icon(instance).map_err(|e| e.to_string())?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
            hIcon: icon,
            hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as *mut _),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_thread().to_string());
        }
        // The hidden host survives closing the management window and owns the tray icon.
        let host = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS,
            PCWSTR(wide(&format!("NearLink host {hash:x}")).as_ptr()),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )
        .map_err(|e| e.to_string())?;
        let host_id = host.0 as isize;
        let wake: Wake = Arc::new(move || {
            let _ = PostMessageW(Some(HWND(host_id as *mut _)), WAKE, WPARAM(0), LPARAM(0));
        });
        let engine = Engine::new(model.clone(), wake, host_id);
        let app = Box::new(RefCell::new(App {
            host,
            main: HWND::default(),
            list: HWND::default(),
            quick: HWND::default(),
            quick_list: HWND::default(),
            quick_closed: None,
            info: HWND::default(),
            font: HFONT::default(),
            heading: HFONT::default(),
            name_font: HFONT::default(),
            battery_font: HFONT::default(),
            caption_font: HFONT::default(),
            icon_font: HFONT::default(),
            row_images: HIMAGELIST::default(),
            theme: Theme::load(),
            text_scale: theme::text_scale(),
            preview,
            icon,
            tray_icon: HICON::default(),
            tray_artwork: None,
            instance,
            taskbar_message: RegisterWindowMessageW(w!("TaskbarCreated")),
            model,
            engine,
            directory,
            dialog: None,
            rows: vec![],
            dpi: 96,
            callback: std::ptr::null(),
        }));
        let pointer = app.as_ref() as *const RefCell<App>;
        app.borrow_mut().callback = pointer;
        SetWindowLongPtrW(host, GWLP_USERDATA, pointer as isize);
        app.borrow_mut().add_tray().map_err(|e| e.to_string())?;
        if !std::env::args().any(|arg| arg == "--hidden") {
            app.borrow_mut().show_main().map_err(|e| e.to_string())?;
        }
        let addresses: Vec<_> = app
            .borrow()
            .model
            .lock()
            .unwrap()
            .peers
            .values()
            .filter(|p| p.saved.enabled)
            .map(|p| p.saved.address)
            .collect();
        for address in addresses.into_iter().filter(|_| !preview) {
            app.borrow_mut().engine.start(address, false);
        }
        if std::env::args().any(|a| a == "--quick-view") {
            app.borrow_mut().show_quick().map_err(|e| e.to_string())?;
        }
        let mut message = MSG::default();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0).0;
            if result == 0 {
                break;
            }
            if result == -1 {
                break;
            }
            let dialog = app.borrow().dialog.as_ref().map(|d| d.hwnd);
            if dialog.is_some_and(|hwnd| IsDialogMessageW(hwnd, &message).as_bool()) {
                continue;
            }
            let quick = app.borrow().quick;
            if !quick.is_invalid() && IsDialogMessageW(quick, &message).as_bool() {
                continue;
            }
            let main = app.borrow().main;
            if !main.is_invalid() && IsDialogMessageW(main, &message).as_bool() {
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let mut state = app.borrow_mut();
        state.engine.shutdown();
        // A final BLE callback may have learned the paired identity just before
        // Quit. Persist it while the instance mutex and host still belong to us.
        if !state.preview {
            let model = state.model.lock().unwrap();
            if model.dirty {
                state.persist(&model.settings)?;
            }
        }
        state.remove_tray();
        for font in [
            state.font,
            state.heading,
            state.name_font,
            state.battery_font,
            state.caption_font,
            state.icon_font,
        ] {
            if !font.is_invalid() {
                let _ = DeleteObject(font.into());
            }
        }
        if !state.row_images.is_invalid() {
            let _ = ImageList_Destroy(Some(state.row_images));
        }
        let _ = DestroyIcon(state.icon);
        let _ = DestroyIcon(state.tray_icon);
        let _ = DestroyWindow(host);
        let _ = CloseHandle(mutex);
        Ok(())
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<App>;
    if pointer.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    // Layout/control creation can paint synchronously while App is borrowed.
    // Repaint after that operation, without holding App across nested messages.
    if message == REDRAW {
        let windows = if let Ok(app) = (*pointer).try_borrow() {
            [
                app.main,
                app.quick,
                app.dialog.as_ref().map(|d| d.hwnd).unwrap_or_default(),
            ]
        } else {
            return LRESULT(0);
        };
        for window in windows {
            if !window.is_invalid() && IsWindowVisible(window).as_bool() {
                let _ = RedrawWindow(
                    Some(window),
                    None,
                    None,
                    RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW,
                );
            }
        }
        if !windows[1].is_invalid() {
            theme::cloak(windows[1], false);
        }
        return LRESULT(0);
    }
    // Win32 may synchronously reenter this procedure (control creation, dialogs, layout).
    // RefCell prevents aliasing mutable application state across those callbacks.
    let Ok(mut state) = (*pointer).try_borrow_mut() else {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    };
    // Ignore synchronous control/paint messages before borrowing application state.
    match message {
        WAKE | SHOW | TRAY | WM_COMMAND | WM_NOTIFY | WM_CLOSE | WM_SIZE | WM_TIMER
        | WM_DPICHANGED | WM_GETMINMAXINFO | WM_ENDSESSION | WM_ERASEBKGND | WM_CTLCOLORSTATIC
        | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_CTLCOLORBTN | WM_SETTINGCHANGE
        | WM_THEMECHANGED | WM_SYSCOLORCHANGE | WM_CONTEXTMENU | WM_ACTIVATE => {}
        other if other == state.taskbar_message => {
            let _ = state.add_tray();
            return LRESULT(0);
        }
        _ => {
            drop(state);
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
    }
    let app = &mut *state;
    match message {
        WM_ERASEBKGND => {
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            FillRect(HDC(wparam.0 as *mut _), &rect, app.theme.brush);
            LRESULT(1)
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            let dc = HDC(wparam.0 as *mut _);
            SetTextColor(dc, app.theme.text);
            let input = message == WM_CTLCOLOREDIT || message == WM_CTLCOLORLISTBOX;
            SetBkColor(
                dc,
                if input {
                    app.theme.surface
                } else {
                    app.theme.background
                },
            );
            LRESULT(if input {
                app.theme.surface_brush.0
            } else {
                app.theme.brush.0
            } as isize)
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED | WM_SYSCOLORCHANGE => {
            app.appearance();
            LRESULT(0)
        }
        WM_CONTEXTMENU if HWND(wparam.0 as *mut _) == app.list => {
            app.device_menu();
            LRESULT(0)
        }
        WM_ACTIVATE if hwnd == app.quick && wparam.0 & 0xffff == WA_INACTIVE as usize => {
            let _ = ShowWindow(app.quick, SW_HIDE);
            let _ = KillTimer(Some(app.quick), 1);
            app.quick_closed = Some(std::time::Instant::now());
            LRESULT(0)
        }
        WAKE => {
            app.refresh();
            app.notices();
            LRESULT(0)
        }
        SHOW => {
            let _ = app.show_main();
            LRESULT(0)
        }
        WM_TIMER if hwnd == app.host => {
            app.update_tray_icon();
            LRESULT(0)
        }
        WM_TIMER => {
            app.refresh();
            LRESULT(0)
        }
        WM_COMMAND => {
            app.command(hwnd, wparam.0 & 0xffff, (wparam.0 >> 16) as u16);
            LRESULT(0)
        }
        WM_NOTIFY => {
            let header = &*(lparam.0 as *const NMHDR);
            if header.code == NM_CUSTOMDRAW {
                if header.hwndFrom == app.list || header.hwndFrom == app.quick_list {
                    return app.draw_list(&mut *(lparam.0 as *mut NMLVCUSTOMDRAW));
                }
                if GetClassNameW(header.hwndFrom, &mut [0u16; 32]) > 0 {
                    return app.draw_button(&mut *(lparam.0 as *mut NMCUSTOMDRAW));
                }
            }
            if header.idFrom == LIST && header.code == LVN_GETEMPTYMARKUP {
                let markup = &mut *(lparam.0 as *mut NMLVEMPTYMARKUP);
                markup.dwFlags = EMF_CENTERED;
                copy_wide(
                    &mut markup.szMarkup,
                    t("No connected devices.\nOn Android NearLink, tap Prepare to connect, then add the device."),
                );
                return LRESULT(1);
            }
            if hwnd == app.main && header.idFrom == LIST && header.code == LVN_ITEMCHANGED {
                app.selection();
            }
            if header.idFrom == LIST
                && (header.code == NM_DBLCLK || header.code == LVN_ITEMACTIVATE)
            {
                if hwnd == app.quick {
                    let _ = app.show_main();
                } else {
                    app.device_menu();
                }
            }
            LRESULT(0)
        }
        WM_SIZE => {
            if hwnd == app.main || hwnd == app.quick {
                app.layout();
                let _ = PostMessageW(Some(app.host), REDRAW, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        WM_GETMINMAXINFO if hwnd == app.main || hwnd == app.quick => {
            let info = &mut *(lparam.0 as *mut MINMAXINFO);
            let work = app.work_area(hwnd);
            info.ptMinTrackSize = POINT {
                x: app
                    .s(if hwnd == app.quick { 390 } else { 620 })
                    .min(work.right - work.left),
                y: app
                    .s(if hwnd == app.quick { 320 } else { 520 })
                    .min(work.bottom - work.top),
            };
            LRESULT(0)
        }
        WM_DPICHANGED => {
            app.dpi = (wparam.0 & 0xffff) as u32;
            app.update_font();
            app.resize_rows();
            let bounds = &*(lparam.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            app.layout();
            app.layout_dialog();
            LRESULT(0)
        }
        WM_CLOSE => {
            if app.dialog.as_ref().is_some_and(|d| d.hwnd == hwnd) {
                app.close_dialog();
            } else if hwnd == app.main || hwnd == app.quick {
                if hwnd == app.main {
                    app.close_dialog();
                }
                let _ = KillTimer(Some(hwnd), 1);
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            LRESULT(0)
        }
        WM_ENDSESSION if wparam.0 != 0 => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        TRAY => {
            let event = lparam.0 as u32 & 0xffff;
            if event == WM_LBUTTONUP || event == NIN_SELECT || event == (NIN_SELECT | 1) {
                // Clicking the icon while the flyout is open first deactivates (closes) it;
                // treat that click as a toggle instead of reopening it.
                let just_closed = app
                    .quick_closed
                    .take()
                    .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(700));
                if !just_closed {
                    let _ = app.show_quick();
                }
            } else if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                app.tray_menu();
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

impl App {
    unsafe fn work_area(&self, hwnd: HWND) -> RECT {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        info.rcWork
    }

    unsafe fn fit_window(&self, hwnd: HWND, width: i32, height: i32) {
        let work = self.work_area(hwnd);
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        let width = self.s(width).min(work.right - work.left);
        let height = self.s(height).min(work.bottom - work.top);
        let x = rect
            .left
            .clamp(work.left, (work.right - width).max(work.left));
        let y = rect
            .top
            .clamp(work.top, (work.bottom - height).max(work.top));
        let _ = SetWindowPos(
            hwnd,
            None,
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    fn persist(&self, settings: &config::Settings) -> Result<(), String> {
        if self.preview {
            Ok(())
        } else {
            config::save(&self.directory, settings)
        }
    }

    unsafe fn appearance(&mut self) {
        self.theme = Theme::load();
        self.update_tray_icon();
        self.text_scale = theme::text_scale();
        self.update_font();
        self.resize_rows();
        for hwnd in [
            self.main,
            self.quick,
            self.dialog.as_ref().map(|d| d.hwnd).unwrap_or_default(),
        ] {
            if hwnd.is_invalid() {
                continue;
            }
            self.theme.window(hwnd);
            let _ = RedrawWindow(
                Some(hwnd),
                None,
                None,
                RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
            );
        }
        // Text-only accessibility scaling needs larger layout, not just larger glyphs.
        if !self.main.is_invalid() {
            let mut rect = RECT::default();
            let _ = GetWindowRect(self.main, &mut rect);
            let _ = SetWindowPos(
                self.main,
                None,
                0,
                0,
                (rect.right - rect.left).max(self.s(620)),
                (rect.bottom - rect.top).max(self.s(520)),
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        self.layout();
        self.layout_dialog();
        let _ = PostMessageW(Some(self.host), REDRAW, WPARAM(0), LPARAM(0));
    }

    unsafe fn device_list(&mut self, parent: HWND) -> HWND {
        let list = self.control(
            parent,
            WC_LISTVIEWW,
            t("Device Battery"),
            WS_TABSTOP
                | WINDOW_STYLE(
                    LVS_REPORT
                        | LVS_SINGLESEL
                        | LVS_SHOWSELALWAYS
                        | LVS_NOCOLUMNHEADER
                        | LVS_SHAREIMAGELISTS,
                ),
            LIST,
        );
        send(
            list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            WPARAM(0),
            LPARAM((LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_LABELTIP) as isize),
        );
        let mut name = wide(t("Device Battery"));
        let column = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH,
            pszText: windows::core::PWSTR(name.as_mut_ptr()),
            cx: self.s(500),
            ..Default::default()
        };
        send(
            list,
            LVM_INSERTCOLUMNW,
            WPARAM(0),
            LPARAM(&column as *const _ as isize),
        );
        if self.row_images.is_invalid() {
            self.row_images = ImageList_Create(1, self.s(ROW_HEIGHT), ILC_COLOR32, 1, 1);
        }
        send(
            list,
            LVM_SETIMAGELIST,
            WPARAM(LVSIL_SMALL as usize),
            LPARAM(self.row_images.0),
        );
        send(
            list,
            LVM_SETBKCOLOR,
            WPARAM(0),
            LPARAM(self.theme.background.0 as isize),
        );
        send(
            list,
            LVM_SETTEXTCOLOR,
            WPARAM(0),
            LPARAM(self.theme.text.0 as isize),
        );
        list
    }

    unsafe fn resize_rows(&mut self) {
        let old = self.row_images;
        self.row_images = ImageList_Create(1, self.s(ROW_HEIGHT), ILC_COLOR32, 1, 1);
        for list in [self.list, self.quick_list] {
            if list.is_invalid() {
                continue;
            }
            send(
                list,
                LVM_SETIMAGELIST,
                WPARAM(LVSIL_SMALL as usize),
                LPARAM(self.row_images.0),
            );
            send(
                list,
                LVM_SETBKCOLOR,
                WPARAM(0),
                LPARAM(self.theme.background.0 as isize),
            );
            send(
                list,
                LVM_SETTEXTCOLOR,
                WPARAM(0),
                LPARAM(self.theme.text.0 as isize),
            );
        }
        if !old.is_invalid() {
            let _ = ImageList_Destroy(Some(old));
        }
    }

    unsafe fn show_quick(&mut self) -> windows::core::Result<()> {
        if self.quick.is_invalid() {
            if self.font.is_invalid() {
                self.update_font();
            }
            self.quick = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_CONTROLPARENT,
                CLASS,
                if self.preview {
                    pw("NearLink · Preview")
                } else {
                    w!("NearLink")
                },
                WS_POPUP | WS_CLIPCHILDREN,
                0,
                0,
                self.s(430),
                self.s(520),
                Some(self.host),
                None,
                Some(self.instance),
                None,
            )?;
            SetWindowLongPtrW(self.quick, GWLP_USERDATA, self.callback as isize);
            self.dpi = GetDpiForWindow(self.quick);
            self.update_font();
            self.resize_rows();
            self.quick_list = self.device_list(self.quick);
            self.control(self.quick,w!("STATIC"),t("No connected devices.\r\nOn Android NearLink, tap Prepare to connect, then add the device."),WINDOW_STYLE(SS_CENTER),305);
            self.button(self.quick, t("&Add device"), ADD);
            self.button(self.quick, t("&Open details"), OPEN_MAIN);
        }
        self.theme.window(self.quick);
        let count = self.model.lock().unwrap().peers.len().clamp(1, 6) as i32;
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let work = info.rcWork;
        let width = self.s(430).min(work.right - work.left);
        let height = self.s(count * ROW_HEIGHT + 80).min(work.bottom - work.top);
        let x = (point.x - width / 2).clamp(work.left, (work.right - width).max(work.left));
        let y = (point.y - height).clamp(work.top, (work.bottom - height).max(work.top));
        let _ = SetWindowPos(
            self.quick,
            Some(HWND_TOPMOST),
            x,
            y,
            width,
            height,
            SWP_NOACTIVATE,
        );
        self.layout();
        self.refresh();
        let _ = PostMessageW(Some(self.host), REDRAW, WPARAM(0), LPARAM(0));
        SetTimer(Some(self.quick), 1, 10_000, None);
        // App is borrowed here, so any paint during ShowWindow takes the default (light) path.
        // Keep the window cloaked until the REDRAW handler repaints it with the app theme.
        theme::cloak(self.quick, true);
        let _ = ShowWindow(self.quick, SW_SHOW);
        let _ = SetForegroundWindow(self.quick);
        let _ = SetFocus(Some(self.quick_list));
        Ok(())
    }

    unsafe fn device_menu(&mut self) {
        if self.selected().is_none() {
            return;
        }
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        for (id, label) in [
            (RETRY, t("&Reconnect")),
            (STOP, t("&Stop receiving")),
            (ALIAS, t("&Nickname")),
            (FORGET, t("Re&move from list")),
        ] {
            let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(wide(label).as_ptr()));
        }
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        // A keyboard context menu belongs next to the focused list row.
        if GetFocus() == self.list {
            let mut rect = RECT::default();
            let _ = GetWindowRect(self.list, &mut rect);
            point = POINT {
                x: rect.left + self.s(32),
                y: rect.top + self.s(32),
            };
        }
        let id = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            Some(0),
            self.main,
            None,
        )
        .0 as usize;
        let _ = DestroyMenu(menu);
        self.command(self.main, id, 0);
    }

    unsafe fn draw_list(&self, draw: &mut NMLVCUSTOMDRAW) -> LRESULT {
        if draw.nmcd.dwDrawStage == CDDS_PREPAINT {
            return LRESULT(CDRF_NOTIFYITEMDRAW as isize);
        }
        if draw.nmcd.dwDrawStage != CDDS_ITEMPREPAINT {
            return LRESULT(CDRF_DODEFAULT as isize);
        }
        let model = self.model.lock().unwrap();
        let Some(peer) = model.peers.values().nth(draw.nmcd.dwItemSpec) else {
            return LRESULT(0);
        };
        let native_state = send(
            draw.nmcd.hdr.hwndFrom,
            LVM_GETITEMSTATE,
            WPARAM(draw.nmcd.dwItemSpec),
            LPARAM((LVIS_SELECTED.0 | LVIS_FOCUSED.0) as isize),
        )
        .0 as u32;
        let selected = native_state & LVIS_SELECTED.0 != 0;
        let dc = draw.nmcd.hdc;
        let mut row = RECT {
            left: LVIR_BOUNDS as i32,
            ..Default::default()
        };
        send(
            draw.nmcd.hdr.hwndFrom,
            LVM_GETITEMRECT,
            WPARAM(draw.nmcd.dwItemSpec),
            LPARAM(&mut row as *mut RECT as isize),
        );
        if row.right <= row.left || row.bottom <= row.top {
            return LRESULT(CDRF_SKIPDEFAULT as isize);
        }
        let saved_dc = SaveDC(dc);
        IntersectClipRect(dc, row.left, row.top, row.right, row.bottom);
        row.left += self.s(2);
        row.right -= self.s(2);
        row.top += self.s(3);
        row.bottom -= self.s(3);
        let background = if selected {
            self.theme.selected
        } else {
            self.theme.surface
        };
        let radius = if self.theme.contrast { 0 } else { self.s(8) };
        theme::round(dc, &row, background, None, radius);
        let foreground = if selected {
            self.theme.on_selected
        } else {
            self.theme.text
        };
        let secondary = if selected && self.theme.contrast {
            foreground
        } else {
            self.theme.secondary
        };
        let accent = if selected && self.theme.contrast {
            foreground
        } else {
            self.theme.accent
        };
        let area = |x, y, w, h| RECT {
            left: row.left + self.s(x),
            top: row.top + self.s(y),
            right: row.left + self.s(x + w),
            bottom: row.top + self.s(y + h),
        };
        let width = (row.right - row.left) * 9600 / (self.dpi * self.text_scale) as i32;
        // Windows Settings device-card order: glyph, name over status, reading on the right.
        let contrast_selected = selected && self.theme.contrast;
        let reading = if contrast_selected {
            foreground
        } else if peer.muted() {
            secondary
        } else {
            accent
        };
        let status = if contrast_selected {
            foreground
        } else if peer.needs_attention() {
            self.theme.warning
        } else {
            secondary
        };
        let right = 104;
        let text_width = (width - 56 - right - 24).max(40);
        theme::text(
            dc,
            self.icon_font,
            reading,
            &peer.battery_glyph().to_string(),
            area(12, 15, 32, 32),
            DT_SINGLELINE | DT_CENTER | DT_VCENTER,
        );
        theme::text(
            dc,
            self.name_font,
            foreground,
            peer.saved.label(),
            area(56, 10, text_width, 22),
            DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        theme::text(
            dc,
            self.font,
            status,
            &peer.row_status(),
            // Without a charging badge the status may run under the reading column.
            area(
                56,
                33,
                if peer.charge_badge().is_empty() {
                    (width - 56 - 16).max(40)
                } else {
                    text_width
                },
                21,
            ),
            DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        let known = peer.level.is_some();
        theme::text(
            dc,
            if known { self.battery_font } else { self.font },
            reading,
            &peer.battery_text(),
            area(width - right - 16, if known { 5 } else { 10 }, right, 28),
            DT_SINGLELINE | DT_RIGHT | if known { DT_TOP } else { DT_VCENTER },
        );
        theme::text(
            dc,
            self.caption_font,
            if contrast_selected { foreground } else { secondary },
            peer.charge_badge(),
            area(width - right - 16, 35, right, 18),
            DT_SINGLELINE | DT_RIGHT | DT_END_ELLIPSIS,
        );
        if selected || self.theme.contrast {
            let edge = if selected { accent } else { self.theme.border };
            let hollow = CreatePen(PS_SOLID, 1, edge);
            let old = SelectObject(dc, hollow.into());
            let brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
            if radius > 0 {
                let _ = RoundRect(dc, row.left, row.top, row.right, row.bottom, radius * 2, radius * 2);
            } else {
                let _ = Rectangle(dc, row.left, row.top, row.right, row.bottom);
            }
            SelectObject(dc, brush);
            SelectObject(dc, old);
            let _ = DeleteObject(hollow.into());
        }
        if native_state & LVIS_FOCUSED.0 != 0 && GetFocus() == draw.nmcd.hdr.hwndFrom {
            let mut focus = row;
            let _ = InflateRect(&mut focus, -self.s(2), -self.s(2));
            let _ = DrawFocusRect(dc, &focus);
        }
        let _ = RestoreDC(dc, saved_dc);
        LRESULT(CDRF_SKIPDEFAULT as isize)
    }

    unsafe fn draw_button(&self, draw: &mut NMCUSTOMDRAW) -> LRESULT {
        if draw.dwDrawStage != CDDS_PREPAINT {
            return LRESULT(CDRF_DODEFAULT as isize);
        }
        let hwnd = draw.hdr.hwndFrom;
        let id = GetDlgCtrlID(hwnd) as usize;
        let primary = id == ADD || id == CONNECT || id == SAVE;
        let disabled = !IsWindowEnabled(hwnd).as_bool();
        let hot = (draw.uItemState & (CDIS_HOT | CDIS_SELECTED)).0 != 0;
        let dc = draw.hdc;
        let background = if primary && !disabled {
            self.theme.accent
        } else if hot {
            self.theme.selected
        } else {
            self.theme.surface
        };
        let color = if disabled {
            self.theme.secondary
        } else if primary {
            self.theme.on_accent
        } else if hot && self.theme.contrast {
            self.theme.on_selected
        } else {
            self.theme.text
        };
        let checkbox = [ALERTS, FULL, STARTUP].contains(&id);
        let mut rect = draw.rc;
        if checkbox {
            theme::fill(dc, &rect, self.theme.background);
            let box_rect = RECT {
                right: rect.left + self.s(18),
                bottom: rect.top + self.s(18),
                ..rect
            };
            let radius = if self.theme.contrast { 0 } else { self.s(4) };
            theme::round(
                dc,
                &box_rect,
                if checked(hwnd) {
                    self.theme.accent
                } else {
                    self.theme.surface
                },
                Some(if checked(hwnd) {
                    self.theme.accent
                } else {
                    self.theme.secondary
                }),
                radius,
            );
            if checked(hwnd) {
                // Draw a check with the same GDI stroke in every theme.
                let pen = CreatePen(PS_SOLID, self.s(2), self.theme.on_accent);
                let old = SelectObject(dc, pen.into());
                let _ = MoveToEx(dc, rect.left + self.s(4), rect.top + self.s(9), None);
                let _ = LineTo(dc, rect.left + self.s(8), rect.top + self.s(13));
                let _ = LineTo(dc, rect.left + self.s(15), rect.top + self.s(5));
                SelectObject(dc, old);
                let _ = DeleteObject(pen.into());
            }
            rect.left += self.s(26);
        } else {
            theme::fill(dc, &rect, self.theme.background);
            let radius = if self.theme.contrast { 0 } else { self.s(4) };
            let border = (!primary || disabled).then_some(self.theme.border);
            theme::round(dc, &rect, background, border, radius);
        }
        // Let DrawText render the native mnemonic underline (including Alt navigation).
        let mut label = wide(&control_text(hwnd));
        label.pop();
        let old = SelectObject(dc, self.font.into());
        SetTextColor(dc, color);
        SetBkMode(dc, TRANSPARENT);
        DrawTextW(
            dc,
            &mut label,
            &mut rect,
            DT_SINGLELINE | DT_VCENTER | if checkbox { DT_LEFT } else { DT_CENTER },
        );
        SelectObject(dc, old);
        if (draw.uItemState & CDIS_FOCUS).0 != 0 {
            let mut focus = draw.rc;
            let _ = InflateRect(&mut focus, -self.s(3), -self.s(3));
            SetTextColor(dc, color);
            let _ = DrawFocusRect(dc, &focus);
        }
        LRESULT(CDRF_SKIPDEFAULT as isize)
    }

    unsafe fn layout_dialog(&self) {
        let Some(dialog) = &self.dialog else {
            return;
        };
        let item = |id| GetDlgItem(Some(dialog.hwnd), id).unwrap_or_default();
        let height = match dialog.kind {
            Kind::Add => 450,
            Kind::Settings => 300,
            Kind::Alias(_) => 270,
        };
        self.fit_window(dialog.hwnd, 600, height);
        match dialog.kind {
            Kind::Add => {
                self.place(item(300), 20, 18, 545, 52);
                self.place(dialog.list, 20, 82, 545, 190);
                self.place(item(SEARCH as i32), 20, 288, 122, 34);
                self.place(item(BT_SETTINGS as i32), 150, 288, 162, 34);
                self.place(item(CONNECT as i32), 320, 288, 245, 34);
                self.place(item(301), 20, 335, 545, 65);
            }
            Kind::Alias(_) => {
                self.place(item(300), 20, 20, 545, 50);
                self.place(dialog.input, 20, 82, 545, 32);
                self.place(item(SAVE as i32), 350, 172, 100, 34);
                self.place(item(CANCEL as i32), 465, 172, 100, 34);
            }
            Kind::Settings => {
                self.place(item(300), 20, 16, 545, 24);
                self.place(item(ALERTS as i32), 20, 54, 205, 28);
                self.place(dialog.input, 230, 54, 50, 28);
                self.place(item(301), 288, 58, 277, 26);
                self.place(item(FULL as i32), 20, 94, 400, 28);
                self.place(item(STARTUP as i32), 20, 134, 510, 28);
                self.place(item(SAVE as i32), 350, 202, 100, 34);
                self.place(item(CANCEL as i32), 465, 202, 100, 34);
            }
        }
    }

    fn s(&self, value: i32) -> i32 {
        (value * self.dpi as i32 * self.text_scale as i32 + 4800) / 9600
    }

    unsafe fn control(
        &self,
        parent: HWND,
        class: PCWSTR,
        text: &str,
        style: WINDOW_STYLE,
        id: usize,
    ) -> HWND {
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            PCWSTR(wide(text).as_ptr()),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            0,
            0,
            Some(parent),
            Some(HMENU(id as *mut _)),
            Some(self.instance),
            None,
        )
        .unwrap_or_default();
        send(hwnd, WM_SETFONT, WPARAM(self.font.0 as usize), LPARAM(1));
        hwnd
    }
    unsafe fn button(&self, parent: HWND, text: &str, id: usize) -> HWND {
        self.control(
            parent,
            w!("BUTTON"),
            text,
            WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
            id,
        )
    }
    unsafe fn place(&self, hwnd: HWND, x: i32, y: i32, width: i32, height: i32) {
        let _ = MoveWindow(
            hwnd,
            self.s(x),
            self.s(y),
            self.s(width),
            self.s(height),
            true,
        );
    }
    unsafe fn update_font(&mut self) {
        let old = [
            self.font,
            self.heading,
            self.name_font,
            self.battery_font,
            self.caption_font,
            self.icon_font,
        ];
        let factor = (self.dpi * self.text_scale) as i32;
        let make_face = |size: i32, weight, face: PCWSTR| {
            CreateFontW(
                -(size * factor + 4800) / 9600,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                DEFAULT_PITCH.0 as u32,
                face,
            )
        };
        let make = |size: i32, weight| make_face(size, weight, w!("Segoe UI"));
        self.font = make(14, FW_NORMAL.0 as i32);
        self.heading = make(24, FW_SEMIBOLD.0 as i32);
        self.name_font = make(14, FW_SEMIBOLD.0 as i32);
        self.battery_font = make(20, FW_SEMIBOLD.0 as i32);
        self.caption_font = make(12, FW_NORMAL.0 as i32);
        // Windows 11 ships Segoe Fluent Icons; MDL2 Assets has the same battery code points.
        self.icon_font = make_face(24, FW_NORMAL.0 as i32, w!("Segoe Fluent Icons"));
        if !font_has_face(self.icon_font, "Segoe Fluent Icons") {
            let _ = DeleteObject(self.icon_font.into());
            self.icon_font = make_face(24, FW_NORMAL.0 as i32, w!("Segoe MDL2 Assets"));
        }
        for parent in [
            self.main,
            self.quick,
            self.dialog.as_ref().map(|d| d.hwnd).unwrap_or_default(),
        ] {
            if parent.is_invalid() {
                continue;
            }
            let mut child = GetWindow(parent, GW_CHILD).unwrap_or_default();
            while !child.is_invalid() {
                send(child, WM_SETFONT, WPARAM(self.font.0 as usize), LPARAM(1));
                child = GetWindow(child, GW_HWNDNEXT).unwrap_or_default();
            }
            if let Ok(title) = GetDlgItem(Some(parent), 300)
                && parent == self.main
            {
                send(
                    title,
                    WM_SETFONT,
                    WPARAM(self.heading.0 as usize),
                    LPARAM(1),
                );
            }
        }
        for font in old {
            if !font.is_invalid() {
                let _ = DeleteObject(font.into());
            }
        }
    }

    unsafe fn show_main(&mut self) -> windows::core::Result<()> {
        if !self.quick.is_invalid() {
            let _ = ShowWindow(self.quick, SW_HIDE);
            let _ = KillTimer(Some(self.quick), 1);
        }
        if !self.main.is_invalid() {
            let _ = ShowWindow(self.main, SW_RESTORE);
            let _ = SetForegroundWindow(self.main);
            SetTimer(Some(self.main), 1, 10_000, None);
            self.refresh();
            return Ok(());
        }
        self.main = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            CLASS,
            if self.preview {
                pw("NearLink · Preview")
            } else {
                w!("NearLink")
            },
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            960,
            570,
            None,
            None,
            Some(self.instance),
            None,
        )?;
        SetWindowLongPtrW(self.main, GWLP_USERDATA, self.callback as isize);
        self.dpi = GetDpiForWindow(self.main);
        self.update_font();
        self.control(
            self.main,
            w!("STATIC"),
            t("Device Battery"),
            WINDOW_STYLE::default(),
            300,
        );
        self.control(
            self.main,
            w!("STATIC"),
            t("Check the battery of nearby phones and tablets."),
            WINDOW_STYLE::default(),
            301,
        );
        for (text, id) in [
            (t("&Add device"), ADD),
            (t("&Reconnect"), RETRY),
            (t("&Stop receiving"), STOP),
            (t("&Nickname"), ALIAS),
            (t("Re&move from list"), FORGET),
            (t("S&ettings"), SETTINGS),
            (t("Device a&ctions"), ACTIONS),
        ] {
            self.button(self.main, text, id);
        }
        self.list = self.device_list(self.main);
        self.control(self.main,w!("STATIC"),t("No connected devices.\r\nOn Android NearLink, tap Prepare to connect, then add the device."),WINDOW_STYLE(SS_CENTER),305);
        self.theme.window(self.main);
        send(
            GetDlgItem(Some(self.main), 300)?,
            WM_SETFONT,
            WPARAM(self.heading.0 as usize),
            LPARAM(1),
        );
        self.info = self.control(self.main, w!("STATIC"), "", WINDOW_STYLE::default(), 303);
        self.control(
            self.main,
            w!("STATIC"),
            if self.preview {
                t("Preview · Sample devices, no BLE reception")
            } else {
                t("NearLink keeps receiving from the tray when you close this window.")
            },
            WINDOW_STYLE::default(),
            304,
        );
        self.fit_window(self.main, 720, 760);
        self.layout();
        self.refresh();
        let _ = PostMessageW(Some(self.host), REDRAW, WPARAM(0), LPARAM(0));
        SetTimer(Some(self.main), 1, 10_000, None);
        let _ = ShowWindow(self.main, SW_SHOW);
        let _ = SetForegroundWindow(self.main);
        let _ = SetFocus(Some(GetDlgItem(Some(self.main), ADD as i32)?));
        Ok(())
    }

    unsafe fn layout(&self) {
        for (hwnd, list, quick) in [
            (self.main, self.list, false),
            (self.quick, self.quick_list, true),
        ] {
            if hwnd.is_invalid() {
                continue;
            }
            let mut bounds = RECT::default();
            let _ = GetClientRect(hwnd, &mut bounds);
            let width = bounds.right * 9600 / (self.dpi * self.text_scale) as i32;
            let height = bounds.bottom * 9600 / (self.dpi * self.text_scale) as i32;
            let item = |id| GetDlgItem(Some(hwnd), id).unwrap_or_default();
            if quick {
                self.place(list, 8, 8, width - 16, (height - 64).max(80));
                self.place(
                    item(305),
                    24,
                    8 + (height - 64).max(80) / 2 - 36,
                    width - 48,
                    72,
                );
                self.place(item(ADD as i32), 12, height - 48, 110, 36);
                self.place(item(OPEN_MAIN as i32), width - 122, height - 48, 110, 36);
            } else {
                let compact = height < 520;
                let menu_actions = compact && height < 400;
                for id in [301, 303, 304] {
                    let _ = ShowWindow(item(id), if compact { SW_HIDE } else { SW_SHOW });
                }
                let _ = ShowWindow(
                    item(ACTIONS as i32),
                    if menu_actions { SW_SHOW } else { SW_HIDE },
                );
                for id in [RETRY, STOP, ALIAS, FORGET] {
                    let _ = ShowWindow(
                        item(id as i32),
                        if menu_actions { SW_HIDE } else { SW_SHOW },
                    );
                }
                if compact {
                    self.place(item(300), 24, 12, width - 48, 34);
                    self.place(item(ADD as i32), 24, 58, 120, 34);
                    self.place(item(SETTINGS as i32), width - 112, 58, 88, 34);
                    let extra = if !menu_actions && width < 610 { 42 } else { 0 };
                    let list_height = (height - 166 - extra).max(64);
                    self.place(list, 16, 106, width - 32, list_height);
                    self.place(item(305), 24, 106 + list_height / 2 - 36, width - 48, 72);
                    let mut x = 24;
                    let mut y = height - 44 - extra;
                    for (id, size) in [(RETRY, 110), (STOP, 110), (ALIAS, 80), (FORGET, 142)] {
                        if x + size > width - 16 {
                            x = 24;
                            y += 42;
                        }
                        self.place(item(id as i32), x, y, size, 34);
                        x += size + 8;
                    }
                    self.place(item(ACTIONS as i32), 24, height - 44, 142, 34);
                    send(
                        list,
                        LVM_SETCOLUMNWIDTH,
                        WPARAM(0),
                        LPARAM(
                            (self.s(width - 32) - GetSystemMetrics(SM_CXVSCROLL) - 4).max(1)
                                as isize,
                        ),
                    );
                    continue;
                }
                self.place(item(300), 24, 20, width - 48, 34);
                self.place(item(301), 24, 62, width - 48, 24);
                self.place(item(ADD as i32), 24, 100, 120, 34);
                self.place(item(SETTINGS as i32), width - 112, 100, 88, 34);
                let wrap = width < 610;
                let extra = if wrap { 42 } else { 0 };
                self.place(list, 16, 150, width - 32, (height - 316 - extra).max(64));
                self.place(
                    item(305),
                    24,
                    150 + (height - 316 - extra).max(64) / 2 - 36,
                    width - 48,
                    72,
                );
                self.place(self.info, 24, height - 156 - extra, width - 48, 52);
                let mut x = 24;
                let mut y = height - 94 - extra;
                for (id, size) in [(RETRY, 110), (STOP, 110), (ALIAS, 80), (FORGET, 142)] {
                    if x + size > width - 16 {
                        x = 24;
                        y += 42;
                    }
                    self.place(item(id as i32), x, y, size, 34);
                    x += size + 8;
                }
                self.place(item(304), 24, height - 44, width - 48, 24);
            }
            send(
                list,
                LVM_SETCOLUMNWIDTH,
                WPARAM(0),
                LPARAM(
                    (self.s(width - if quick { 24 } else { 32 })
                        - GetSystemMetrics(SM_CXVSCROLL)
                        - 4)
                    .max(1) as isize,
                ),
            );
        }
    }

    unsafe fn selected(&self) -> Option<u64> {
        if self.main.is_invalid() {
            return None;
        }
        let row = send(
            self.list,
            LVM_GETNEXTITEM,
            WPARAM(usize::MAX),
            LPARAM(LVNI_SELECTED as isize),
        )
        .0;
        if row < 0 {
            None
        } else {
            self.rows.get(row as usize).copied()
        }
    }
    unsafe fn selection(&self) {
        let model = self.model.lock().unwrap();
        let peer = self
            .selected()
            .and_then(|address| model.peers.get(&address));
        for id in [RETRY, STOP, ALIAS, FORGET, ACTIONS] {
            let enabled = peer.is_some_and(|p| id != STOP || p.state != State::Stopped);
            enable_if_changed(
                GetDlgItem(Some(self.main), id as i32).unwrap_or_default(),
                enabled,
            );
        }
        let detail = peer.map(|peer| {
            if peer.stale() { t("This value hasn't been updated for over 15 minutes. If the battery doesn't change, the received time can get old.").into() }
            else if peer.confirmed_unchanged() { t("The battery hasn't changed, so the received time is old, but a recent connection check succeeded and this is the current value.").into() }
            else if !peer.detail.is_empty() { peer.detail.clone() }
            else if peer.state != State::Connected && peer.updated.is_some() { t("This is the last level received and may differ from the current value.").into() }
            else { t("The value updates automatically when the battery changes. The last received time is based on battery data reception.").into() }
        }).unwrap_or_else(|| if model.message.is_empty() { t("Select a device to see its connection status.").into() } else { model.message.clone() });
        set_text_if_changed(self.info, &detail);
    }

    unsafe fn refresh(&mut self) {
        let learned = {
            let mut model = self.model.lock().unwrap();
            if model.dirty {
                model.dirty = false;
                Some(model.settings.clone())
            } else {
                None
            }
        };
        if let Some(settings) = learned
            && let Err(error) = self.persist(&settings)
        {
            self.model.lock().unwrap().message = error;
        }
        self.update_tray_icon();
        for (parent, list) in [(self.main, self.list), (self.quick, self.quick_list)] {
            if parent.is_invalid() {
                continue;
            }
            let selected = send(
                list,
                LVM_GETNEXTITEM,
                WPARAM(usize::MAX),
                LPARAM(LVNI_SELECTED as isize),
            )
            .0;
            let address = if selected >= 0 {
                self.rows.get(selected as usize).copied()
            } else {
                None
            };
            let top = send(list, LVM_GETTOPINDEX, WPARAM(0), LPARAM(0)).0;
            SetWindowLongPtrW(parent, GWLP_USERDATA, 0);
            let model = self.model.lock().unwrap();
            let next: Vec<_> = model.peers.keys().copied().collect();
            let rebuild = send(list, LVM_GETITEMCOUNT, WPARAM(0), LPARAM(0)).0 as usize
                != next.len()
                || self.rows != next;
            if rebuild {
                send(list, WM_SETREDRAW, WPARAM(0), LPARAM(0));
                send(list, LVM_DELETEALLITEMS, WPARAM(0), LPARAM(0));
            }
            let count = send(list, LVM_GETITEMCOUNT, WPARAM(0), LPARAM(0)).0 as usize;
            for (row, peer) in model.peers.values().enumerate() {
                let mut label = wide(&peer.accessible_text());
                if !rebuild {
                    let mut current = vec![0; label.len() + 1];
                    let mut item = LVITEMW {
                        pszText: windows::core::PWSTR(current.as_mut_ptr()),
                        cchTextMax: current.len() as i32,
                        ..Default::default()
                    };
                    let length = send(
                        list,
                        LVM_GETITEMTEXTW,
                        WPARAM(row),
                        LPARAM(&mut item as *mut _ as isize),
                    )
                    .0 as usize;
                    if current[..length] == label[..label.len() - 1] {
                        continue;
                    }
                }
                let item = LVITEMW {
                    mask: LVIF_TEXT,
                    iItem: row as i32,
                    pszText: windows::core::PWSTR(label.as_mut_ptr()),
                    ..Default::default()
                };
                send(
                    list,
                    if row >= count {
                        LVM_INSERTITEMW
                    } else {
                        LVM_SETITEMTEXTW
                    },
                    WPARAM(if row >= count { 0 } else { row }),
                    LPARAM(&item as *const _ as isize),
                );
                if rebuild && address == Some(peer.saved.address) {
                    let item = LVITEMW {
                        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
                        state: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
                        ..Default::default()
                    };
                    send(
                        list,
                        LVM_SETITEMSTATE,
                        WPARAM(row),
                        LPARAM(&item as *const _ as isize),
                    );
                }
            }
            // Keep the same top row while refreshing the accessible strings.
            if rebuild && top >= 0 && (top as usize) < next.len() {
                send(list, LVM_ENSUREVISIBLE, WPARAM(top as usize), LPARAM(0));
            }
            if rebuild {
                send(list, WM_SETREDRAW, WPARAM(1), LPARAM(0));
                let _ = InvalidateRect(Some(list), None, false);
            }
            // Apply visibility after WM_SETREDRAW(TRUE), which adds WS_VISIBLE.
            let _ = ShowWindow(list, if next.is_empty() { SW_HIDE } else { SW_SHOWNA });
            if let Ok(empty) = GetDlgItem(Some(parent), 305) {
                let _ = ShowWindow(empty, if next.is_empty() { SW_SHOWNA } else { SW_HIDE });
            }
            drop(model);
            SetWindowLongPtrW(parent, GWLP_USERDATA, self.callback as isize);
        }
        self.rows = self.model.lock().unwrap().peers.keys().copied().collect();
        if !self.main.is_invalid() {
            self.selection();
        }
        self.refresh_dialog();
    }

    unsafe fn command(&mut self, hwnd: HWND, id: usize, notification: u16) {
        if hwnd == self.quick && id == 2 {
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = KillTimer(Some(hwnd), 1);
            return;
        }
        if self
            .dialog
            .as_ref()
            .is_some_and(|dialog| dialog.hwnd == hwnd)
        {
            match id {
                CANCEL | 2 => self.close_dialog(),
                SEARCH => {
                    if !self.preview {
                        self.engine.scan();
                    }
                }
                BT_SETTINGS => {
                    ShellExecuteW(
                        Some(hwnd),
                        w!("open"),
                        w!("ms-settings:bluetooth"),
                        None,
                        None,
                        SW_SHOWNORMAL,
                    );
                }
                CONNECT | 1 if matches!(self.dialog.as_ref().map(|d| &d.kind), Some(Kind::Add)) => {
                    self.connect_selected()
                }
                SAVE | 1 => self.save_dialog(),
                LIST if notification as u32 == LBN_SELCHANGE => self.refresh_dialog(),
                _ => {}
            }
            return;
        }
        match id {
            ADD => {
                let _ = self.show_main();
                self.open_dialog(Kind::Add);
            }
            SETTINGS => {
                let _ = self.show_main();
                self.open_dialog(Kind::Settings);
            }
            OPEN_MAIN => {
                let _ = self.show_main();
                self.open_dialog(Kind::Settings);
            }
            QUIT => PostQuitMessage(0),
            ACTIONS => self.device_menu(),
            RETRY => self.set_enabled(true),
            STOP => self.set_enabled(false),
            ALIAS => {
                if let Some(address) = self.selected() {
                    self.open_dialog(Kind::Alias(address));
                }
            }
            FORGET => self.forget(),
            _ => {}
        }
    }

    unsafe fn close_dialog(&mut self) {
        if let Some(dialog) = self.dialog.take() {
            self.engine.cancel_scan();
            self.model.lock().unwrap().scanning = false;
            let _ = EnableWindow(self.main, true);
            let _ = DestroyWindow(dialog.hwnd);
            let _ = SetForegroundWindow(self.main);
        }
    }

    unsafe fn open_dialog(&mut self, kind: Kind) {
        if let Some(dialog) = &self.dialog {
            let _ = SetForegroundWindow(dialog.hwnd);
            return;
        }
        if self.font.is_invalid() {
            self.update_font();
        }
        let title = match kind {
            Kind::Add => t("Add device"),
            Kind::Alias(_) => t("Device nickname"),
            Kind::Settings => t("Settings"),
        };
        let height = if matches!(kind, Kind::Add) { 450 } else { 300 };
        let hwnd = match CreateWindowExW(
            WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT,
            CLASS,
            PCWSTR(wide(title).as_ptr()),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            self.s(600),
            self.s(height),
            Some(self.main),
            None,
            Some(self.instance),
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(error) => {
                fatal(&error.to_string());
                return;
            }
        };
        let info = self.control(hwnd, w!("STATIC"), "", WINDOW_STYLE::default(), 300);
        let mut list = HWND::default();
        let mut input = HWND::default();
        match kind {
            Kind::Add => {
                self.place(info, 20, 18, 545, 52);
                let _ = SetWindowTextW(
                    info,
                    pw("On Android NearLink, tap Prepare to connect.\r\nSelect the discovered device and connect to start pairing."),
                );
                list = self.control(
                    hwnd,
                    w!("LISTBOX"),
                    "",
                    WS_BORDER
                        | WS_VSCROLL
                        | WS_TABSTOP
                        | WINDOW_STYLE(LBS_NOTIFY as u32 | LBS_NOINTEGRALHEIGHT as u32),
                    LIST,
                );
                self.place(list, 20, 82, 545, 190);
                self.place(self.button(hwnd, t("&Rescan"), SEARCH), 20, 288, 122, 34);
                self.place(
                    self.button(hwnd, t("&Bluetooth settings"), BT_SETTINGS),
                    150,
                    288,
                    162,
                    34,
                );
                self.place(
                    self.button(hwnd, t("&Connect selected device"), CONNECT),
                    320,
                    288,
                    245,
                    34,
                );
                let status =
                    self.control(hwnd, w!("STATIC"), t("Searching…"), WINDOW_STYLE::default(), 301);
                self.place(status, 20, 335, 545, 65);
            }
            Kind::Alias(address) => {
                self.place(info, 20, 20, 545, 50);
                let _ = SetWindowTextW(
                    info,
                    pw("&Nickname\r\nLeave it empty to use the Bluetooth device name."),
                );
                input = self.control(
                    hwnd,
                    w!("EDIT"),
                    "",
                    WS_BORDER | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                    INPUT,
                );
                send(input, EM_SETLIMITTEXT, WPARAM(48), LPARAM(0));
                if let Some(peer) = self.model.lock().unwrap().peers.get(&address) {
                    let _ = SetWindowTextW(input, PCWSTR(wide(&peer.saved.alias).as_ptr()));
                }
                self.place(input, 20, 82, 545, 32);
                self.place(self.button(hwnd, t("&Save"), SAVE), 350, 172, 100, 34);
                self.place(self.button(hwnd, t("Cancel"), CANCEL), 465, 172, 100, 34);
            }
            Kind::Settings => {
                self.place(info, 20, 16, 545, 24);
                let _ = SetWindowTextW(info, pw("Notifications and startup"));
                let settings = &self.model.lock().unwrap().settings;
                let alerts = self.control(
                    hwnd,
                    w!("BUTTON"),
                    t("&Low battery alert"),
                    WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                    ALERTS,
                );
                self.place(alerts, 20, 54, 205, 28);
                set_checked(alerts, settings.notifications);
                input = self.control(
                    hwnd,
                    w!("EDIT"),
                    &settings.low_threshold.to_string(),
                    WS_BORDER | WS_TABSTOP | WINDOW_STYLE(ES_NUMBER as u32),
                    INPUT,
                );
                self.place(input, 230, 54, 50, 28);
                let unit = self.control(
                    hwnd,
                    w!("STATIC"),
                    t("% or below · Not while charging"),
                    WINDOW_STYLE::default(),
                    301,
                );
                self.place(unit, 288, 58, 277, 26);
                let full = self.control(
                    hwnd,
                    w!("BUTTON"),
                    t("Notify when &fully charged (100%)"),
                    WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                    FULL,
                );
                self.place(full, 20, 94, 400, 28);
                set_checked(full, settings.full_notifications);
                let startup = self.control(
                    hwnd,
                    w!("BUTTON"),
                    t("Start in the tray when I sign in to &Windows"),
                    WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                    STARTUP,
                );
                self.place(startup, 20, 134, 510, 28);
                set_checked(startup, startup_enabled());
                self.place(self.button(hwnd, t("&Save"), SAVE), 350, 202, 100, 34);
                self.place(self.button(hwnd, t("Cancel"), CANCEL), 465, 202, 100, 34);
            }
        }
        let scan = matches!(kind, Kind::Add);
        self.dialog = Some(Dialog {
            hwnd,
            kind,
            list,
            input,
        });
        self.layout_dialog();
        self.theme.window(hwnd);
        let _ = EnableWindow(self.main, false);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, self.callback as isize);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        if !input.is_invalid() {
            let _ = SetFocus(Some(input));
        } else if !list.is_invalid() {
            let _ = SetFocus(Some(list));
        }
        if scan && !self.preview {
            self.engine.scan();
        }
        self.refresh_dialog();
        let _ = PostMessageW(Some(self.host), REDRAW, WPARAM(0), LPARAM(0));
    }

    unsafe fn refresh_dialog(&self) {
        let Some(dialog) = &self.dialog else {
            return;
        };
        if !matches!(dialog.kind, Kind::Add) {
            return;
        }
        let model = self.model.lock().unwrap();
        let selected = send(dialog.list, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
        let count = send(dialog.list, LB_GETCOUNT, WPARAM(0), LPARAM(0)).0;
        let changed = count != model.discovered.len() as isize
            || model.discovered.iter().enumerate().any(|(row, peer)| {
                let length = send(dialog.list, LB_GETTEXTLEN, WPARAM(row), LPARAM(0)).0;
                if length < 0 {
                    return true;
                }
                let mut text = vec![0; length as usize + 1];
                send(
                    dialog.list,
                    LB_GETTEXT,
                    WPARAM(row),
                    LPARAM(text.as_mut_ptr() as isize),
                );
                text != wide(&discovered_label(&model, peer))
            });
        if changed {
            send(dialog.list, WM_SETREDRAW, WPARAM(0), LPARAM(0));
            send(dialog.list, LB_RESETCONTENT, WPARAM(0), LPARAM(0));
            for peer in &model.discovered {
                send(
                    dialog.list,
                    LB_ADDSTRING,
                    WPARAM(0),
                    LPARAM(wide(&discovered_label(&model, peer)).as_ptr() as isize),
                );
            }
            if selected >= 0 {
                send(
                    dialog.list,
                    LB_SETCURSEL,
                    WPARAM(selected as usize),
                    LPARAM(0),
                );
            }
            send(dialog.list, WM_SETREDRAW, WPARAM(1), LPARAM(0));
            let _ = InvalidateRect(Some(dialog.list), None, true);
        }
        enable_if_changed(
            GetDlgItem(Some(dialog.hwnd), SEARCH as i32).unwrap_or_default(),
            !model.scanning,
        );
        enable_if_changed(
            GetDlgItem(Some(dialog.hwnd), CONNECT as i32).unwrap_or_default(),
            selected >= 0 && (selected as usize) < model.discovered.len(),
        );
        let status = if model.scanning {
            tf(
                "Looking for devices… {0} found · The search ends in 15 seconds.",
                &[&model.discovered.len()],
            )
        } else if !model.message.is_empty() {
            model.message.clone()
        } else {
            tn(
                "Found {0} device. Select a device to connect.",
                "Found {0} devices. Select a device to connect.",
                model.discovered.len(),
            )
        };
        set_text_if_changed(
            GetDlgItem(Some(dialog.hwnd), 301).unwrap_or_default(),
            &status,
        );
    }

    unsafe fn connect_selected(&mut self) {
        let Some(dialog) = &self.dialog else {
            return;
        };
        let index = send(dialog.list, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
        if index < 0 {
            return;
        }
        let candidate = self
            .model
            .lock()
            .unwrap()
            .discovered
            .get(index as usize)
            .cloned();
        let Some(candidate) = candidate else {
            return;
        };
        let mut settings = self.model.lock().unwrap().settings.clone();
        if !settings
            .peers
            .iter()
            .any(|peer| peer.address == candidate.address)
        {
            if settings.peers.len() >= 16 {
                warn(
                    dialog.hwnd,
                    t("You can add up to 16 devices. Remove an unused device first."),
                );
                return;
            }
            settings.peers.push(SavedPeer::new(
                candidate.address,
                candidate.name.clone(),
                candidate.random_address,
            ));
        }
        if let Some(peer) = settings
            .peers
            .iter_mut()
            .find(|p| p.address == candidate.address)
        {
            peer.enabled = true;
        }
        if let Err(error) = self.persist(&settings) {
            fatal(&error);
            return;
        }
        {
            let mut model = self.model.lock().unwrap();
            model.settings = settings;
            let saved = model
                .settings
                .peers
                .iter()
                .find(|p| p.address == candidate.address)
                .unwrap()
                .clone();
            model
                .peers
                .entry(candidate.address)
                .and_modify(|p| p.saved = saved.clone())
                .or_insert_with(|| Peer::new(saved));
        }
        self.close_dialog();
        self.engine.start(candidate.address, true);
        self.refresh();
    }

    unsafe fn save_dialog(&mut self) {
        let Some(dialog) = &self.dialog else {
            return;
        };
        let mut settings = self.model.lock().unwrap().settings.clone();
        match dialog.kind {
            Kind::Alias(address) => {
                let alias = control_text(dialog.input)
                    .trim()
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(48)
                    .collect::<String>();
                if let Some(peer) = settings
                    .peers
                    .iter_mut()
                    .find(|peer| peer.address == address)
                {
                    peer.alias = alias;
                }
            }
            Kind::Settings => {
                let threshold = control_text(dialog.input).trim().parse::<u8>();
                let Ok(threshold @ 1..=90) = threshold else {
                    warn(dialog.hwnd, t("Enter a number from 1 to 90 for the low battery threshold."));
                    let _ = SetFocus(Some(dialog.input));
                    return;
                };
                settings.notifications =
                    checked(GetDlgItem(Some(dialog.hwnd), ALERTS as i32).unwrap_or_default());
                settings.full_notifications =
                    checked(GetDlgItem(Some(dialog.hwnd), FULL as i32).unwrap_or_default());
                settings.low_threshold = threshold;
                let startup =
                    checked(GetDlgItem(Some(dialog.hwnd), STARTUP as i32).unwrap_or_default());
                if !self.preview
                    && startup != startup_enabled()
                    && let Err(error) = set_startup(startup)
                {
                    fatal(&error);
                    return;
                }
            }
            Kind::Add => return,
        }
        if let Err(error) = self.persist(&settings) {
            fatal(&error);
            return;
        }
        {
            let mut model = self.model.lock().unwrap();
            for saved in &settings.peers {
                if let Some(peer) = model.peers.get_mut(&saved.address) {
                    peer.saved = saved.clone();
                }
            }
            model.settings = settings;
        }
        self.close_dialog();
        self.refresh();
    }

    unsafe fn forget(&mut self) {
        let Some(address) = self.selected() else {
            return;
        };
        let mut settings = self.model.lock().unwrap().settings.clone();
        let label = settings
            .peers
            .iter()
            .find(|peer| peer.address == address)
            .map(|peer| peer.label().to_string())
            .unwrap_or_default();
        let message = tf(
            "Remove {0} from this PC's list?\nWindows pairing is kept. To also stop Android from sending, stop the connection in the Android app.",
            &[&label],
        );
        if MessageBoxW(
            Some(self.main),
            PCWSTR(wide(&message).as_ptr()),
            pw("Remove device"),
            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
        ) != IDYES
        {
            return;
        }
        settings.peers.retain(|peer| peer.address != address);
        if settings.tray_peer == Some(address) {
            settings.tray_peer = None;
        }
        if let Err(error) = self.persist(&settings) {
            fatal(&error);
            return;
        }
        self.engine.stop(address);
        {
            let mut model = self.model.lock().unwrap();
            model.settings = settings;
            model.peers.remove(&address);
        }
        self.refresh();
    }

    unsafe fn set_enabled(&mut self, enabled: bool) {
        let Some(address) = self.selected() else {
            return;
        };
        let mut settings = self.model.lock().unwrap().settings.clone();
        if let Some(peer) = settings.peers.iter_mut().find(|p| p.address == address) {
            peer.enabled = enabled;
        }
        if let Err(error) = self.persist(&settings) {
            fatal(&error);
            return;
        }
        {
            let mut model = self.model.lock().unwrap();
            model.settings = settings;
            if let Some(peer) = model.peers.get_mut(&address) {
                peer.saved.enabled = enabled;
            }
        }
        if self.preview {
            if let Some(peer) = self.model.lock().unwrap().peers.get_mut(&address) {
                peer.state = if enabled {
                    State::Retrying
                } else {
                    State::Stopped
                };
            }
        } else if enabled {
            self.engine.start(address, true);
        } else {
            self.engine.stop(address);
        }
        self.refresh();
    }

    fn tray_data(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.host,
            uID: 1,
            ..Default::default()
        }
    }
    unsafe fn add_tray(&mut self) -> windows::core::Result<()> {
        let artwork = self.current_tray_artwork();
        let icon = artwork.create()?;
        let mut data = self.tray_data();
        data.uFlags = NIF_ICON | NIF_MESSAGE;
        data.uCallbackMessage = TRAY;
        data.hIcon = icon;
        if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
            let _ = DestroyIcon(icon);
            return Err(windows::core::Error::from_thread());
        }
        let old = std::mem::replace(&mut self.tray_icon, icon);
        if !old.is_invalid() {
            let _ = DestroyIcon(old);
        }
        self.tray_artwork = Some(artwork);
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
        self.tray_freshness_timer(artwork);
        Ok(())
    }
    unsafe fn current_tray_artwork(&self) -> Artwork {
        let model = self.model.lock().unwrap();
        let level = model
            .settings
            .tray_peer
            .and_then(|address| model.peers.get(&address))
            .and_then(crate::tray::live_level);
        Artwork::current(theme::tray_color(&self.theme), level)
    }
    unsafe fn tray_freshness_timer(&self, artwork: Artwork) {
        // Only a displayed number needs an expiry wake-up while all windows are closed.
        if artwork.level.is_some() {
            SetTimer(Some(self.host), 2, 60_000, None);
        } else {
            let _ = KillTimer(Some(self.host), 2);
        }
    }
    unsafe fn update_tray_icon(&mut self) {
        let artwork = self.current_tray_artwork();
        if self.tray_artwork == Some(artwork) {
            return;
        }
        let Ok(icon) = artwork.create() else {
            return;
        };
        let mut data = self.tray_data();
        data.uFlags = NIF_ICON;
        data.hIcon = icon;
        if Shell_NotifyIconW(NIM_MODIFY, &data).as_bool() {
            let old = std::mem::replace(&mut self.tray_icon, icon);
            self.tray_artwork = Some(artwork);
            if !old.is_invalid() {
                let _ = DestroyIcon(old);
            }
            self.tray_freshness_timer(artwork);
        } else {
            let _ = DestroyIcon(icon);
        }
    }
    unsafe fn remove_tray(&self) {
        let _ = Shell_NotifyIconW(NIM_DELETE, &self.tray_data());
    }
    unsafe fn notices(&self) {
        let notices: Vec<_> = self.model.lock().unwrap().notices.drain(..).collect();
        // A balloon may be suppressed by Windows notification settings or Focus Assist.
        if let Some((title, message)) = notice_balloon(&notices) {
            let mut data = self.tray_data();
            data.uFlags = NIF_INFO;
            data.dwInfoFlags = NIIF_INFO | NIIF_RESPECT_QUIET_TIME;
            copy_wide(&mut data.szInfoTitle, &title);
            copy_wide(&mut data.szInfo, &message);
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }
    unsafe fn tray_menu(&mut self) {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let model = self.model.lock().unwrap();
        let addresses: Vec<_> = model.peers.keys().copied().collect();
        if let Ok(display) = CreatePopupMenu() {
            let _ = AppendMenuW(display, MF_STRING, 1000, pw("Battery outline (default)"));
            for (index, peer) in model.peers.values().enumerate() {
                let label = tf("Show {0} level as number", &[&peer.saved.label().replace('&', "&&")]);
                let _ = AppendMenuW(
                    display,
                    MF_STRING,
                    1001 + index,
                    PCWSTR(wide(&label).as_ptr()),
                );
            }
            let selected = model
                .settings
                .tray_peer
                .and_then(|address| addresses.iter().position(|value| *value == address))
                .map_or(1000, |index| 1001 + index as u32);
            let _ = CheckMenuRadioItem(
                display,
                1000,
                1000 + addresses.len() as u32,
                selected,
                MF_BYCOMMAND.0,
            );
            if AppendMenuW(menu, MF_POPUP, display.0 as usize, pw("&Tray icon")).is_err()
            {
                let _ = DestroyMenu(display);
            }
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        }
        if model.peers.is_empty() {
            let _ = AppendMenuW(
                menu,
                MF_STRING | MF_DISABLED,
                10,
                pw("No connected devices"),
            );
        }
        for (index, peer) in model.peers.values().enumerate() {
            let label = format!(
                "{}   {}   {} · {}",
                peer.saved.label().replace('&', "&&"),
                peer.battery_text(),
                peer.charge_text(),
                peer.status_text()
            );
            let _ = AppendMenuW(menu, MF_STRING, 20 + index, PCWSTR(wide(&label).as_ptr()));
        }
        drop(model);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, 1, pw("&Open Device Battery"));
        let _ = AppendMenuW(menu, MF_STRING, 2, pw("&Add device"));
        let _ = AppendMenuW(menu, MF_STRING, 4, pw("&Settings"));
        let _ = AppendMenuW(menu, MF_STRING, 3, pw("E&xit"));
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let _ = SetForegroundWindow(self.host);
        let selected = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            Some(0),
            self.host,
            None,
        )
        .0;
        let _ = PostMessageW(Some(self.host), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        match selected {
            1000..=1016 => {
                let mut settings = self.model.lock().unwrap().settings.clone();
                settings.tray_peer = if selected == 1000 {
                    None
                } else {
                    addresses.get(selected as usize - 1001).copied()
                };
                if let Err(error) = self.persist(&settings) {
                    fatal(&error);
                } else {
                    self.model.lock().unwrap().settings = settings;
                    self.refresh();
                }
            }
            1 | 20..=35 => {
                let _ = self.show_main();
            }
            2 => {
                let _ = self.show_main();
                self.open_dialog(Kind::Add);
            }
            3 => {
                PostQuitMessage(0);
            }
            4 => {
                let _ = self.show_main();
                self.open_dialog(Kind::Settings);
            }
            _ => {}
        }
    }
}

/// Explicit, isolated, non-persistent fixtures for visual QA. Never start BLE or
/// raise notifications in this mode; every visible surface labels the preview.
fn preview_model(model: &mut Model) {
    use std::time::{Duration, SystemTime};
    model.peers.clear();
    model.settings.peers.clear();
    if std::env::args().any(|a| a == "--preview-empty") {
        return;
    }
    for (address, name, level, charging, state, seconds) in [
        (1, "Galaxy S25", Some(73), Some(true), State::Connected, 24),
        (
            2,
            "Galaxy Tab S10",
            Some(42),
            Some(false),
            State::Connected,
            1860,
        ),
        (
            3,
            t("Work phone"),
            Some(18),
            Some(false),
            State::Retrying,
            420,
        ),
        (4, t("Device not received yet"), None, None, State::Stopped, 0),
    ] {
        let mut saved = SavedPeer::new(address, name.into(), false);
        saved.enabled = state != State::Stopped;
        model.settings.peers.push(saved.clone());
        let mut peer = Peer::new(saved);
        peer.state = state;
        peer.level = level;
        peer.charging = charging;
        peer.plugged = charging;
        peer.updated = level.map(|_| SystemTime::now() - Duration::from_secs(seconds));
        model.peers.insert(address, peer);
    }
}

unsafe fn set_checked(hwnd: HWND, value: bool) {
    send(
        hwnd,
        BM_SETCHECK,
        WPARAM(if value {
            BST_CHECKED.0 as usize
        } else {
            BST_UNCHECKED.0 as usize
        }),
        LPARAM(0),
    );
}
unsafe fn checked(hwnd: HWND) -> bool {
    send(hwnd, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == BST_CHECKED.0 as isize
}
unsafe fn control_text(hwnd: HWND) -> String {
    let mut text = vec![0u16; GetWindowTextLengthW(hwnd).max(0) as usize + 1];
    let count = GetWindowTextW(hwnd, &mut text);
    String::from_utf16_lossy(&text[..count as usize])
}

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
unsafe fn startup_enabled() -> bool {
    let mut size = 0;
    RegGetValueW(
        HKEY_CURRENT_USER,
        RUN_KEY,
        w!("NearLink"),
        RRF_RT_REG_SZ,
        None,
        None,
        Some(&mut size),
    )
    .is_ok()
        && size > 2
}
unsafe fn set_startup(enabled: bool) -> Result<(), String> {
    if !enabled {
        let error = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, w!("NearLink"));
        if error == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        return error.ok().map_err(|e| e.to_string());
    }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let command = wide(&format!("\"{}\" --hidden", executable.display()));
    let bytes = std::slice::from_raw_parts(command.as_ptr() as *const u8, command.len() * 2);
    RegSetKeyValueW(
        HKEY_CURRENT_USER,
        RUN_KEY,
        w!("NearLink"),
        REG_SZ.0,
        Some(bytes.as_ptr().cast()),
        bytes.len() as u32,
    )
    .ok()
    .map_err(|e| e.to_string())
}

unsafe fn battery_icon(instance: HINSTANCE) -> windows::core::Result<HICON> {
    let mut mask = [255u8; 128];
    let mut pixels = [0u8; 32 * 32 * 4];
    for y in 7..25 {
        for x in 4..28 {
            let body = (4..25).contains(&x) && (8..24).contains(&y);
            let terminal = (25..28).contains(&x) && (13..19).contains(&y);
            let hole = (7..22).contains(&x) && (11..21).contains(&y);
            let fill = (7..18).contains(&x) && (11..21).contains(&y);
            if (body && !hole) || terminal || fill {
                let index = (y * 32 + x) * 4;
                pixels[index..index + 4].copy_from_slice(&[173, 91, 24, 255]);
                mask[y * 4 + x / 8] &= !(0x80 >> (x % 8));
            }
        }
    }
    CreateIcon(
        Some(instance),
        32,
        32,
        1,
        32,
        mask.as_ptr(),
        pixels.as_ptr(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestWindow(HWND);
    impl Drop for TestWindow {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }

    #[test]
    fn text_updates_leave_unchanged_and_hidden_controls_alone() {
        unsafe {
            // An offscreen parent lets this exercise real native controls without
            // opening a window or relying on BLE, timers, or screenshot timing.
            let parent = TestWindow(
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("STATIC"),
                    w!(""),
                    WS_OVERLAPPED,
                    0,
                    0,
                    200,
                    100,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap(),
            );
            let label = TestWindow(
                CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("STATIC"),
                    w!("연결됨"),
                    WS_CHILD | WS_VISIBLE,
                    0,
                    0,
                    180,
                    30,
                    Some(parent.0),
                    None,
                    None,
                    None,
                )
                .unwrap(),
            );
            let _ = ValidateRect(Some(label.0), None);
            set_text_if_changed(label.0, "연결됨");
            assert!(!GetUpdateRect(label.0, None, false).as_bool());
            assert!(!GetUpdateRect(parent.0, None, false).as_bool());

            set_text_if_changed(label.0, "연결 중");
            let mut text = [0u16; 32];
            let length = GetWindowTextW(label.0, &mut text) as usize;
            assert_eq!(String::from_utf16_lossy(&text[..length]), "연결 중");
            assert!(!GetUpdateRect(parent.0, None, false).as_bool());
            let _ = ValidateRect(Some(label.0), None);
            set_text_if_changed(label.0, "연결 중");
            assert!(!GetUpdateRect(label.0, None, false).as_bool());

            let _ = ShowWindow(label.0, SW_HIDE);
            set_text_if_changed(label.0, "수신 전");
            enable_if_changed(label.0, false);
            assert_eq!(
                GetWindowLongPtrW(label.0, GWL_STYLE) as u32 & WS_VISIBLE.0,
                0
            );
            assert!(!IsWindowEnabled(label.0).as_bool());
        }
    }
}
