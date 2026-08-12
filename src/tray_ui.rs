use super::{
    CMD_CLOSE, CMD_OPEN_SOURCE, CMD_REMOVE_SOURCE, CMD_SAVE_ALBUM, CMD_SYNC, CMD_TOGGLE_SOURCE,
    MediaKind, SourceSpec, ViewState,
};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CreateFontW, CreateRoundRectRgn, CreateSolidBrush,
    DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_RIGHT, DT_SINGLELINE,
    DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FW_NORMAL, FW_SEMIBOLD, FillRect, FillRgn,
    FrameRect, HDC, HFONT, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::UI::Controls::{DRAWITEMSTRUCT, ODS_FOCUS, ODS_SELECTED};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowTextW};

const SURFACE: u32 = tone(5);
const SURFACE_CONTAINER: u32 = tone(10);
const SURFACE_HIGH: u32 = tone(17);
const SURFACE_HOVER: u32 = tone(23);
const OUTLINE: u32 = tone(28);
const TEXT_PRIMARY: u32 = tone(96);
const TEXT_SECONDARY: u32 = tone(72);
const TEXT_MUTED: u32 = tone(62);
const PRIMARY: u32 = tone(96);
const ON_PRIMARY: u32 = tone(4);

#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn paint(
    hwnd: HWND,
    view: &ViewState,
    paused: bool,
    syncing: bool,
    dry_run: bool,
    has_selection: bool,
    animation: u32,
    next_run: i64,
    setup: bool,
    google_connected: bool,
    has_folder: bool,
    takeout_imported: bool,
    autostart: bool,
    settings: bool,
) {
    unsafe {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut paint);
        fill(dc, rect(0, 0, 720, 656), SURFACE);
        SetBkMode(dc, TRANSPARENT as i32);
        let fonts = Fonts::create();
        if settings {
            settings_page(dc, view, autostart, &fonts);
        } else if setup {
            setup_page(
                dc,
                view,
                google_connected,
                has_folder,
                takeout_imported,
                autostart,
                &fonts,
            );
        } else {
            header(dc, paused, &fonts);
            status_panel(
                dc, view, paused, syncing, dry_run, animation, next_run, &fonts,
            );
            metrics(dc, view, &fonts);
            sources_panel(dc, view, has_selection, &fonts);
        }
        fonts.destroy();
        EndPaint(hwnd, &paint);
    }
}

unsafe fn settings_page(dc: HDC, view: &ViewState, autostart: bool, fonts: &Fonts) {
    unsafe {
        text(
            dc,
            "Einstellungen",
            rect(40, 30, 600, 72),
            TEXT_PRIMARY,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.title,
        );
        text(
            dc,
            "Leichtgewichtig, lokal und pro Ordner steuerbar",
            rect(40, 72, 680, 100),
            TEXT_SECONDARY,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.body,
        );
        text(
            dc,
            "Ausgewählter Ordner",
            rect(40, 128, 680, 156),
            TEXT_MUTED,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.label,
        );
        text(
            dc,
            "Lokale App-Daten",
            rect(40, 294, 680, 322),
            TEXT_MUTED,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.label,
        );
        text(
            dc,
            "Datensicherung enthält Einstellungen, Duplikat-Datenbank und den Windows-geschützten Google-Zugang.",
            rect(40, 382, 680, 414),
            TEXT_SECONDARY,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            fonts.label,
        );
        text(
            dc,
            if autostart {
                "Autostart ist aktiv"
            } else {
                "Autostart ist aus"
            },
            rect(40, 414, 680, 438),
            TEXT_MUTED,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.label,
        );
        text(
            dc,
            "Google-Zugriff und Aktualisierungen",
            rect(40, 494, 680, 518),
            TEXT_MUTED,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.label,
        );
        text(
            dc,
            &view.detail,
            rect(40, 590, 680, 628),
            TEXT_MUTED,
            DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            fonts.label,
        );
    }
}

