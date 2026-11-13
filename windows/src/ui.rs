//! Native controls remain accessible to keyboard navigation and Windows UI Automation.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::{
    ble::{Engine, Wake},
    config::{self, SavedPeer},
    model::{Model, Peer, Shared, State},
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
            Input::KeyboardAndMouse::{EnableWindow, SetFocus},
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
const ADD: usize = 100;
const RETRY: usize = 101;
const STOP: usize = 102;
const ALIAS: usize = 103;
const FORGET: usize = 104;
const SETTINGS: usize = 105;
const LIST: usize = 110;
const CONNECT: usize = 201;
const SEARCH: usize = 202;
const BT_SETTINGS: usize = 203;
const SAVE: usize = 204;
const CANCEL: usize = 205;
const INPUT: usize = 210;
const ALERTS: usize = 211;
const FULL: usize = 212;
const STARTUP: usize = 213;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
unsafe fn send(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    SendMessageW(hwnd, message, Some(wparam), Some(lparam))
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
    info: HWND,
    font: HFONT,
    icon: HICON,
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
        let model = Arc::new(Mutex::new(Model::new(settings)));
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
        let engine = Engine::new(model.clone(), wake);
        let app = Box::new(RefCell::new(App {
            host,
            main: HWND::default(),
            list: HWND::default(),
            info: HWND::default(),
            font: HFONT::default(),
            icon,
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
        app.borrow().add_tray().map_err(|e| e.to_string())?;
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
        for address in addresses {
            app.borrow_mut().engine.start(address, false);
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
            let main = app.borrow().main;
            if !main.is_invalid() && IsDialogMessageW(main, &message).as_bool() {
                continue;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let mut state = app.borrow_mut();
        state.engine.cancel_scan();
        state.remove_tray();
        if !state.font.is_invalid() {
            let _ = DeleteObject(state.font.into());
        }
        let _ = DestroyIcon(state.icon);
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
    // Win32 may synchronously reenter this procedure (control creation, dialogs, layout).
    // RefCell prevents aliasing mutable application state across those callbacks.
    let Ok(mut state) = (*pointer).try_borrow_mut() else {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    };
    // Ignore synchronous control/paint messages before borrowing application state.
    match message {
        WAKE | SHOW | TRAY | WM_COMMAND | WM_NOTIFY | WM_CLOSE | WM_SIZE | WM_TIMER
        | WM_DPICHANGED | WM_GETMINMAXINFO | WM_ENDSESSION => {}
        other if other == state.taskbar_message => {
            let _ = state.add_tray();
            return LRESULT(0);
        }
        _ => return DefWindowProcW(hwnd, message, wparam, lparam),
    }
    let app = &mut *state;
    match message {
        WAKE => {
            app.refresh();
            app.notices();
            LRESULT(0)
        }
        SHOW => {
            let _ = app.show_main();
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
            if hwnd == app.main && header.idFrom == LIST && header.code == LVN_GETEMPTYMARKUP {
                let markup = &mut *(lparam.0 as *mut NMLVEMPTYMARKUP);
                markup.dwFlags = EMF_CENTERED;
                copy_wide(
                    &mut markup.szMarkup,
                    "연결한 기기가 없습니다.\nAndroid NearLink에서 연결 준비를 누른 뒤 기기를 추가하세요.",
                );
                return LRESULT(1);
            }
            if hwnd == app.main && header.idFrom == LIST && header.code == LVN_ITEMCHANGED {
                app.selection();
            }
            LRESULT(0)
        }
        WM_SIZE => {
            if hwnd == app.main {
                app.layout();
            }
            LRESULT(0)
        }
        WM_GETMINMAXINFO if hwnd == app.main => {
            let info = &mut *(lparam.0 as *mut MINMAXINFO);
            info.ptMinTrackSize = POINT {
                x: app.s(780),
                y: app.s(450),
            };
            LRESULT(0)
        }
        WM_DPICHANGED if hwnd == app.main => {
            app.dpi = (wparam.0 & 0xffff) as u32;
            app.update_font();
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
            LRESULT(0)
        }
        WM_CLOSE => {
            if app.dialog.as_ref().is_some_and(|d| d.hwnd == hwnd) {
                app.close_dialog();
            } else if hwnd == app.main {
                app.close_dialog();
                let _ = KillTimer(Some(hwnd), 1);
                app.main = HWND::default();
                app.rows.clear();
                let _ = DestroyWindow(hwnd);
                if !app.font.is_invalid() {
                    let _ = DeleteObject(app.font.into());
                    app.font = HFONT::default();
                }
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
                let _ = app.show_main();
            } else if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                app.tray_menu();
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

impl App {
    fn s(&self, value: i32) -> i32 {
        (value * self.dpi as i32 + 48) / 96
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
        let previous = self.font;
        self.font = CreateFontW(
            -self.s(15),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            DEFAULT_PITCH.0 as u32,
            w!("Segoe UI"),
        );
        if !self.main.is_invalid() {
            let mut child = GetWindow(self.main, GW_CHILD).unwrap_or_default();
            while !child.is_invalid() {
                send(child, WM_SETFONT, WPARAM(self.font.0 as usize), LPARAM(1));
                child = GetWindow(child, GW_HWNDNEXT).unwrap_or_default();
            }
        }
        if !previous.is_invalid() {
            let _ = DeleteObject(previous.into());
        }
    }

    unsafe fn show_main(&mut self) -> windows::core::Result<()> {
        if !self.main.is_invalid() {
            let _ = ShowWindow(self.main, SW_RESTORE);
            let _ = SetForegroundWindow(self.main);
            return Ok(());
        }
        self.main = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            CLASS,
            w!("NearLink"),
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
            "기기 배터리",
            WINDOW_STYLE::default(),
            300,
        );
        self.control(
            self.main,
            w!("STATIC"),
            "가까이 있는 휴대폰과 태블릿의 배터리를 확인하세요.",
            WINDOW_STYLE::default(),
            301,
        );
        for (text, id) in [
            ("기기 추가(&A)", ADD),
            ("다시 연결(&R)", RETRY),
            ("수신 중지(&S)", STOP),
            ("별명(&N)", ALIAS),
            ("목록에서 제거(&D)", FORGET),
            ("설정(&O)", SETTINGS),
        ] {
            self.button(self.main, text, id);
        }
        self.list = self.control(
            self.main,
            WC_LISTVIEWW,
            "",
            WS_TABSTOP | WS_BORDER | WINDOW_STYLE(LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS),
            LIST,
        );
        send(
            self.list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            WPARAM(0),
            LPARAM((LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize),
        );
        for (index, text) in ["기기", "배터리", "충전 상태", "연결", "마지막 수신"]
            .iter()
            .enumerate()
        {
            let mut label = wide(text);
            let column = LVCOLUMNW {
                mask: LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM,
                pszText: windows::core::PWSTR(label.as_mut_ptr()),
                cx: self.s(140),
                iSubItem: index as i32,
                ..Default::default()
            };
            send(
                self.list,
                LVM_INSERTCOLUMNW,
                WPARAM(index),
                LPARAM(&column as *const _ as isize),
            );
        }
        self.info = self.control(self.main, w!("STATIC"), "", WINDOW_STYLE::default(), 303);
        self.control(
            self.main,
            w!("STATIC"),
            "창을 닫아도 트레이에서 수신합니다. 종료하려면 트레이 메뉴를 사용하세요.",
            WINDOW_STYLE::default(),
            304,
        );
        let _ = SetWindowPos(
            self.main,
            None,
            0,
            0,
            self.s(960),
            self.s(570),
            SWP_NOMOVE | SWP_NOZORDER,
        );
        self.layout();
        self.refresh();
        SetTimer(Some(self.main), 1, 10_000, None);
        let _ = ShowWindow(self.main, SW_SHOW);
        let _ = SetForegroundWindow(self.main);
        let _ = SetFocus(Some(GetDlgItem(Some(self.main), ADD as i32)?));
        Ok(())
    }

    unsafe fn layout(&self) {
        if self.main.is_invalid() {
            return;
        }
        let mut bounds = RECT::default();
        let _ = GetClientRect(self.main, &mut bounds);
        let width = bounds.right * 96 / self.dpi as i32;
        let height = bounds.bottom * 96 / self.dpi as i32;
        let item = |id| GetDlgItem(Some(self.main), id).unwrap_or_default();
        self.place(item(300), 24, 18, width - 48, 25);
        self.place(item(301), 24, 48, width - 48, 24);
        let mut x = 24;
        for (id, size) in [
            (ADD, 112),
            (RETRY, 112),
            (STOP, 112),
            (ALIAS, 76),
            (FORGET, 132),
        ] {
            self.place(item(id as i32), x, 86, size, 34);
            x += size + 8;
        }
        self.place(item(SETTINGS as i32), width - 104, 86, 80, 34);
        self.place(self.list, 24, 136, width - 48, (height - 254).max(120));
        self.place(self.info, 24, height - 102, width - 48, 54);
        self.place(item(304), 24, height - 35, width - 48, 24);
        let columns = [
            210,
            86,
            136,
            144,
            (width - 48 - 210 - 86 - 136 - 144 - 6).max(100),
        ];
        for (index, size) in columns.into_iter().enumerate() {
            send(
                self.list,
                LVM_SETCOLUMNWIDTH,
                WPARAM(index),
                LPARAM(self.s(size) as isize),
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
        for id in [RETRY, STOP, ALIAS, FORGET] {
            let enabled = peer.is_some_and(|p| id != STOP || p.state != State::Stopped);
            let _ = EnableWindow(
                GetDlgItem(Some(self.main), id as i32).unwrap_or_default(),
                enabled,
            );
        }
        let detail = peer.map(|peer| {
            if !peer.detail.is_empty() { peer.detail.clone() }
            else if peer.state != State::Connected && peer.updated.is_some() { "마지막으로 받은 잔량입니다. 현재 값과 다를 수 있습니다.".into() }
            else { "배터리 값이 바뀌면 자동으로 갱신합니다. 마지막 수신 시각은 배터리 데이터 수신 기준입니다.".into() }
        }).unwrap_or_else(|| if model.message.is_empty() { "기기를 선택하면 연결 상태를 확인할 수 있습니다.".into() } else { model.message.clone() });
        let _ = SetWindowTextW(self.info, PCWSTR(wide(&detail).as_ptr()));
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
            && let Err(error) = config::save(&self.directory, &settings)
        {
            self.model.lock().unwrap().message = error;
        }
        self.update_tooltip();
        if !self.main.is_invalid() {
            let selected = self.selected();
            // Disable notifications while replacing rows to avoid borrowing state reentrantly.
            SetWindowLongPtrW(self.main, GWLP_USERDATA, 0);
            send(self.list, WM_SETREDRAW, WPARAM(0), LPARAM(0));
            send(self.list, LVM_DELETEALLITEMS, WPARAM(0), LPARAM(0));
            self.rows.clear();
            let model = self.model.lock().unwrap();
            for (row, (address, peer)) in model.peers.iter().enumerate() {
                self.rows.push(*address);
                let texts = [
                    peer.saved.label().to_string(),
                    peer.battery_text(),
                    peer.charge_text().into(),
                    peer.state.text().into(),
                    peer.age_text(),
                ];
                for (column, text) in texts.iter().enumerate() {
                    let mut text = wide(text);
                    let item = LVITEMW {
                        mask: LVIF_TEXT,
                        iItem: row as i32,
                        iSubItem: column as i32,
                        pszText: windows::core::PWSTR(text.as_mut_ptr()),
                        ..Default::default()
                    };
                    send(
                        self.list,
                        if column == 0 {
                            LVM_INSERTITEMW
                        } else {
                            LVM_SETITEMTEXTW
                        },
                        WPARAM(if column == 0 { 0 } else { row }),
                        LPARAM(&item as *const _ as isize),
                    );
                }
                if selected == Some(*address) {
                    let item = LVITEMW {
                        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
                        state: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
                        ..Default::default()
                    };
                    send(
                        self.list,
                        LVM_SETITEMSTATE,
                        WPARAM(row),
                        LPARAM(&item as *const _ as isize),
                    );
                }
            }
            drop(model);
            send(self.list, WM_SETREDRAW, WPARAM(1), LPARAM(0));
            let _ = InvalidateRect(Some(self.list), None, true);
            SetWindowLongPtrW(self.main, GWLP_USERDATA, self.callback as isize);
            self.selection();
        }
        self.refresh_dialog();
    }

    unsafe fn command(&mut self, hwnd: HWND, id: usize, notification: u16) {
        if self
            .dialog
            .as_ref()
            .is_some_and(|dialog| dialog.hwnd == hwnd)
        {
            match id {
                CANCEL | 2 => self.close_dialog(),
                SEARCH => self.engine.scan(),
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
            ADD => self.open_dialog(Kind::Add),
            SETTINGS => self.open_dialog(Kind::Settings),
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
        let title = match kind {
            Kind::Add => "기기 추가",
            Kind::Alias(_) => "기기 별명",
            Kind::Settings => "설정",
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
                    w!(
                        "Android NearLink에서 연결 준비를 누르세요.\r\n검색된 기기를 선택하고 연결하면 페어링을 진행합니다."
                    ),
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
                self.place(self.button(hwnd, "다시 검색(&R)", SEARCH), 20, 288, 122, 34);
                self.place(
                    self.button(hwnd, "블루투스 설정(&B)", BT_SETTINGS),
                    150,
                    288,
                    162,
                    34,
                );
                self.place(
                    self.button(hwnd, "선택한 기기 연결(&C)", CONNECT),
                    320,
                    288,
                    245,
                    34,
                );
                let status =
                    self.control(hwnd, w!("STATIC"), "검색 중…", WINDOW_STYLE::default(), 301);
                self.place(status, 20, 335, 545, 65);
            }
            Kind::Alias(address) => {
                self.place(info, 20, 20, 545, 50);
                let _ = SetWindowTextW(
                    info,
                    w!("기기 별명(&N)\r\n비워 두면 블루투스 기기 이름을 사용합니다."),
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
                self.place(self.button(hwnd, "저장(&S)", SAVE), 350, 172, 100, 34);
                self.place(self.button(hwnd, "취소", CANCEL), 465, 172, 100, 34);
            }
            Kind::Settings => {
                self.place(info, 20, 16, 545, 24);
                let _ = SetWindowTextW(info, w!("알림과 자동 시작"));
                let settings = &self.model.lock().unwrap().settings;
                let alerts = self.control(
                    hwnd,
                    w!("BUTTON"),
                    "배터리 부족 알림(&L)",
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
                    "% 이하 · 충전 중에는 알리지 않음",
                    WINDOW_STYLE::default(),
                    301,
                );
                self.place(unit, 288, 58, 277, 26);
                let full = self.control(
                    hwnd,
                    w!("BUTTON"),
                    "100% 충전 완료 알림(&F)",
                    WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                    FULL,
                );
                self.place(full, 20, 94, 400, 28);
                set_checked(full, settings.full_notifications);
                let startup = self.control(
                    hwnd,
                    w!("BUTTON"),
                    "Windows 로그인 시 트레이에서 시작(&W)",
                    WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                    STARTUP,
                );
                self.place(startup, 20, 134, 510, 28);
                set_checked(startup, startup_enabled());
                self.place(self.button(hwnd, "저장(&S)", SAVE), 350, 202, 100, 34);
                self.place(self.button(hwnd, "취소", CANCEL), 465, 202, 100, 34);
            }
        }
        let scan = matches!(kind, Kind::Add);
        self.dialog = Some(Dialog {
            hwnd,
            kind,
            list,
            input,
        });
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, self.callback as isize);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        if !input.is_invalid() {
            let _ = SetFocus(Some(input));
        } else if !list.is_invalid() {
            let _ = SetFocus(Some(list));
        }
        if scan {
            self.engine.scan();
        }
        self.refresh_dialog();
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
        send(dialog.list, WM_SETREDRAW, WPARAM(0), LPARAM(0));
        send(dialog.list, LB_RESETCONTENT, WPARAM(0), LPARAM(0));
        for peer in &model.discovered {
            send(
                dialog.list,
                LB_ADDSTRING,
                WPARAM(0),
                LPARAM(wide(&peer.name).as_ptr() as isize),
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
        let _ = EnableWindow(
            GetDlgItem(Some(dialog.hwnd), SEARCH as i32).unwrap_or_default(),
            !model.scanning,
        );
        let _ = EnableWindow(
            GetDlgItem(Some(dialog.hwnd), CONNECT as i32).unwrap_or_default(),
            selected >= 0 && (selected as usize) < model.discovered.len(),
        );
        let status = if model.scanning {
            format!(
                "기기를 찾고 있습니다… {}대 발견 · 검색은 15초 후 끝납니다.",
                model.discovered.len()
            )
        } else if !model.message.is_empty() {
            model.message.clone()
        } else {
            format!(
                "{}대를 찾았습니다. 연결할 기기를 선택하세요.",
                model.discovered.len()
            )
        };
        let _ = SetWindowTextW(
            GetDlgItem(Some(dialog.hwnd), 301).unwrap_or_default(),
            PCWSTR(wide(&status).as_ptr()),
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
                fatal("최대 16대까지 등록할 수 있습니다. 사용하지 않는 기기를 먼저 제거하세요.");
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
        if let Err(error) = config::save(&self.directory, &settings) {
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
                    fatal("부족 알림 기준은 1~90 사이의 숫자를 입력하세요.");
                    return;
                };
                settings.notifications =
                    checked(GetDlgItem(Some(dialog.hwnd), ALERTS as i32).unwrap_or_default());
                settings.full_notifications =
                    checked(GetDlgItem(Some(dialog.hwnd), FULL as i32).unwrap_or_default());
                settings.low_threshold = threshold;
                let startup =
                    checked(GetDlgItem(Some(dialog.hwnd), STARTUP as i32).unwrap_or_default());
                if startup != startup_enabled()
                    && let Err(error) = set_startup(startup)
                {
                    fatal(&error);
                    return;
                }
            }
            Kind::Add => return,
        }
        if let Err(error) = config::save(&self.directory, &settings) {
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
        let message = format!(
            "{label} 기기를 이 PC의 목록에서 제거할까요?\nWindows 페어링은 유지됩니다. Android의 송신도 끝내려면 상대 앱에서 연결을 중지하세요."
        );
        if MessageBoxW(
            Some(self.main),
            PCWSTR(wide(&message).as_ptr()),
            w!("기기 제거"),
            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
        ) != IDYES
        {
            return;
        }
        settings.peers.retain(|peer| peer.address != address);
        if let Err(error) = config::save(&self.directory, &settings) {
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
        if let Err(error) = config::save(&self.directory, &settings) {
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
        if enabled {
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
    unsafe fn add_tray(&self) -> windows::core::Result<()> {
        let mut data = self.tray_data();
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = TRAY;
        data.hIcon = self.icon;
        copy_wide(&mut data.szTip, "NearLink · 기기 배터리");
        if !Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
            return Err(windows::core::Error::from_thread());
        }
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
        Ok(())
    }
    unsafe fn remove_tray(&self) {
        let _ = Shell_NotifyIconW(NIM_DELETE, &self.tray_data());
    }
    unsafe fn update_tooltip(&self) {
        let model = self.model.lock().unwrap();
        let labels: Vec<_> = model
            .peers
            .values()
            .take(4)
            .map(|peer| {
                format!(
                    "{} {} · {}",
                    peer.saved.label(),
                    peer.battery_text(),
                    peer.state.text()
                )
            })
            .collect();
        let mut data = self.tray_data();
        data.uFlags = NIF_TIP;
        copy_wide(
            &mut data.szTip,
            &format!(
                "NearLink\n{}",
                if labels.is_empty() {
                    "연결한 기기가 없습니다.".into()
                } else {
                    labels.join("\n")
                }
            ),
        );
        let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
    }
    unsafe fn notices(&self) {
        let notices: Vec<_> = self.model.lock().unwrap().notices.drain(..).collect();
        // A balloon may be suppressed by Windows notification settings or Focus Assist.
        if let Some((title, message)) = notices.last() {
            let mut data = self.tray_data();
            data.uFlags = NIF_INFO;
            data.dwInfoFlags = NIIF_INFO | NIIF_RESPECT_QUIET_TIME;
            copy_wide(&mut data.szInfoTitle, &format!("NearLink · {title}"));
            copy_wide(&mut data.szInfo, message);
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }
    unsafe fn tray_menu(&mut self) {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let model = self.model.lock().unwrap();
        if model.peers.is_empty() {
            let _ = AppendMenuW(
                menu,
                MF_STRING | MF_DISABLED,
                10,
                w!("연결한 기기가 없습니다"),
            );
        }
        for (index, peer) in model.peers.values().enumerate() {
            let label = format!(
                "{}   {}   {} · {}",
                peer.saved.label().replace('&', "&&"),
                peer.battery_text(),
                peer.charge_text(),
                peer.state.text()
            );
            let _ = AppendMenuW(menu, MF_STRING, 20 + index, PCWSTR(wide(&label).as_ptr()));
        }
        drop(model);
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, 1, w!("기기 배터리 열기(&O)"));
        let _ = AppendMenuW(menu, MF_STRING, 2, w!("기기 추가(&A)"));
        let _ = AppendMenuW(menu, MF_STRING, 3, w!("종료(&X)"));
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
            _ => {}
        }
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
