use super::{AppPaths, Logger, SingleInstance, open_database, source_files, sync, unix_seconds};
use rusqlite::params;
use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, CreateRoundRectRgn,
    DIB_RGB_COLORS, DeleteObject, GetMonitorInfoW, InvalidateRect, MONITOR_DEFAULTTONEAREST,
    MONITORINFO, MonitorFromPoint, SetWindowRgn, UpdateWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, IsWindowEnabled, SetFocus, VK_ESCAPE,
};
use windows_sys::Win32::UI::Shell::{
    DefSubclassProc, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    NOTIFYICONIDENTIFIER, RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconGetRect,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BM_GETSTATE, BN_CLICKED, BS_DEFPUSHBUTTON, BS_NOTIFY, BS_PUSHBUTTON, BST_PUSHED,
    CS_HREDRAW, CS_VREDRAW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetDlgCtrlID,
    GetDlgItem, GetMessageW, HWND_TOPMOST, ICONINFO, IDC_ARROW, IsDialogMessageW, KillTimer,
    LoadCursorW, MF_SEPARATOR, MF_STRING, MSG, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SW_HIDE, SWP_SHOWWINDOW, SendMessageW, SetForegroundWindow, SetTimer,
    SetWindowPos, SetWindowTextW, ShowWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    TranslateMessage, WA_INACTIVE, WM_ACTIVATE, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY,
    WM_ENABLE, WM_ERASEBKGND, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_NCDESTROY, WM_PAINT, WM_RBUTTONUP, WM_SETFOCUS, WM_TIMER, WNDCLASSW, WS_CHILD,
    WS_EX_CONTROLPARENT, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_TABSTOP, WS_VISIBLE,
};

#[path = "tray_ui.rs"]
mod ui;

const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;
const TIMER_SCHEDULE: usize = 1;
const TIMER_INITIAL: usize = 2;
const TIMER_ANIMATION: usize = 3;
const CMD_OPEN: usize = 1001;
const CMD_SYNC: usize = 1002;
const CMD_EXIT: usize = 1003;
const CMD_PAUSE: usize = 1004;
const WINDOW_WIDTH: i32 = 484;
const WINDOW_HEIGHT: i32 = 410;

const BUTTON_SYNC: RECT = RECT {
    left: 24,
    top: 338,
    right: 234,
    bottom: 382,
};
const BUTTON_PAUSE: RECT = RECT {
    left: 250,
    top: 338,
    right: 460,
    bottom: 382,
};

static STATE: OnceLock<Arc<TrayState>> = OnceLock::new();
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static FLYOUT_WAS_ACTIVE: AtomicBool = AtomicBool::new(false);
static APP_ICON: AtomicIsize = AtomicIsize::new(0);

#[derive(Clone)]
struct ViewState {
    status: String,
    detail: String,
    last_run: String,
    screenshots: i64,
    clips: i64,
    pending: usize,
}

struct TrayState {
    paths: AppPaths,
    logger: Arc<Logger>,
    view: Mutex<ViewState>,
    syncing: AtomicBool,
    paused: AtomicBool,
    next_run: AtomicI64,
    hwnd: AtomicIsize,
    animation: AtomicU32,
}

pub(super) fn run(
    paths: AppPaths,
    logger: Logger,
    show_on_start: bool,
    sync_on_start: bool,
) -> super::AppResult<()> {
    let _tray_instance = SingleInstance::acquire_tray()?;
    let (screenshots, clips, pending) = counts(&paths).unwrap_or((0, 0, 0));
    let state = Arc::new(TrayState {
        paths,
        logger: Arc::new(logger),
        view: Mutex::new(ViewState {
            status: "Bereit".to_owned(),
            detail: "Keine doppelten Medien · 4 parallele Uploads".to_owned(),
            last_run: "Noch kein Lauf in dieser Sitzung".to_owned(),
            screenshots,
            clips,
            pending,
        }),
        syncing: AtomicBool::new(false),
        paused: AtomicBool::new(false),
        next_run: AtomicI64::new(unix_seconds() + 2),
        hwnd: AtomicIsize::new(0),
        animation: AtomicU32::new(0),
    });
    STATE
        .set(state)
        .map_err(|_| "Der Infobereich wurde bereits initialisiert.")?;
    win32_loop(show_on_start, sync_on_start)
}

