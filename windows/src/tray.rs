//! Monochrome tray artwork, rasterized at the shell's small-icon size.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::model::{Peer, State};
use windows::{
    Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*},
    core::w,
};

pub fn live_level(peer: &Peer) -> Option<u8> {
    if peer.saved.enabled
        && peer.state == State::Connected
        && !peer.stale()
        && peer.updated.is_some()
    {
        peer.level.filter(|level| *level <= 100)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SavedPeer;
    use std::time::{Duration, SystemTime};

    #[test]
    fn numbers_require_a_fresh_connected_enabled_peer() {
        let mut peer = Peer::new(SavedPeer::new(1, "Phone".into(), false));
        peer.level = Some(73);
        peer.state = State::Connected;
        assert_eq!(live_level(&peer), None);
        peer.updated = Some(SystemTime::now());
        assert_eq!(live_level(&peer), Some(73));
        peer.state = State::Retrying;
        assert_eq!(live_level(&peer), None);
        peer.state = State::Connected;
        peer.saved.enabled = false;
        assert_eq!(live_level(&peer), None);
        peer.saved.enabled = true;
        peer.updated = Some(SystemTime::now() - Duration::from_secs(900));
        assert_eq!(live_level(&peer), None);
        peer.updated = Some(SystemTime::now());
        peer.level = Some(100);
        assert_eq!(live_level(&peer), Some(100));
        peer.level = Some(101);
        assert_eq!(live_level(&peer), None);
    }

    #[test]
    fn native_artwork_supports_shell_sizes_and_level_extremes() {
        unsafe {
            for size in [16, 20, 24, 32] {
                for level in [None, Some(0), Some(9), Some(73), Some(100)] {
                    let icon = Artwork {
                        size,
                        color: COLORREF(0xeeeeee),
                        level,
                    }
                    .create()
                    .unwrap();
                    assert!(!icon.is_invalid());
                    let mut info = ICONINFO::default();
                    GetIconInfo(icon, &mut info).unwrap();
                    let mut bitmap = BITMAP::default();
                    assert_ne!(
                        GetObjectW(
                            info.hbmColor.into(),
                            size_of::<BITMAP>() as i32,
                            Some((&mut bitmap as *mut BITMAP).cast())
                        ),
                        0
                    );
                    assert_eq!(bitmap.bmWidth, size);
                    assert_eq!(bitmap.bmHeight, size);
                    let mut dib = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: size,
                            biHeight: -size,
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    let mut pixels = vec![0u8; (size * size * 4) as usize];
                    let dc = CreateCompatibleDC(None);
                    assert_eq!(
                        GetDIBits(
                            dc,
                            info.hbmColor,
                            0,
                            size as u32,
                            Some(pixels.as_mut_ptr().cast()),
                            &mut dib,
                            DIB_RGB_COLORS
                        ),
                        size
                    );
                    let _ = DeleteDC(dc);
                    assert!(pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0));
                    for y in 0..size {
                        for x in [0, size - 1] {
                            assert_eq!(
                                pixels[((y * size + x) * 4 + 3) as usize],
                                0,
                                "clipped artwork: {size}px, {level:?}"
                            );
                        }
                    }
                    let _ = DeleteObject(info.hbmColor.into());
                    let _ = DeleteObject(info.hbmMask.into());
                    let _ = DestroyIcon(icon);
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Artwork {
    pub size: i32,
    pub color: COLORREF,
    pub level: Option<u8>,
}

impl Artwork {
    pub unsafe fn current(color: COLORREF, level: Option<u8>) -> Self {
        Self {
            size: GetSystemMetrics(SM_CXSMICON).clamp(16, 64),
            color,
            level,
        }
    }

    pub unsafe fn create(self) -> windows::core::Result<HICON> {
        // Supersampling gives small curves and Segoe UI digits grayscale alpha,
        // without ClearType fringes or an opaque background in the taskbar.
        let scale = 4;
        let side = self.size * scale;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: side,
                biHeight: -side,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            let _ = DeleteObject(bitmap.into());
            return Err(windows::core::Error::from_thread());
        }
        let previous = SelectObject(dc, bitmap.into());
        let samples = std::slice::from_raw_parts_mut(bits.cast::<u8>(), (side * side * 4) as usize);
        samples.fill(0);
        let result = (|| {
            if let Some(level) = self.level {
                let font = CreateFontW(
                    -(side * 86 / 100),
                    if level == 100 { side * 28 / 100 } else { 0 },
                    0,
                    0,
                    FW_SEMIBOLD.0 as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    ANTIALIASED_QUALITY,
                    DEFAULT_PITCH.0 as u32,
                    w!("Segoe UI"),
                );
                if font.is_invalid() {
                    return Err(windows::core::Error::from_thread());
                }
                let old_font = SelectObject(dc, font.into());
                SetTextColor(dc, COLORREF(0xffffff));
                SetBkMode(dc, TRANSPARENT);
                let mut rect = RECT {
                    left: 0,
                    top: 0,
                    right: side,
                    bottom: side,
                };
                let mut digits: Vec<u16> = level.to_string().encode_utf16().collect();
                DrawTextW(
                    dc,
                    &mut digits,
                    &mut rect,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
                );
                SelectObject(dc, old_font);
                let _ = DeleteObject(font.into());
            } else {
                let white = CreateSolidBrush(COLORREF(0xffffff));
                let black = CreateSolidBrush(COLORREF(0));
                for (x1, y1, x2, y2, radius, brush) in [
                    (1.0, 4.0, 13.0, 12.0, 3.0, white),
                    (2.5, 5.5, 11.5, 10.5, 1.0, black),
                    (13.5, 6.0, 15.0, 10.0, 1.5, white),
                ] {
                    let px = |v: f64| (v * side as f64 / 16.0).round() as i32;
                    let region =
                        CreateRoundRectRgn(px(x1), px(y1), px(x2), px(y2), px(radius), px(radius));
                    let _ = FillRgn(dc, region, brush);
                    let _ = DeleteObject(region.into());
                }
                let _ = DeleteObject(white.into());
                let _ = DeleteObject(black.into());
            }
            let _ = GdiFlush();
            let mut pixels = vec![0u8; (self.size * self.size * 4) as usize];
            let mask_stride = ((self.size + 15) / 16 * 2) as usize;
            let mut mask = vec![255u8; mask_stride * self.size as usize];
            for y in 0..self.size {
                for x in 0..self.size {
                    let mut coverage = 0u32;
                    for sy in 0..scale {
                        for sx in 0..scale {
                            coverage += samples
                                [(((y * scale + sy) * side + x * scale + sx) * 4) as usize]
                                as u32;
                        }
                    }
                    let alpha = (coverage / (scale * scale) as u32) as u8;
                    let index = ((y * self.size + x) * 4) as usize;
                    // DIB colors are BGRA and alpha is premultiplied.
                    for (channel, shift) in [16, 8, 0].into_iter().enumerate() {
                        pixels[index + channel] =
                            (((self.color.0 >> shift) & 255) * alpha as u32 / 255) as u8;
                    }
                    pixels[index + 3] = alpha;
                    if alpha > 0 {
                        mask[y as usize * mask_stride + x as usize / 8] &= !(0x80 >> (x % 8));
                    }
                }
            }
            let color = CreateBitmap(self.size, self.size, 1, 32, Some(pixels.as_ptr().cast()));
            let monochrome = CreateBitmap(self.size, self.size, 1, 1, Some(mask.as_ptr().cast()));
            let icon = if color.is_invalid() || monochrome.is_invalid() {
                Err(windows::core::Error::from_thread())
            } else {
                CreateIconIndirect(&ICONINFO {
                    fIcon: true.into(),
                    hbmMask: monochrome,
                    hbmColor: color,
                    ..Default::default()
                })
            };
            let _ = DeleteObject(color.into());
            let _ = DeleteObject(monochrome.into());
            icon
        })();
        SelectObject(dc, previous);
        let _ = DeleteDC(dc);
        let _ = DeleteObject(bitmap.into());
        result
    }
}
