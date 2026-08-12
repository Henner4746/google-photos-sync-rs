use super::ViewState;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CreateFontW, CreateRoundRectRgn, CreateSolidBrush,
    DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_RIGHT, DT_SINGLELINE,
    DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FW_NORMAL, FW_SEMIBOLD, FillRect, FillRgn, HDC,
    HFONT, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW;

const SURFACE: u32 = tone(6);
const SURFACE_CONTAINER: u32 = tone(10);
const SURFACE_HIGH: u32 = tone(18);
const OUTLINE: u32 = tone(24);
const TEXT_PRIMARY: u32 = tone(96);
const TEXT_SECONDARY: u32 = tone(68);
const TEXT_MUTED: u32 = tone(60);
const PRIMARY: u32 = tone(96);
const ON_PRIMARY: u32 = tone(4);

#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn paint(
    hwnd: HWND,
    view: &ViewState,
    paused: bool,
    syncing: bool,
    animation: u32,
    next_run: i64,
) {
    unsafe {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut paint);
        fill(
            dc,
            RECT {
                left: 0,
                top: 0,
                right: 484,
                bottom: 410,
            },
            SURFACE,
        );
        SetBkMode(dc, TRANSPARENT as i32);

        let fonts = Fonts::create();
        header(dc, paused, &fonts);
        status_panel(dc, view, paused, syncing, animation, &fonts);
        metrics(dc, view, &fonts);
        schedule_line(dc, view, paused, syncing, next_run, &fonts);
        fonts.destroy();
        EndPaint(hwnd, &paint);
    }
}

pub(super) unsafe fn paint_button(
    hwnd: HWND,
    is_primary: bool,
    pressed: bool,
    focused: bool,
    disabled: bool,
) {
    unsafe {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut paint);
        let rect = RECT {
            left: 0,
            top: 0,
            right: 210,
            bottom: 44,
        };
        SetBkMode(dc, TRANSPARENT as i32);
        let base = if is_primary { PRIMARY } else { SURFACE_HIGH };
        let color = if disabled {
            tone(28)
        } else if pressed {
            shade(base, if is_primary { -18 } else { 10 })
        } else {
            base
        };

        fill(dc, rect, SURFACE);
        if focused {
            rounded_fill(dc, rect, 22, tone(76));
            rounded_fill(dc, inset(rect, 2), 20, color);
        } else {
            rounded_fill(dc, rect, 22, color);
        }

        let mut buffer = [0_u16; 64];
        let length = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        let label = String::from_utf16_lossy(&buffer[..length.max(0) as usize]);
        let font = font(14, FW_SEMIBOLD);
        text(
            dc,
            &label,
            rect,
            if disabled {
                tone(58)
            } else if is_primary {
                ON_PRIMARY
            } else {
                TEXT_PRIMARY
            },
            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
            font,
        );
        DeleteObject(font);
        EndPaint(hwnd, &paint);
    }
}

struct Fonts {
    title: HFONT,
    heading: HFONT,
    body: HFONT,
    label: HFONT,
    metric: HFONT,
}

impl Fonts {
    unsafe fn create() -> Self {
        unsafe {
            Self {
                title: font(24, FW_SEMIBOLD),
                heading: font(18, FW_SEMIBOLD),
                body: font(15, FW_NORMAL),
                label: font(13, FW_NORMAL),
                metric: font(22, FW_SEMIBOLD),
            }
        }
    }

    unsafe fn destroy(&self) {
        unsafe {
            DeleteObject(self.title);
            DeleteObject(self.heading);
            DeleteObject(self.body);
            DeleteObject(self.label);
            DeleteObject(self.metric);
        }
    }
}

unsafe fn header(dc: HDC, paused: bool, fonts: &Fonts) {
    unsafe {
        text(
            dc,
            "Foto-Sicherung",
            RECT {
                left: 24,
                top: 16,
                right: 330,
                bottom: 50,
            },
            TEXT_PRIMARY,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.title,
        );
        text(
            dc,
            "Google Fotos  \u{00b7}  lokal indiziert",
            RECT {
                left: 24,
                top: 47,
                right: 330,
                bottom: 73,
            },
            TEXT_SECONDARY,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.body,
        );

        rounded_fill(
            dc,
            RECT {
                left: 344,
                top: 22,
                right: 460,
                bottom: 54,
            },
            16,
            SURFACE_HIGH,
        );
        rounded_fill(
            dc,
            RECT {
                left: 357,
                top: 35,
                right: 363,
                bottom: 41,
            },
            3,
            if paused { tone(62) } else { tone(92) },
        );
        text(
            dc,
            if paused {
                "Pausiert"
            } else {
                "Autostart aktiv"
            },
            RECT {
                left: 370,
                top: 22,
                right: 452,
                bottom: 54,
            },
            if paused { tone(76) } else { tone(90) },
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.label,
        );
    }
}