pub(super) unsafe fn paint_button(
    hwnd: HWND,
    command: usize,
    hovered: bool,
    pressed: bool,
    focused: bool,
    disabled: bool,
) {
    unsafe {
        let mut paint = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut paint);
        let mut bounds = RECT::default();
        GetClientRect(hwnd, &mut bounds);
        SetBkMode(dc, TRANSPARENT as i32);

        let detail_button = matches!(
            command,
            CMD_SAVE_ALBUM | CMD_OPEN_SOURCE | CMD_TOGGLE_SOURCE | CMD_REMOVE_SOURCE
        );
        fill(
            dc,
            bounds,
            if detail_button {
                SURFACE_CONTAINER
            } else {
                SURFACE
            },
        );
        let primary = matches!(command, CMD_SYNC | super::CMD_SETUP_FINISH);
        let bare = command == CMD_CLOSE;
        let base = if primary { PRIMARY } else { SURFACE_HIGH };
        let button_color = if disabled {
            tone(25)
        } else if pressed {
            if primary { tone(78) } else { tone(30) }
        } else if hovered {
            if primary { tone(88) } else { SURFACE_HOVER }
        } else {
            base
        };

        if !bare {
            if focused {
                rounded_fill(dc, bounds, bounds.bottom / 2, tone(76));
                rounded_fill(dc, inset(bounds, 2), (bounds.bottom - 4) / 2, button_color);
            } else {
                rounded_fill(dc, bounds, bounds.bottom / 2, button_color);
            }
        } else if hovered || focused {
            rounded_fill(dc, bounds, bounds.bottom / 2, SURFACE_HIGH);
        }

        let mut buffer = [0_u16; 96];
        let length = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        let label = String::from_utf16_lossy(&buffer[..length.max(0) as usize]);
        let label_font = font(if bare { 21 } else { 14 }, FW_SEMIBOLD);
        text(
            dc,
            &label,
            bounds,
            if disabled {
                tone(55)
            } else if primary {
                ON_PRIMARY
            } else {
                TEXT_PRIMARY
            },
            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
            label_font,
        );
        DeleteObject(label_font);
        EndPaint(hwnd, &paint);
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn setup_page(
    dc: HDC,
    view: &ViewState,
    google_connected: bool,
    has_folder: bool,
    takeout_imported: bool,
    autostart: bool,
    fonts: &Fonts,
) {
    unsafe {
        text(
            dc,
            "Google Photos Sync einrichten",
            rect(40, 30, 680, 72),
            TEXT_PRIMARY,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.title,
        );
        text(
            dc,
            "Alles Wichtige direkt in der App · keine Konsole",
            rect(40, 72, 680, 100),
            TEXT_SECONDARY,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.body,
        );
        rounded_fill(dc, rect(40, 116, 680, 218), 16, SURFACE_CONTAINER);
        text(
            dc,
            "Google-Zugriff vor dem Verbinden",
            rect(60, 126, 660, 152),
            TEXT_PRIMARY,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.heading,
        );
        text(
            dc,
            "Neue Medien aus gewählten Ordnern gehen direkt an dein Google-Fotos-Konto.",
            rect(60, 152, 660, 174),
            TEXT_SECONDARY,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            fonts.label,
        );
        text(
            dc,
            "Seit März 2025 liest die App nur selbst hochgeladene Inhalte; andere bleiben unsichtbar.",
            rect(60, 174, 660, 196),
            TEXT_MUTED,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            fonts.label,
        );
        text(
            dc,
            "Zugang nur Windows-verschlüsselt auf diesem PC · kein eigener Server.",
            rect(60, 196, 660, 216),
            TEXT_MUTED,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            fonts.label,
        );
        text(
            dc,
            "Mit dem nächsten Klick stimmst du genau diesem Zugriff zu.",
            rect(40, 218, 680, 240),
            TEXT_SECONDARY,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
            fonts.label,
        );
        for (top, done) in [
            (242, google_connected),
            (306, has_folder),
            (370, takeout_imported),
            (434, autostart),
        ] {
            rounded_fill(
                dc,
                rect(58, top + 19, 68, top + 29),
                5,
                if done { TEXT_PRIMARY } else { TEXT_MUTED },
            );
        }
        text(
            dc,
            if view.status == "Bereit" {
                ""
            } else {
                &view.detail
            },
            rect(40, 486, 680, 516),
            TEXT_SECONDARY,
            DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            fonts.label,
        );
        text(
            dc,
            "Takeout wird nur lokal gelesen. Hochgeladen werden ausschließlich neue Inhalte.",
            rect(40, 590, 680, 628),
            TEXT_MUTED,
            DT_CENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            fonts.label,
        );
    }
}

pub(super) unsafe fn paint_source_item(item: &DRAWITEMSTRUCT, source: Option<&SourceSpec>) {
    let Some(source) = source else { return };
    unsafe {
        let selected = item.itemState & ODS_SELECTED != 0;
        let focused = item.itemState & ODS_FOCUS != 0;
        let bounds = item.rcItem;
        fill(
            item.hDC,
            bounds,
            if selected {
                SURFACE_HIGH
            } else {
                SURFACE_CONTAINER
            },
        );
        if focused {
            let brush = CreateSolidBrush(tone(72));
            FrameRect(item.hDC, &bounds, brush);
            DeleteObject(brush);
        }
        let title_font = font(15, FW_SEMIBOLD);
        let detail_font = font(12, FW_NORMAL);
        text(
            item.hDC,
            &source.album,
            rect(
                bounds.left + 14,
                bounds.top + 6,
                bounds.right - 136,
                bounds.top + 29,
            ),
            if source.enabled {
                TEXT_PRIMARY
            } else {
                TEXT_SECONDARY
            },
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            title_font,
        );
        text(
            item.hDC,
            &source.path.to_string_lossy(),
            rect(
                bounds.left + 14,
                bounds.top + 29,
                bounds.right - 14,
                bounds.top + 53,
            ),
            TEXT_MUTED,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            detail_font,
        );
        let kind = match source.kind {
            MediaKind::Images => "Bilder",
            MediaKind::Videos => "Videos",
            MediaKind::All => "Alle Medien",
        };
        text(
            item.hDC,
            &format!(
                "{}  ·  {kind}  ·  {} Min.",
                if source.enabled { "Aktiv" } else { "Pausiert" },
                source.schedule_minutes,
            ),
            rect(
                bounds.right - 142,
                bounds.top + 7,
                bounds.right - 14,
                bounds.top + 29,
            ),
            if source.enabled {
                TEXT_SECONDARY
            } else {
                TEXT_MUTED
            },
            DT_RIGHT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            detail_font,
        );
        DeleteObject(title_font);
        DeleteObject(detail_font);
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
                title: font(25, FW_SEMIBOLD),
                heading: font(18, FW_SEMIBOLD),
                body: font(14, FW_NORMAL),
                label: font(12, FW_NORMAL),
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
            "Google Photos Sync",
            rect(24, 14, 440, 46),
            TEXT_PRIMARY,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.title,
        );
        text(
            dc,
            "Sichert Ordner ohne doppelte Uploads",
            rect(24, 46, 490, 70),
            TEXT_SECONDARY,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.body,
        );
        rounded_fill(dc, rect(540, 21, 655, 53), 16, SURFACE_HIGH);
        rounded_fill(
            dc,
            rect(552, 34, 558, 40),
            3,
            if paused { tone(60) } else { tone(94) },
        );
        text(
            dc,
            if paused { "Pausiert" } else { "Autostart" },
            rect(566, 21, 647, 53),
            if paused { TEXT_SECONDARY } else { TEXT_PRIMARY },
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.label,
        );
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn status_panel(
    dc: HDC,
    view: &ViewState,
    paused: bool,
    syncing: bool,
    dry_run: bool,
    animation: u32,
    next_run: i64,
    fonts: &Fonts,
) {
    unsafe {
        rounded_fill(dc, rect(24, 86, 696, 190), 16, SURFACE_CONTAINER);
        let needs_attention = view.status == "Pruefen";
        let headline = if syncing && dry_run {
            "Testlauf prüft deine Ordner"
        } else if syncing {
            "Sicherung läuft"
        } else if paused {
            "Automatik pausiert"
        } else if needs_attention {
            "Aktion erforderlich"
        } else if view.pending == 0 {
            "Alles gesichert"
        } else {
            "Neue Medien gefunden"
        };
        let detail = if syncing && dry_run {
            "Es wird nichts hochgeladen oder in Google Fotos geändert"
        } else if syncing {
            "Nur neue Inhalte werden zu Google Fotos übertragen"
        } else if paused {
            "Manuelle Sicherungen und Testläufe bleiben verfügbar"
        } else if view.pending == 0 {
            "Vorhandene Bilder und Videos werden nicht erneut hochgeladen"
        } else {
            view.detail.as_str()
        };
        rounded_fill(
            dc,
            rect(40, 108, 50, 118),
            5,
            if paused || needs_attention {
                tone(64)
            } else {
                tone(94)
            },
        );
        text(
            dc,
            headline,
            rect(64, 97, 680, 128),
            TEXT_PRIMARY,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.heading,
        );
        text(
            dc,
            detail,
            rect(40, 130, 680, 153),
            TEXT_SECONDARY,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            fonts.body,
        );
        let next = if paused {
            "Automatik angehalten".to_owned()
        } else if syncing {
            if dry_run {
                "Schreibgeschützt"
            } else {
                "Übertragung aktiv"
            }
            .to_owned()
        } else {
            format!(
                "Nächste Prüfung in {}",
                countdown(next_run - crate::unix_seconds())
            )
        };
        text(
            dc,
            &view.last_run,
            rect(40, 153, 380, 176),
            TEXT_MUTED,
            DT_LEFT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            fonts.label,
        );
        text(
            dc,
            &next,
            rect(390, 153, 680, 176),
            TEXT_MUTED,
            DT_RIGHT | DT_SINGLELINE | DT_END_ELLIPSIS | DT_VCENTER,
            fonts.label,
        );
        rounded_fill(dc, rect(40, 178, 680, 182), 2, OUTLINE);
        if syncing && view.progress_total > 0 {
            let ratio = (view.progress_files.min(view.progress_total) as f64
                / view.progress_total as f64)
                .clamp(0.0, 1.0);
            let right = 40 + (640.0 * ratio) as i32;
            rounded_fill(dc, rect(40, 178, right.max(40), 182), 2, TEXT_PRIMARY);
        } else if syncing {
            let left = 40 + ((animation as i32 * 19) % 500);
            rounded_fill(dc, rect(left, 178, left + 140, 182), 2, TEXT_PRIMARY);
        } else {
            let total = view.protected + view.pending as i64;
            let right = if total == 0 {
                40
            } else {
                40 + (640_i64 * view.protected / total) as i32
            };
            rounded_fill(dc, rect(40, 178, right.max(40), 182), 2, TEXT_PRIMARY);
        }
    }
}

unsafe fn metrics(dc: HDC, view: &ViewState, fonts: &Fonts) {
    unsafe {
        rounded_fill(dc, rect(24, 206, 696, 274), 12, SURFACE_CONTAINER);
        fill(dc, rect(247, 219, 248, 261), OUTLINE);
        fill(dc, rect(471, 219, 472, 261), OUTLINE);
        metric(dc, "Gesichert", view.protected, 40, 228, fonts);
        metric(dc, "Ordner", view.folders as i64, 264, 452, fonts);
        metric(dc, "Noch offen", view.pending as i64, 488, 680, fonts);
    }
}

unsafe fn metric(dc: HDC, label: &str, value: i64, left: i32, right: i32, fonts: &Fonts) {
    unsafe {
        text(
            dc,
            label,
            rect(left, 211, right, 234),
            TEXT_MUTED,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.label,
        );
        text(
            dc,
            &value.to_string(),
            rect(left, 234, right, 266),
            TEXT_PRIMARY,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.metric,
        );
    }
}

unsafe fn sources_panel(dc: HDC, view: &ViewState, has_selection: bool, fonts: &Fonts) {
    unsafe {
        text(
            dc,
            "Gesicherte Ordner",
            rect(24, 294, 400, 334),
            TEXT_PRIMARY,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            fonts.heading,
        );
        rounded_fill(dc, rect(24, 342, 696, 476), 12, SURFACE_CONTAINER);
        if view.folders == 0 {
            text(
                dc,
                "Noch keine Ordner",
                rect(48, 365, 672, 398),
                TEXT_PRIMARY,
                DT_CENTER | DT_SINGLELINE | DT_VCENTER,
                fonts.heading,
            );
            text(
                dc,
                "Füge einen Ordner mit Bildern oder Videos hinzu.",
                rect(48, 399, 672, 430),
                TEXT_SECONDARY,
                DT_CENTER | DT_SINGLELINE | DT_VCENTER,
                fonts.body,
            );
        }
        if has_selection {
            rounded_fill(dc, rect(24, 492, 696, 568), 12, SURFACE_CONTAINER);
            text(
                dc,
                "Zielalbum für neue Medien",
                rect(40, 495, 300, 518),
                TEXT_MUTED,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER,
                fonts.label,
            );
        } else {
            text(
                dc,
                "Wähle einen Ordner aus, um Zielalbum und Status zu ändern.",
                rect(24, 492, 696, 568),
                TEXT_MUTED,
                DT_CENTER | DT_SINGLELINE | DT_VCENTER,
                fonts.body,
            );
        }
    }
}

unsafe fn fill(dc: HDC, area: RECT, color: u32) {
    let brush = unsafe { CreateSolidBrush(color) };
    unsafe {
        FillRect(dc, &area, brush);
        DeleteObject(brush);
    }
}

unsafe fn rounded_fill(dc: HDC, area: RECT, radius: i32, color: u32) {
    let region = unsafe {
        CreateRoundRectRgn(
            area.left,
            area.top,
            area.right + 1,
            area.bottom + 1,
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

unsafe fn text(dc: HDC, value: &str, mut area: RECT, color: u32, flags: u32, selected_font: HFONT) {
    let value = wide(value);
    unsafe {
        let previous = SelectObject(dc, selected_font);
        SetTextColor(dc, color);
        DrawTextW(dc, value.as_ptr(), -1, &mut area, flags);
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
        format!("{seconds} Sek.")
    } else {
        format!("{} Min.", (seconds + 59) / 60)
    }
}

const fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left,
        top,
        right,
        bottom,
    }
}

const fn inset(area: RECT, amount: i32) -> RECT {
    rect(
        area.left + amount,
        area.top + amount,
        area.right - amount,
        area.bottom - amount,
    )
}

const fn tone(value: u32) -> u32 {
    let channel = value * 255 / 100;
    channel | (channel << 8) | (channel << 16)
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