fn win32_loop(show_on_start: bool, sync_on_start: bool) -> super::AppResult<()> {
    unsafe {
        let instance = GetModuleHandleW(null());
        if instance.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        let class_name = wide("GooglePhotosSyncWindow");
        let taskbar_message = wide("TaskbarCreated");
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(taskbar_message.as_ptr()),
            Ordering::Release,
        );
        let app_icon = create_app_icon();
        APP_ICON.store(app_icon as isize, Ordering::Release);
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hIcon: app_icon,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        };
        if RegisterClassW(&window_class) == 0 {
            return Err(io::Error::last_os_error().into());
        }
        let title = wide("Foto-Sicherung");
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_CONTROLPARENT,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            0,
            0,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        STATE
            .get()
            .expect("tray state")
            .hwnd
            .store(hwnd as isize, Ordering::Release);
        let button_class = wide("BUTTON");
        let sync_label = wide("Jetzt sichern");
        let pause_label = wide("Pausieren");
        let sync_button = CreateWindowExW(
            0,
            button_class.as_ptr(),
            sync_label.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON as u32 | BS_NOTIFY as u32,
            BUTTON_SYNC.left,
            BUTTON_SYNC.top,
            BUTTON_SYNC.right - BUTTON_SYNC.left,
            BUTTON_SYNC.bottom - BUTTON_SYNC.top,
            hwnd,
            CMD_SYNC as _,
            instance,
            null(),
        );
        let pause_button = CreateWindowExW(
            0,
            button_class.as_ptr(),
            pause_label.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32 | BS_NOTIFY as u32,
            BUTTON_PAUSE.left,
            BUTTON_PAUSE.top,
            BUTTON_PAUSE.right - BUTTON_PAUSE.left,
            BUTTON_PAUSE.bottom - BUTTON_PAUSE.top,
            hwnd,
            CMD_PAUSE as _,
            instance,
            null(),
        );
        if sync_button.is_null() || pause_button.is_null() {
            DestroyWindow(hwnd);
            return Err(io::Error::last_os_error().into());
        }
        SetWindowSubclass(sync_button, Some(button_proc), CMD_SYNC, 0);
        SetWindowSubclass(pause_button, Some(button_proc), CMD_PAUSE, 0);
        let corner = CreateRoundRectRgn(0, 0, WINDOW_WIDTH + 1, WINDOW_HEIGHT + 1, 28, 28);
        SetWindowRgn(hwnd, corner, 1);
        add_tray_icon(hwnd)?;
        SetTimer(hwnd, TIMER_SCHEDULE, 30_000, None);
        SetTimer(hwnd, TIMER_ANIMATION, 120, None);
        if sync_on_start {
            SetTimer(hwnd, TIMER_INITIAL, 1_000, None);
        }
        if show_on_start {
            show_dashboard(hwnd);
        }

        let mut message = MSG::default();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            if message.message == WM_KEYDOWN && message.wParam == VK_ESCAPE as usize {
                ShowWindow(hwnd, SW_HIDE);
                continue;
            }
            if IsDialogMessageW(hwnd, &message) != 0 {
                continue;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        Ok(())
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == TASKBAR_CREATED.load(Ordering::Acquire) {
        let _ = add_tray_icon(hwnd);
        return 0;
    }
    match message {
        WM_TRAY => {
            let event = lparam as u32;
            if event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK {
                show_dashboard(hwnd);
            } else if event == WM_RBUTTONUP {
                tray_menu(hwnd);
            }
            0
        }
        WM_PAINT => {
            paint_dashboard(hwnd);
            0
        }
        WM_ERASEBKGND => 1,
        WM_ACTIVATE if wparam as u32 & 0xffff == WA_INACTIVE => {
            if FLYOUT_WAS_ACTIVE.swap(false, Ordering::AcqRel) {
                unsafe { ShowWindow(hwnd, SW_HIDE) };
            }
            0
        }
        WM_ACTIVATE => {
            FLYOUT_WAS_ACTIVE.store(true, Ordering::Release);
            0
        }
        WM_TIMER => {
            if wparam == TIMER_INITIAL {
                unsafe { KillTimer(hwnd, TIMER_INITIAL) };
                request_sync(false);
            } else if wparam == TIMER_SCHEDULE {
                let state = STATE.get().expect("tray state");
                if unix_seconds() >= state.next_run.load(Ordering::Acquire) {
                    request_sync(false);
                }
                invalidate();
            } else if wparam == TIMER_ANIMATION
                && STATE
                    .get()
                    .expect("tray state")
                    .syncing
                    .load(Ordering::Acquire)
            {
                STATE
                    .get()
                    .expect("tray state")
                    .animation
                    .fetch_add(1, Ordering::Relaxed);
                invalidate();
            }
            0
        }
        WM_COMMAND => {
            let command = wparam & 0xffff;
            let notification = (wparam >> 16) as u32;
            if notification == BN_CLICKED || command == CMD_OPEN || command == CMD_EXIT {
                handle_command(hwnd, command);
            }
            0
        }
        WM_CLOSE => {
            unsafe { ShowWindow(hwnd, SW_HIDE) };
            0
        }
        WM_DESTROY => {
            remove_tray_icon(hwnd);
            let icon = APP_ICON.swap(0, Ordering::AcqRel);
            if icon != 0 {
                unsafe { DestroyIcon(icon as _) };
            }
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe extern "system" fn button_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    match message {
        WM_PAINT => {
            let state = STATE.get().expect("tray state");
            let pressed = unsafe { SendMessageW(hwnd, BM_GETSTATE, 0, 0) } as u32 & BST_PUSHED != 0;
            unsafe {
                ui::paint_button(
                    hwnd,
                    GetDlgCtrlID(hwnd) as usize == CMD_SYNC,
                    pressed,
                    GetFocus() == hwnd,
                    IsWindowEnabled(hwnd) == 0,
                )
            };
            let _ = state;
            0
        }
        WM_ERASEBKGND => 1,
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_SETFOCUS | WM_KILLFOCUS | WM_ENABLE => {
            let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
            unsafe { InvalidateRect(hwnd, null(), 0) };
            result
        }
        WM_NCDESTROY => {
            unsafe { RemoveWindowSubclass(hwnd, Some(button_proc), subclass_id) };
            unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
        }
        _ => unsafe { DefSubclassProc(hwnd, message, wparam, lparam) },
    }
}

fn add_tray_icon(hwnd: HWND) -> super::AppResult<()> {
    unsafe {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: APP_ICON.load(Ordering::Acquire) as _,
            ..Default::default()
        };
        copy_wide(&mut data.szTip, "Google Fotos Sync · bereit");
        if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
            return Err(io::Error::last_os_error().into());
        }
    }
    Ok(())
}

