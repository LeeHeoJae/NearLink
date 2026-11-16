//! Fluent-inspired color and type over native Win32 controls. No private UXTheme APIs.
#![allow(unsafe_op_in_unsafe_fn)]
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        System::Registry::*,
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            WindowsAndMessaging::*,
        },
    },
    core::{BOOL, w},
};

pub fn preview() -> bool {
    std::env::args().any(|a| a == "--preview")
}
fn argument(key: &str) -> Option<String> {
    let args: Vec<_> = std::env::args().collect();
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
unsafe fn dword(path: windows::core::PCWSTR, key: windows::core::PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut size = 4;
    RegGetValueW(
        HKEY_CURRENT_USER,
        path,
        key,
        RRF_RT_REG_DWORD,
        None,
        Some((&mut value as *mut u32).cast()),
        Some(&mut size),
    )
    .ok()
    .ok()
    .map(|_| value)
}
pub unsafe fn text_scale() -> u32 {
    if preview()
        && let Some(value) = argument("--preview-text-scale").and_then(|s| s.parse::<u32>().ok())
    {
        return value.clamp(100, 225);
    }
    dword(
        w!("Software\\Microsoft\\Accessibility"),
        w!("TextScaleFactor"),
    )
    .unwrap_or(100)
    .clamp(100, 225)
}
pub const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

/// The taskbar follows Windows mode, independently of the app's light/dark mode.
pub unsafe fn tray_color(theme: &Theme) -> COLORREF {
    if theme.contrast {
        return theme.text;
    }
    let dark = if preview() && argument("--preview-theme").is_some() {
        theme.dark
    } else {
        dword(
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
        ) != Some(1)
    };
    if dark {
        rgb(238, 238, 238)
    } else {
        rgb(38, 38, 38)
    }
}

pub struct Theme {
    pub dark: bool,
    pub contrast: bool,
    pub background: COLORREF,
    pub surface: COLORREF,
    pub text: COLORREF,
    pub secondary: COLORREF,
    pub accent: COLORREF,
    pub on_accent: COLORREF,
    pub selected: COLORREF,
    pub on_selected: COLORREF,
    pub border: COLORREF,
    /// Caution text for lost connections and old values; always paired with words.
    pub warning: COLORREF,
    pub brush: HBRUSH,
    pub surface_brush: HBRUSH,
}
impl Theme {
    pub unsafe fn load() -> Self {
        let mut hc = HIGHCONTRASTW {
            cbSize: size_of::<HIGHCONTRASTW>() as u32,
            ..Default::default()
        };
        let _ = SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            Some((&mut hc as *mut HIGHCONTRASTW).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let forced = if preview() {
            argument("--preview-theme")
        } else {
            None
        };
        let contrast =
            (hc.dwFlags & HCF_HIGHCONTRASTON).0 != 0 || forced.as_deref() == Some("contrast");
        let dark = match forced.as_deref() {
            Some("dark" | "contrast") => true,
            Some("light") => false,
            _ => {
                dword(
                    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                    w!("AppsUseLightTheme"),
                ) == Some(0)
            }
        };
        let (
            background,
            surface,
            text,
            secondary,
            accent,
            on_accent,
            selected,
            on_selected,
            border,
            warning,
        ) = if contrast {
            // Follow the actual contrast palette. A forced preview uses a deterministic test palette.
            if forced.as_deref() == Some("contrast") && (hc.dwFlags & HCF_HIGHCONTRASTON).0 == 0 {
                (
                    rgb(0, 0, 0),
                    rgb(0, 0, 0),
                    rgb(255, 255, 255),
                    rgb(255, 255, 255),
                    rgb(255, 255, 0),
                    rgb(0, 0, 0),
                    rgb(255, 255, 0),
                    rgb(0, 0, 0),
                    rgb(255, 255, 255),
                    rgb(255, 255, 255),
                )
            } else {
                (
                    COLORREF(GetSysColor(COLOR_WINDOW)),
                    COLORREF(GetSysColor(COLOR_WINDOW)),
                    COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
                    COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
                    COLORREF(GetSysColor(COLOR_HIGHLIGHT)),
                    COLORREF(GetSysColor(COLOR_HIGHLIGHTTEXT)),
                    COLORREF(GetSysColor(COLOR_HIGHLIGHT)),
                    COLORREF(GetSysColor(COLOR_HIGHLIGHTTEXT)),
                    COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
                    COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
                )
            }
        } else if dark {
            (
                rgb(32, 32, 32),
                rgb(39, 39, 39),
                rgb(243, 243, 246),
                rgb(188, 188, 198),
                rgb(155, 191, 255),
                rgb(0, 47, 119),
                rgb(24, 52, 92),
                rgb(243, 243, 246),
                rgb(65, 65, 69),
                rgb(252, 225, 0),
            )
        } else {
            (
                rgb(243, 243, 246),
                rgb(255, 255, 255),
                rgb(23, 23, 27),
                rgb(89, 89, 97),
                rgb(21, 89, 206),
                rgb(255, 255, 255),
                rgb(232, 240, 255),
                rgb(23, 23, 27),
                rgb(224, 224, 230),
                rgb(157, 93, 0),
            )
        };
        Self {
            dark,
            contrast,
            background,
            surface,
            text,
            secondary,
            accent,
            on_accent,
            selected,
            on_selected,
            border,
            warning,
            brush: CreateSolidBrush(background),
            surface_brush: CreateSolidBrush(surface),
        }
    }
    pub unsafe fn window(&self, hwnd: HWND) {
        let dark = BOOL::from(self.dark && !self.contrast);
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&dark as *const BOOL).cast(),
            size_of::<BOOL>() as u32,
        );
        let corner = if self.contrast {
            DWMWCP_DONOTROUND
        } else {
            DWMWCP_ROUND
        };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&corner as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
            size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
    }
}
impl Drop for Theme {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.brush.into());
            let _ = DeleteObject(self.surface_brush.into());
        }
    }
}
pub unsafe fn fill(dc: HDC, rect: &RECT, color: COLORREF) {
    let brush = CreateSolidBrush(color);
    FillRect(dc, rect, brush);
    let _ = DeleteObject(brush.into());
}
/// Windows 11 style rounded surface; a zero radius (high contrast) stays square.
pub unsafe fn round(dc: HDC, rect: &RECT, color: COLORREF, border: Option<COLORREF>, radius: i32) {
    let brush = CreateSolidBrush(color);
    let pen = CreatePen(PS_SOLID, 1, border.unwrap_or(color));
    let old_brush = SelectObject(dc, brush.into());
    let old_pen = SelectObject(dc, pen.into());
    if radius > 0 {
        let _ = RoundRect(dc, rect.left, rect.top, rect.right, rect.bottom, radius * 2, radius * 2);
    } else {
        let _ = Rectangle(dc, rect.left, rect.top, rect.right, rect.bottom);
    }
    SelectObject(dc, old_pen);
    SelectObject(dc, old_brush);
    let _ = DeleteObject(pen.into());
    let _ = DeleteObject(brush.into());
}
pub unsafe fn text(
    dc: HDC,
    font: HFONT,
    color: COLORREF,
    value: &str,
    mut rect: RECT,
    flags: DRAW_TEXT_FORMAT,
) {
    // An empty slice would hand DrawText a dangling pointer.
    if value.is_empty() {
        return;
    }
    let old = SelectObject(dc, font.into());
    SetTextColor(dc, color);
    SetBkMode(dc, TRANSPARENT);
    let mut value: Vec<_> = value.encode_utf16().collect();
    DrawTextW(dc, &mut value, &mut rect, flags | DT_NOPREFIX);
    SelectObject(dc, old);
}