unsafe fn status_panel(
    dc: HDC,
    view: &ViewState,
    paused: bool,
    syncing: bool,
    animation: u32,
    fonts: &Fonts,
) {
    unsafe {
        rounded_fill(
            dc,
            RECT {
                left: 24,
                top: 86,
                right: 460,
                bottom: 187,
            },
            16,
            SURFACE_CONTAINER,
        );

        let needs_attention = view.status == "Pruefen";
        let status_color = if syncing {
            TEXT_PRIMARY
        } else if paused || needs_attention {
            tone(64)
        } else if view.pending == 0 {
            tone(90)
        } else {
            tone(80)
        };
        rounded_fill(
            dc,
            RECT {
                left: 40,
                top: 109,
                right: 50,
                bottom: 119,
            },
            5,
            status_color,
        );

        let headline = if syncing {
            "Sicherung l\u{00e4}uft"
        } else if paused {
            "Automatik pausiert"
        } else if needs_attention {
            "Aktion erforderlich"
        } else if view.pending == 0 {
            "Alles gesichert"
        } else {
            "Neue Dateien gefunden"
        };
        let detail = if syncing {
            "Dateien werden gepr\u{00fc}ft und parallel \u{00fc}bertragen"
        } else if paused {
            "Manuelle Sicherung bleibt jederzeit verf\u{00fc}gbar"
        } else if view.pending == 0 {
            "Keine neuen oder doppelten Medien"
        } else {
            view.detail.as_str()
        };
        text(
            dc,
            headline,
            RECT {
                left: 64,
                top: 98,
                right: 440,
                bottom: 128,
            },
            TEXT_PRIMARY,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.heading,
        );
        text(
            dc,
            detail,
            RECT {
                left: 40,
                top: 133,
                right: 440,
                bottom: 160,
            },
            TEXT_SECONDARY,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            fonts.body,
        );

        rounded_fill(
            dc,
            RECT {
                left: 40,
                top: 170,
                right: 444,
                bottom: 174,
            },
            2,
            OUTLINE,
        );
        if syncing {
            let left = 40 + ((animation as i32 * 17) % 284);
            rounded_fill(
                dc,
                RECT {
                    left,
                    top: 170,
                    right: left + 120,
                    bottom: 174,
                },
                2,
                status_color,
            );
        } else {
            let total = view.screenshots + view.clips + view.pending as i64;
            let completed = view.screenshots + view.clips;
            let right = if total == 0 {
                40
            } else {
                40 + (404_i64 * completed / total) as i32
            };
            rounded_fill(
                dc,
                RECT {
                    left: 40,
                    top: 170,
                    right: right.max(40),
                    bottom: 174,
                },
                2,
                status_color,
            );
        }
    }
}

unsafe fn metrics(dc: HDC, view: &ViewState, fonts: &Fonts) {
    unsafe {
        rounded_fill(
            dc,
            RECT {
                left: 24,
                top: 205,
                right: 460,
                bottom: 274,
            },
            12,
            SURFACE_CONTAINER,
        );
        fill(
            dc,
            RECT {
                left: 169,
                top: 218,
                right: 170,
                bottom: 261,
            },
            OUTLINE,
        );
        fill(
            dc,
            RECT {
                left: 314,
                top: 218,
                right: 315,
                bottom: 261,
            },
            OUTLINE,
        );
        metric(dc, "Screenshots", view.screenshots, 36, 157, fonts);
        metric(dc, "AMD-Clips", view.clips, 182, 302, fonts);
        metric(dc, "Noch offen", view.pending as i64, 327, 448, fonts);
    }
}

unsafe fn metric(dc: HDC, label: &str, value: i64, left: i32, right: i32, fonts: &Fonts) {
    unsafe {
        text(
            dc,
            label,
            RECT {
                left,
                top: 212,
                right,
                bottom: 235,
            },
            TEXT_MUTED,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.label,
        );
        text(
            dc,
            &value.to_string(),
            RECT {
                left,
                top: 235,
                right,
                bottom: 265,
            },
            TEXT_PRIMARY,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.metric,
        );
    }
}