fn remove_tray_icon(hwnd: HWND) {
    unsafe {
        let data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            ..Default::default()
        };
        Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

fn create_app_icon() -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 32,
                biHeight: -32,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut raw = null_mut();
        let color = CreateDIBSection(null_mut(), &info, DIB_RGB_COLORS, &mut raw, null_mut(), 0);
        if color.is_null() || raw.is_null() {
            return null_mut();
        }
        let pixels = std::slice::from_raw_parts_mut(raw.cast::<u32>(), 32 * 32);
        for y in 0_i32..32 {
            for x in 0_i32..32 {
                let dx = x - 16;
                let dy = y - 16;
                let distance = dx * dx + dy * dy;
                let outline = (62..=142).contains(&distance);
                let ring = (76..=126).contains(&distance);
                let forward_arrow = (20..=27).contains(&x) && (6..=13).contains(&y) && x + y >= 31;
                let back_arrow = (5..=12).contains(&x) && (19..=26).contains(&y) && x + y <= 31;
                let pixel = if ring || forward_arrow || back_arrow {
                    0xff_f2_f2_f2
                } else if outline {
                    0xff_22_22_22
                } else {
                    0
                };
                pixels[(y * 32 + x) as usize] = pixel;
            }
        }
        let mask = CreateBitmap(32, 32, 1, 1, null());
        let icon_info = ICONINFO {
            fIcon: 1,
            hbmColor: color,
            hbmMask: mask,
            ..Default::default()
        };
        let icon = CreateIconIndirect(&icon_info);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}

fn show_dashboard(hwnd: HWND) {
    unsafe {
        let identifier = NOTIFYICONIDENTIFIER {
            cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            ..Default::default()
        };
        let mut icon = RECT::default();
        if Shell_NotifyIconGetRect(&identifier, &mut icon) < 0 {
            let mut cursor = POINT::default();
            GetCursorPos(&mut cursor);
            icon = RECT {
                left: cursor.x,
                top: cursor.y,
                right: cursor.x + 1,
                bottom: cursor.y + 1,
            };
        }
        let monitor = MonitorFromPoint(
            POINT {
                x: icon.left,
                y: icon.top,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        GetMonitorInfoW(monitor, &mut info);
        let margin = 10;
        let x = (icon.right - WINDOW_WIDTH).clamp(
            info.rcWork.left + margin,
            info.rcWork.right - WINDOW_WIDTH - margin,
        );
        let y = if icon.top >= info.rcWork.bottom {
            info.rcWork.bottom - WINDOW_HEIGHT - margin
        } else if icon.bottom <= info.rcWork.top {
            info.rcWork.top + margin
        } else {
            (icon.bottom - WINDOW_HEIGHT).clamp(
                info.rcWork.top + margin,
                info.rcWork.bottom - WINDOW_HEIGHT - margin,
            )
        };
        refresh_controls();
        FLYOUT_WAS_ACTIVE.store(false, Ordering::Release);
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            SWP_SHOWWINDOW,
        );
        SetForegroundWindow(hwnd);
        SetFocus(GetDlgItem(hwnd, CMD_SYNC as i32));
        InvalidateRect(hwnd, null(), 0);
        UpdateWindow(hwnd);
    }
}

fn tray_menu(hwnd: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        let open = wide("Dashboard oeffnen");
        let sync = wide("Jetzt synchronisieren");
        let exit = wide("Beenden");
        AppendMenuW(menu, MF_STRING, CMD_OPEN, open.as_ptr());
        AppendMenuW(menu, MF_STRING, CMD_SYNC, sync.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        AppendMenuW(menu, MF_STRING, CMD_EXIT, exit.as_ptr());
        let mut point = POINT::default();
        GetCursorPos(&mut point);
        SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            null(),
        ) as usize;
        DestroyMenu(menu);
        handle_command(hwnd, command);
    }
}

fn handle_command(hwnd: HWND, command: usize) {
    match command {
        CMD_OPEN => show_dashboard(hwnd),
        CMD_SYNC => request_sync(true),
        CMD_PAUSE => toggle_pause(),
        CMD_EXIT => unsafe {
            DestroyWindow(hwnd);
        },
        _ => {}
    }
}

fn toggle_pause() {
    let state = STATE.get().expect("tray state");
    let paused = !state.paused.fetch_xor(true, Ordering::AcqRel);
    if let Ok(mut view) = state.view.lock() {
        view.status = if paused { "Pausiert" } else { "Bereit" }.to_owned();
        view.detail = if paused {
            "Automatische Laeufe sind angehalten".to_owned()
        } else {
            "Naechster Lauf innerhalb von 15 Minuten".to_owned()
        };
    }
    refresh_controls();
    invalidate();
}