unsafe fn schedule_line(
    dc: HDC,
    view: &ViewState,
    paused: bool,
    syncing: bool,
    next_run: i64,
    fonts: &Fonts,
) {
    unsafe {
        text(
            dc,
            &view.last_run,
            RECT {
                left: 24,
                top: 286,
                right: 270,
                bottom: 316,
            },
            TEXT_MUTED,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            fonts.label,
        );
        let next_label = if paused {
            "Automatik angehalten".to_owned()
        } else if syncing {
            "\u{00dc}bertragung aktiv".to_owned()
        } else {
            format!(
                "N\u{00e4}chste Pr\u{00fc}fung in {}",
                countdown(next_run - crate::unix_seconds())
            )
        };
        text(
            dc,
            &next_label,
            RECT {
                left: 270,
                top: 286,
                right: 460,
                bottom: 316,
            },
            TEXT_MUTED,
            DT_RIGHT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            fonts.label,
        );
    }
}

unsafe fn fill(dc: HDC, rect: RECT, color: u32) {
    let brush = unsafe { CreateSolidBrush(color) };
    unsafe {
        FillRect(dc, &rect, brush);
        DeleteObject(brush);
    }
}

unsafe fn rounded_fill(dc: HDC, rect: RECT, radius: i32, color: u32) {
    let region = unsafe {
        CreateRoundRectRgn(
            rect.left,
            rect.top,
            rect.right + 1,
            rect.bottom + 1,
            radius * 2,
            radius * 2,
        )
    };
    let brush = unsafe { CreateSolidBrush(color) };
    unsafe {
        FillRgn(dc, region, brush);
        DeleteObject(brush);
        DeleteObject(region);
    }
}

unsafe fn text(dc: HDC, value: &str, mut rect: RECT, color: u32, flags: u32, font: HFONT) {
    let value = wide(value);
    unsafe {
        let previous = SelectObject(dc, font);
        SetTextColor(dc, color);
        DrawTextW(dc, value.as_ptr(), -1, &mut rect, flags);
        SelectObject(dc, previous);
    }
}

unsafe fn font(height: i32, weight: u32) -> HFONT {
    let face = wide("Segoe UI Variable Text");
    unsafe {
        CreateFontW(
            -height,
            0,
            0,
            0,
            weight as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            0,
            0,
            CLEARTYPE_QUALITY as u32,
            DEFAULT_PITCH as u32,
            face.as_ptr(),
        )
    }
}

fn countdown(seconds: i64) -> String {
    let seconds = seconds.max(0);
    if seconds < 60 {
        format!("{} Sek.", seconds)
    } else {
        format!("{} Min.", (seconds + 59) / 60)
    }
}

fn inset(mut rect: RECT, amount: i32) -> RECT {
    rect.left += amount;
    rect.top += amount;
    rect.right -= amount;
    rect.bottom -= amount;
    rect
}

fn shade(color: u32, amount: i32) -> u32 {
    let channel = |shift: u32| ((color >> shift) & 0xff) as i32;
    rgb(
        (channel(0) + amount).clamp(0, 255) as u32,
        (channel(8) + amount).clamp(0, 255) as u32,
        (channel(16) + amount).clamp(0, 255) as u32,
    )
}

const fn tone(value: u32) -> u32 {
    rgb(value * 255 / 100, value * 255 / 100, value * 255 / 100)
}

const fn rgb(red: u32, green: u32, blue: u32) -> u32 {
    red | (green << 8) | (blue << 16)
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ON_PRIMARY, PRIMARY, SURFACE, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY};

    #[test]
    fn text_contrast_meets_wcag_aa() {
        assert!(contrast(TEXT_PRIMARY, SURFACE) >= 4.5);
        assert!(contrast(TEXT_SECONDARY, SURFACE) >= 4.5);
        assert!(contrast(TEXT_MUTED, SURFACE) >= 4.5);
        assert!(contrast(ON_PRIMARY, PRIMARY) >= 4.5);
    }

    fn contrast(first: u32, second: u32) -> f64 {
        let first = relative_luminance(first);
        let second = relative_luminance(second);
        (first.max(second) + 0.05) / (first.min(second) + 0.05)
    }

    fn relative_luminance(color: u32) -> f64 {
        let channel = (color & 0xff) as f64 / 255.0;
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    }
}