fn request_sync(manual: bool) {
    let state = STATE.get().expect("tray state").clone();
    if state.paused.load(Ordering::Acquire) && !manual {
        state.next_run.store(unix_seconds() + 60, Ordering::Release);
        return;
    }
    if state
        .syncing
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    if let Ok(mut view) = state.view.lock() {
        view.status = "Synchronisiert".to_owned();
        view.detail = "Hashes pr\u{00fc}fen \u{00b7} Uploads parallel \u{00fc}bertragen".to_owned();
    }
    refresh_controls();
    invalidate();

    thread::spawn(move || {
        let result = match SingleInstance::acquire() {
            Ok(_instance) => sync(&state.paths, &state.logger, false, None),
            Err(error) => Err(error),
        };
        let refreshed = counts(&state.paths).unwrap_or((0, 0, 0));
        if let Ok(mut view) = state.view.lock() {
            view.screenshots = refreshed.0;
            view.clips = refreshed.1;
            view.pending = refreshed.2;
            view.last_run = "Letzter Lauf: gerade eben".to_owned();
            match result {
                Ok(()) => {
                    view.status = "Aktuell".to_owned();
                    view.detail = if refreshed.2 == 0 {
                        "Alles sicher in Google Fotos".to_owned()
                    } else {
                        format!("Noch {} neue Dateien vorgemerkt", refreshed.2)
                    };
                }
                Err(error) => {
                    view.status = "Pruefen".to_owned();
                    view.detail = error.to_string();
                }
            }
        }
        state
            .next_run
            .store(unix_seconds() + 15 * 60, Ordering::Release);
        state.syncing.store(false, Ordering::Release);
        refresh_controls();
        invalidate();
    });
}

fn refresh_controls() {
    let Some(state) = STATE.get() else {
        return;
    };
    let hwnd = state.hwnd.load(Ordering::Acquire) as HWND;
    if hwnd.is_null() {
        return;
    }
    let syncing = state.syncing.load(Ordering::Acquire);
    let paused = state.paused.load(Ordering::Acquire);
    unsafe {
        let sync_button = GetDlgItem(hwnd, CMD_SYNC as i32);
        let pause_button = GetDlgItem(hwnd, CMD_PAUSE as i32);
        let sync_label = wide(if syncing {
            "Sicherung l\u{00e4}uft"
        } else {
            "Jetzt sichern"
        });
        let pause_label = wide(if paused { "Fortsetzen" } else { "Pausieren" });
        SetWindowTextW(sync_button, sync_label.as_ptr());
        SetWindowTextW(pause_button, pause_label.as_ptr());
        EnableWindow(sync_button, (!syncing).into());
        InvalidateRect(sync_button, null(), 0);
        InvalidateRect(pause_button, null(), 0);
    }
}

fn counts(paths: &AppPaths) -> super::AppResult<(i64, i64, usize)> {
    let connection = open_database(&paths.database)?;
    let screenshots: i64 = connection.query_row(
        "SELECT COUNT(*) FROM media WHERE album = ?1",
        params!["Screenshots"],
        |row| row.get(0),
    )?;
    let clips: i64 = connection.query_row(
        "SELECT COUNT(*) FROM media WHERE album = ?1",
        params!["AMD-Clips"],
        |row| row.get(0),
    )?;
    let mut source_total = 0_usize;
    for source in &paths.sources {
        if source.path.is_dir() {
            source_total += source_files(&source.path, source.extensions())?.len();
        }
    }
    let known = usize::try_from(screenshots + clips).unwrap_or_default();
    Ok((screenshots, clips, source_total.saturating_sub(known)))
}

fn invalidate() {
    if let Some(state) = STATE.get() {
        let raw = state.hwnd.load(Ordering::Acquire);
        if raw != 0 {
            unsafe { InvalidateRect(raw as HWND, null(), 0) };
        }
    }
}

fn paint_dashboard(hwnd: HWND) {
    let state = STATE.get().expect("tray state");
    let view = state.view.lock().expect("view state").clone();
    unsafe {
        ui::paint(
            hwnd,
            &view,
            state.paused.load(Ordering::Acquire),
            state.syncing.load(Ordering::Acquire),
            state.animation.load(Ordering::Relaxed),
            state.next_run.load(Ordering::Acquire),
        );
    }
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn copy_wide(target: &mut [u16], value: &str) {
    let encoded = wide(value);
    let length = encoded.len().min(target.len());
    target[..length].copy_from_slice(&encoded[..length]);
}
