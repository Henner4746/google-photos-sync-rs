#![allow(unsafe_op_in_unsafe_fn)]

use super::{
    AppPaths, DEFAULT_SCHEDULE_MINUTES, Logger, MediaKind, OperationProgress, SingleInstance,
    SourceSpec, authorize, authorize_json, current_record, disconnect_google,
    duplicate_guard_ready, embedded_oauth_client, hex_encode, import_takeout, open_database,
    save_config, set_autostart_executable, source_files, source_files_for_source, sync,
    trusted_state, unix_seconds,
};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, CreateRoundRectRgn,
    CreateSolidBrush, DEFAULT_GUI_FONT, DIB_RGB_COLORS, DeleteObject, GetMonitorInfoW,
    GetStockObject, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    SetBkColor, SetTextColor, SetWindowRgn, UpdateWindow,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows_sys::Win32::UI::Controls::{DRAWITEMSTRUCT, WM_MOUSELEAVE};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, IsWindowEnabled, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VK_ESCAPE,
};
use windows_sys::Win32::UI::Shell::{
    BIF_EDITBOX, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, DefSubclassProc, NIF_ICON,
    NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    NOTIFYICONIDENTIFIER, RemoveWindowSubclass, SHBrowseForFolderW, SHGetPathFromIDListW,
    SetWindowSubclass, Shell_NotifyIconGetRect, Shell_NotifyIconW, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, BM_GETSTATE, BN_CLICKED, BS_DEFPUSHBUTTON, BS_NOTIFY, BS_PUSHBUTTON, BST_PUSHED,
    CS_HREDRAW, CS_VREDRAW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, EN_CHANGE, GetCursorPos,
    GetDlgCtrlID, GetDlgItem, GetMessageW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    HMENU, HTCAPTION, HTCLIENT, HWND_TOP, ICONINFO, IDC_ARROW, IDC_HAND, IDYES, IsDialogMessageW,
    IsWindowVisible, KillTimer, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL,
    LB_SETITEMHEIGHT, LBN_DBLCLK, LBN_SELCHANGE, LBS_HASSTRINGS, LBS_NOINTEGRALHEIGHT, LBS_NOTIFY,
    LBS_OWNERDRAWFIXED, LoadCursorW, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNO, MF_SEPARATOR,
    MF_STRING, MSG, MessageBoxW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SW_HIDE,
    SW_SHOW, SW_SHOWNORMAL, SWP_SHOWWINDOW, SendMessageW, SetCursor, SetForegroundWindow, SetTimer,
    SetWindowPos, SetWindowTextW, ShowWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    TranslateMessage, WM_APP, WM_CLOSE, WM_COMMAND, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX,
    WM_DESTROY, WM_DRAWITEM, WM_ENABLE, WM_ERASEBKGND, WM_EXITSIZEMOVE, WM_KEYDOWN, WM_KILLFOCUS,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY, WM_NCHITTEST,
    WM_PAINT, WM_RBUTTONUP, WM_SETCURSOR, WM_SETFOCUS, WM_SETFONT, WM_TIMER, WNDCLASSW, WS_CHILD,
    WS_EX_CONTROLPARENT, WS_POPUP, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

#[path = "tray_ui.rs"]
mod ui;

const WM_TRAY: u32 = WM_APP + 1;
const TRAY_ID: u32 = 1;
const TIMER_SCHEDULE: usize = 1;
const TIMER_INITIAL: usize = 2;
const TIMER_ANIMATION: usize = 3;
const TIMER_UPDATE: usize = 4;
const SCHEDULE_INTERVAL_MS: u32 = 60_000;
const ANIMATION_INTERVAL_MS: u32 = 250;
const INITIAL_UPDATE_DELAY_MS: u32 = 60_000;
const UPDATE_INTERVAL_MS: u32 = 24 * 60 * 60 * 1_000;
const CMD_OPEN: usize = 1001;
const CMD_SYNC: usize = 1002;
const CMD_EXIT: usize = 1003;
const CMD_PAUSE: usize = 1004;
const CMD_CLOSE: usize = 1005;
const CMD_ADD_SOURCE: usize = 1006;
const CMD_SOURCES: usize = 1007;
const CMD_ALBUM: usize = 1008;
const CMD_SAVE_ALBUM: usize = 1009;
const CMD_OPEN_SOURCE: usize = 1010;
const CMD_TOGGLE_SOURCE: usize = 1011;
const CMD_REMOVE_SOURCE: usize = 1012;
const CMD_DRY_RUN: usize = 1013;
const CMD_OPEN_LOG: usize = 1014;
const CMD_OPEN_PHOTOS: usize = 1015;
const CMD_SETUP_GOOGLE: usize = 1016;
const CMD_SETUP_FOLDER: usize = 1017;
const CMD_SETUP_TAKEOUT: usize = 1018;
const CMD_SETUP_AUTOSTART: usize = 1019;
const CMD_SETUP_FINISH: usize = 1020;
const CMD_SCHEDULE: usize = 1021;
const CMD_EXCLUDE: usize = 1022;
const CMD_BACKUP: usize = 1023;
const CMD_RESTORE: usize = 1024;
const CMD_SETTINGS: usize = 1025;
const CMD_UPDATE: usize = 1026;
const CMD_DISCONNECT_GOOGLE: usize = 1027;
const CMD_SETTINGS_TAKEOUT: usize = 1028;
const WM_WORK_FINISHED: u32 = WM_APP + 2;
const WORK_GOOGLE: usize = 1;
const WORK_TAKEOUT: usize = 2;
const WORK_UPDATE: usize = 3;
const WORK_DISCONNECT: usize = 4;

const WINDOW_WIDTH: i32 = 720;
const WINDOW_HEIGHT: i32 = 656;
const NO_SELECTION: u32 = u32::MAX;

const BUTTON_CLOSE: RECT = rect(668, 20, 704, 56);
const BUTTON_ADD: RECT = rect(526, 294, 696, 334);
const SOURCE_LIST: RECT = rect(24, 342, 696, 476);
const ALBUM_EDIT: RECT = rect(40, 518, 280, 554);
const BUTTON_SAVE_ALBUM: RECT = rect(288, 518, 378, 554);
const BUTTON_OPEN_SOURCE: RECT = rect(390, 518, 478, 554);
const BUTTON_TOGGLE_SOURCE: RECT = rect(486, 518, 584, 554);
const BUTTON_REMOVE_SOURCE: RECT = rect(592, 518, 680, 554);
const BUTTON_SYNC: RECT = rect(24, 588, 236, 632);
const BUTTON_DRY_RUN: RECT = rect(248, 588, 396, 632);
const BUTTON_PAUSE: RECT = rect(408, 588, 562, 632);
const BUTTON_OPEN_LOG: RECT = rect(574, 588, 696, 632);

const SETUP_GOOGLE: RECT = rect(40, 242, 680, 290);
const SETUP_FOLDER: RECT = rect(40, 306, 680, 354);
const SETUP_TAKEOUT: RECT = rect(40, 370, 680, 418);
const SETUP_AUTOSTART: RECT = rect(40, 434, 680, 482);
const SETUP_FINISH: RECT = rect(40, 526, 680, 578);
const BUTTON_SETTINGS: RECT = rect(620, 20, 660, 56);
const SETTINGS_SCHEDULE: RECT = rect(40, 168, 680, 216);
const SETTINGS_EXCLUDE: RECT = rect(40, 232, 680, 280);
const SETTINGS_BACKUP: RECT = rect(40, 328, 350, 376);
const SETTINGS_RESTORE: RECT = rect(370, 328, 680, 376);
const SETTINGS_TAKEOUT: RECT = rect(40, 446, 680, 486);
const SETTINGS_UPDATE: RECT = rect(40, 526, 350, 578);
const SETTINGS_DISCONNECT: RECT = rect(370, 526, 680, 578);

static STATE: OnceLock<Arc<TrayState>> = OnceLock::new();
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static APP_ICON: AtomicIsize = AtomicIsize::new(0);
static CONTROL_BRUSH: AtomicIsize = AtomicIsize::new(0);
static HOVERED_CONTROL: AtomicU32 = AtomicU32::new(0);

#[derive(Clone)]
struct ViewState {
    status: String,
    detail: String,
    last_run: String,
    protected: i64,
    folders: usize,
    pending: usize,
    progress_files: usize,
    progress_total: usize,
    progress_bytes: u64,
    progress_speed: u64,
}

struct TrayState {
    paths: AppPaths,
    logger: Arc<Logger>,
    sources: Mutex<Vec<SourceSpec>>,
    window_position: Mutex<Option<(i32, i32)>>,
    view: Mutex<ViewState>,
    syncing: AtomicBool,
    dry_run: AtomicBool,
    paused: AtomicBool,
    onboarding_completed: AtomicBool,
    autostart_enabled: AtomicBool,
    auto_update: AtomicBool,
    takeout_imported_at: AtomicI64,
    takeout_not_required_confirmed: AtomicBool,
    progress: Arc<OperationProgress>,
    positioned: AtomicBool,
    next_run: AtomicI64,
    selected: AtomicU32,
    hwnd: AtomicIsize,
    animation: AtomicU32,
    setup_mode: AtomicBool,
    working: AtomicBool,
    pending_error: Mutex<Option<String>>,
    settings_mode: AtomicBool,
    update_ready: AtomicBool,
    counts_loaded: AtomicBool,
}

pub(super) fn run(
    paths: AppPaths,
    logger: Logger,
    show_on_start: bool,
    sync_on_start: bool,
) -> super::AppResult<()> {
    let _tray_instance = SingleInstance::acquire_tray()?;
    let initial_sources = paths.sources.clone();
    let initial_next_run = next_due_time(&initial_sources);
    let initial_paused = paths.paused;
    let initial_onboarding_completed = paths.onboarding_completed;
    let initial_autostart_enabled = paths.autostart_enabled;
    let initial_auto_update = paths.auto_update;
    let initial_takeout_imported_at = paths.takeout_imported_at.unwrap_or(0);
    let initial_takeout_not_required_confirmed = paths.takeout_not_required_confirmed;
    let initial_setup_mode = !initial_onboarding_completed || !state_ready_for_dashboard(&paths);
    let load_initial_counts = show_on_start || initial_setup_mode || !sync_on_start;
    let (protected, folders, pending) = if load_initial_counts {
        counts(&paths, &initial_sources).unwrap_or((0, initial_sources.len(), 0))
    } else {
        // Autostart stays lightweight: the due sync refreshes these values once instead of
        // walking every configured folder twice during Windows sign-in.
        (0, initial_sources.len(), 0)
    };
    let state = Arc::new(TrayState {
        window_position: Mutex::new(paths.window_position),
        paths,
        logger: Arc::new(logger),
        sources: Mutex::new(initial_sources),
        view: Mutex::new(ViewState {
            status: "Bereit".to_owned(),
            detail: "Unver\u{00e4}nderte Medien bleiben vollst\u{00e4}ndig offline".to_owned(),
            last_run: "Noch kein Lauf in dieser Sitzung".to_owned(),
            protected,
            folders,
            pending,
            progress_files: 0,
            progress_total: 0,
            progress_bytes: 0,
            progress_speed: 0,
        }),
        syncing: AtomicBool::new(false),
        dry_run: AtomicBool::new(false),
        paused: AtomicBool::new(initial_paused),
        onboarding_completed: AtomicBool::new(initial_onboarding_completed),
        autostart_enabled: AtomicBool::new(initial_autostart_enabled),
        auto_update: AtomicBool::new(initial_auto_update),
        takeout_imported_at: AtomicI64::new(initial_takeout_imported_at),
        takeout_not_required_confirmed: AtomicBool::new(initial_takeout_not_required_confirmed),
        progress: Arc::new(OperationProgress::default()),
        positioned: AtomicBool::new(false),
        next_run: AtomicI64::new(initial_next_run),
        selected: AtomicU32::new(if folders == 0 { NO_SELECTION } else { 0 }),
        hwnd: AtomicIsize::new(0),
        animation: AtomicU32::new(0),
        setup_mode: AtomicBool::new(initial_setup_mode),
        working: AtomicBool::new(false),
        pending_error: Mutex::new(None),
        settings_mode: AtomicBool::new(false),
        update_ready: AtomicBool::new(false),
        counts_loaded: AtomicBool::new(load_initial_counts),
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
        TASKBAR_CREATED.store(
            RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
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
        let hwnd = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class_name.as_ptr(),
            wide("Google Photos Sync").as_ptr(),
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
        CONTROL_BRUSH.store(CreateSolidBrush(gray(26)) as isize, Ordering::Release);
        create_controls(hwnd, instance)?;
        SetWindowRgn(
            hwnd,
            CreateRoundRectRgn(0, 0, WINDOW_WIDTH + 1, WINDOW_HEIGHT + 1, 28, 28),
            1,
        );
        add_tray_icon(hwnd)?;
        SetTimer(hwnd, TIMER_SCHEDULE, SCHEDULE_INTERVAL_MS, None);
        if STATE
            .get()
            .expect("tray state")
            .auto_update
            .load(Ordering::Acquire)
            && !STATE
                .get()
                .expect("tray state")
                .setup_mode
                .load(Ordering::Acquire)
        {
            SetTimer(hwnd, TIMER_UPDATE, INITIAL_UPDATE_DELAY_MS, None);
        }
        if sync_on_start
            && !STATE
                .get()
                .expect("tray state")
                .setup_mode
                .load(Ordering::Acquire)
            && initial_sync_is_due()
        {
            SetTimer(hwnd, TIMER_INITIAL, 1_000, None);
        }
        refresh_source_list();
        refresh_mode_controls();
        if show_on_start
            || STATE
                .get()
                .expect("tray state")
                .setup_mode
                .load(Ordering::Acquire)
        {
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

unsafe fn create_controls(hwnd: HWND, instance: HINSTANCE) -> super::AppResult<()> {
    let buttons = [
        (CMD_CLOSE, "\u{00d7}", BUTTON_CLOSE),
        (CMD_ADD_SOURCE, "+  Ordner hinzuf\u{00fc}gen", BUTTON_ADD),
        (CMD_SAVE_ALBUM, "Speichern", BUTTON_SAVE_ALBUM),
        (CMD_OPEN_SOURCE, "\u{00d6}ffnen", BUTTON_OPEN_SOURCE),
        (CMD_TOGGLE_SOURCE, "Pausieren", BUTTON_TOGGLE_SOURCE),
        (CMD_REMOVE_SOURCE, "Entfernen", BUTTON_REMOVE_SOURCE),
        (CMD_SYNC, "Jetzt sichern", BUTTON_SYNC),
        (CMD_DRY_RUN, "Testlauf", BUTTON_DRY_RUN),
        (CMD_PAUSE, "Automatik pausieren", BUTTON_PAUSE),
        (CMD_OPEN_LOG, "Protokoll", BUTTON_OPEN_LOG),
        (CMD_SETUP_GOOGLE, "Mit Google verbinden", SETUP_GOOGLE),
        (CMD_SETUP_FOLDER, "Sicherungsordner auswählen", SETUP_FOLDER),
        (
            CMD_SETUP_TAKEOUT,
            "Takeout für sicheren Duplikatschutz",
            SETUP_TAKEOUT,
        ),
        (CMD_SETUP_AUTOSTART, "Mit Windows starten", SETUP_AUTOSTART),
        (CMD_SETUP_FINISH, "Einrichtung abschließen", SETUP_FINISH),
        (CMD_SETTINGS, "⋯", BUTTON_SETTINGS),
        (CMD_SCHEDULE, "Zeitplan", SETTINGS_SCHEDULE),
        (CMD_EXCLUDE, "Unterordner ausschließen", SETTINGS_EXCLUDE),
        (CMD_BACKUP, "Daten sichern", SETTINGS_BACKUP),
        (CMD_RESTORE, "Daten wiederherstellen", SETTINGS_RESTORE),
        (
            CMD_SETTINGS_TAKEOUT,
            "Google Takeout importieren",
            SETTINGS_TAKEOUT,
        ),
        (CMD_UPDATE, "Nach Aktualisierung suchen", SETTINGS_UPDATE),
        (CMD_DISCONNECT_GOOGLE, "Google trennen", SETTINGS_DISCONNECT),
    ];
    for (id, label, rect) in buttons {
        let style = WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | BS_NOTIFY as u32
            | if matches!(id, CMD_SYNC | CMD_SETUP_FINISH) {
                BS_DEFPUSHBUTTON as u32
            } else {
                BS_PUSHBUTTON as u32
            };
        let button = CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(label).as_ptr(),
            style,
            rect.left,
            rect.top,
            width(rect),
            height(rect),
            hwnd,
            id as _,
            instance,
            null(),
        );
        if button.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        SetWindowSubclass(button, Some(button_proc), id, 0);
    }

    let list = CreateWindowExW(
        0,
        wide("LISTBOX").as_ptr(),
        null(),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WS_VSCROLL
            | LBS_NOTIFY as u32
            | LBS_OWNERDRAWFIXED as u32
            | LBS_HASSTRINGS as u32
            | LBS_NOINTEGRALHEIGHT as u32,
        SOURCE_LIST.left,
        SOURCE_LIST.top,
        width(SOURCE_LIST),
        height(SOURCE_LIST),
        hwnd,
        CMD_SOURCES as _,
        instance,
        null(),
    );
    let edit = CreateWindowExW(
        0,
        wide("EDIT").as_ptr(),
        null(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        ALBUM_EDIT.left,
        ALBUM_EDIT.top,
        width(ALBUM_EDIT),
        height(ALBUM_EDIT),
        hwnd,
        CMD_ALBUM as _,
        instance,
        null(),
    );
    if list.is_null() || edit.is_null() {
        return Err(io::Error::last_os_error().into());
    }
    SendMessageW(list, LB_SETITEMHEIGHT, 0, 58);
    let font = GetStockObject(DEFAULT_GUI_FONT);
    SendMessageW(list, WM_SETFONT, font as usize, 1);
    SendMessageW(edit, WM_SETFONT, font as usize, 1);
    SetWindowRgn(
        list,
        CreateRoundRectRgn(0, 0, width(SOURCE_LIST), height(SOURCE_LIST), 12, 12),
        1,
    );
    SetWindowRgn(
        edit,
        CreateRoundRectRgn(0, 0, width(ALBUM_EDIT), height(ALBUM_EDIT), 12, 12),
        1,
    );
    refresh_mode_controls();
    Ok(())
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
        WM_NCHITTEST => {
            let (_, screen_y) = screen_point(lparam);
            let mut window = RECT::default();
            GetWindowRect(hwnd, &mut window);
            if screen_y - window.top < 76 {
                HTCAPTION as LRESULT
            } else {
                HTCLIENT as LRESULT
            }
        }
        WM_EXITSIZEMOVE => {
            persist_window_position(hwnd);
            0
        }
        WM_PAINT => {
            paint_dashboard(hwnd);
            0
        }
        WM_DRAWITEM => {
            let item = &*(lparam as *const DRAWITEMSTRUCT);
            if item.CtlID as usize == CMD_SOURCES {
                let state = STATE.get().expect("tray state");
                let sources = state.sources.lock().expect("source state");
                ui::paint_source_item(item, sources.get(item.itemID as usize));
                return 1;
            }
            0
        }
        WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
            let dc = wparam as _;
            SetBkColor(dc, gray(26));
            SetTextColor(dc, gray(238));
            CONTROL_BRUSH.load(Ordering::Acquire) as LRESULT
        }
        WM_ERASEBKGND => 1,
        WM_TIMER => {
            handle_timer(hwnd, wparam);
            0
        }
        WM_WORK_FINISHED => {
            finish_background_work(wparam);
            0
        }
        WM_COMMAND => {
            let command = wparam & 0xffff;
            let notification = (wparam >> 16) as u32;
            if command == CMD_SOURCES {
                if notification == LBN_SELCHANGE {
                    update_selection();
                } else if notification == LBN_DBLCLK {
                    open_selected_source(hwnd);
                }
            } else if command == CMD_ALBUM && notification == EN_CHANGE {
                EnableWindow(GetDlgItem(hwnd, CMD_SAVE_ALBUM as i32), 1);
            } else if notification == BN_CLICKED
                || matches!(command, CMD_OPEN | CMD_EXIT | CMD_OPEN_PHOTOS)
            {
                handle_command(hwnd, command);
            }
            0
        }
        WM_CLOSE => {
            ShowWindow(hwnd, SW_HIDE);
            0
        }
        WM_DESTROY => {
            remove_tray_icon(hwnd);
            let icon = APP_ICON.swap(0, Ordering::AcqRel);
            if icon != 0 {
                DestroyIcon(icon as _);
            }
            let brush = CONTROL_BRUSH.swap(0, Ordering::AcqRel);
            if brush != 0 {
                DeleteObject(brush as _);
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
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
            let pressed = SendMessageW(hwnd, BM_GETSTATE, 0, 0) as u32 & BST_PUSHED != 0;
            ui::paint_button(
                hwnd,
                GetDlgCtrlID(hwnd) as usize,
                HOVERED_CONTROL.load(Ordering::Acquire) as usize == GetDlgCtrlID(hwnd) as usize,
                pressed,
                GetFocus() == hwnd,
                IsWindowEnabled(hwnd) == 0,
            );
            0
        }
        WM_MOUSEMOVE => {
            let id = GetDlgCtrlID(hwnd) as u32;
            if HOVERED_CONTROL.swap(id, Ordering::AcqRel) != id {
                InvalidateRect(hwnd, null(), 0);
            }
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            TrackMouseEvent(&mut tracking);
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
        WM_MOUSELEAVE => {
            let id = GetDlgCtrlID(hwnd) as u32;
            let _ = HOVERED_CONTROL.compare_exchange(id, 0, Ordering::AcqRel, Ordering::Acquire);
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(null_mut(), IDC_HAND));
            1
        }
        WM_ERASEBKGND => 1,
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_SETFOCUS | WM_KILLFOCUS | WM_ENABLE => {
            let result = DefSubclassProc(hwnd, message, wparam, lparam);
            InvalidateRect(hwnd, null(), 0);
            result
        }
        WM_NCDESTROY => {
            RemoveWindowSubclass(hwnd, Some(button_proc), subclass_id);
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
        _ => DefSubclassProc(hwnd, message, wparam, lparam),
    }
}

fn handle_timer(hwnd: HWND, timer: usize) {
    if timer == TIMER_INITIAL {
        unsafe { KillTimer(hwnd, TIMER_INITIAL) };
        request_sync(false, false);
    } else if timer == TIMER_SCHEDULE {
        let state = STATE.get().expect("tray state");
        if !state.setup_mode.load(Ordering::Acquire)
            && unix_seconds() >= state.next_run.load(Ordering::Acquire)
        {
            request_sync(false, false);
        }
        if unsafe { IsWindowVisible(hwnd) } != 0 {
            invalidate();
        }
    } else if timer == TIMER_UPDATE {
        unsafe { KillTimer(hwnd, TIMER_UPDATE) };
        request_update();
    } else if timer == TIMER_ANIMATION && {
        let state = STATE.get().expect("tray state");
        state.syncing.load(Ordering::Acquire) || state.working.load(Ordering::Acquire)
    } {
        let state = STATE.get().expect("tray state");
        state.animation.fetch_add(1, Ordering::Relaxed);
        let files = state.progress.files_done.load(Ordering::Acquire);
        let total = state.progress.files_total.load(Ordering::Acquire);
        let bytes = state
            .progress
            .bytes_done
            .load(Ordering::Acquire)
            .min(state.progress.bytes_total.load(Ordering::Acquire));
        let elapsed = (unix_seconds() - state.progress.started_at.load(Ordering::Acquire)).max(1);
        if let Ok(mut view) = state.view.lock() {
            view.progress_files = files;
            view.progress_total = total;
            view.progress_bytes = bytes;
            view.progress_speed = bytes / elapsed as u64;
            if total > 0 {
                view.detail = format!(
                    "{} / {} Dateien · {}/s",
                    files,
                    total,
                    format_bytes(view.progress_speed)
                );
            }
        }
        invalidate();
    }
}

fn show_error_notification(detail: &str) {
    let Some(state) = STATE.get() else { return };
    let hwnd = state.hwnd.load(Ordering::Acquire) as HWND;
    if hwnd.is_null() {
        return;
    }
    unsafe {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            uFlags: NIF_INFO,
            dwInfoFlags: NIIF_ERROR,
            ..Default::default()
        };
        copy_wide(&mut data.szInfoTitle, "Google Photos Sync braucht Hilfe");
        copy_wide(&mut data.szInfo, detail);
        Shell_NotifyIconW(NIM_MODIFY, &data);
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
        copy_wide(&mut data.szTip, "Google Photos Sync \u{00b7} bereit");
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
                let ring = (76..=126).contains(&distance);
                let forward = (20..=27).contains(&x) && (6..=13).contains(&y) && x + y >= 31;
                let back = (5..=12).contains(&x) && (19..=26).contains(&y) && x + y <= 31;
                pixels[(y * 32 + x) as usize] = if ring || forward || back {
                    0xff_f2_f2_f2
                } else if distance <= 225 {
                    0xff_11_11_11
                } else {
                    0
                };
            }
        }
        let mask = CreateBitmap(32, 32, 1, 1, null());
        let icon = CreateIconIndirect(&ICONINFO {
            fIcon: 1,
            hbmColor: color,
            hbmMask: mask,
            ..Default::default()
        });
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}

fn show_dashboard(hwnd: HWND) {
    let state = STATE.get().expect("tray state");
    if !state.counts_loaded.load(Ordering::Acquire) {
        refresh_counts();
    }
    unsafe {
        refresh_controls();
        if !state.positioned.swap(true, Ordering::AcqRel) {
            let (x, y) = initial_window_position(hwnd);
            SetWindowPos(
                hwnd,
                HWND_TOP,
                x,
                y,
                WINDOW_WIDTH,
                WINDOW_HEIGHT,
                SWP_SHOWWINDOW,
            );
        } else {
            ShowWindow(hwnd, SW_SHOW);
        }
        SetForegroundWindow(hwnd);
        let focus = if state.setup_mode.load(Ordering::Acquire) {
            CMD_SETUP_FINISH
        } else if state.settings_mode.load(Ordering::Acquire) {
            CMD_SETTINGS
        } else {
            CMD_SYNC
        };
        SetFocus(GetDlgItem(hwnd, focus as i32));
        InvalidateRect(hwnd, null(), 0);
        UpdateWindow(hwnd);
    }
}

fn initial_window_position(hwnd: HWND) -> (i32, i32) {
    let state = STATE.get().expect("tray state");
    if let Some(position) = *state.window_position.lock().expect("window position") {
        return clamp_to_monitor(position.0, position.1);
    }
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
            return clamp_to_monitor(cursor.x - WINDOW_WIDTH, cursor.y - WINDOW_HEIGHT);
        }
        clamp_to_monitor(icon.right - WINDOW_WIDTH, icon.top - WINDOW_HEIGHT - 10)
    }
}

fn clamp_to_monitor(x: i32, y: i32) -> (i32, i32) {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        GetMonitorInfoW(monitor, &mut info);
        (
            x.clamp(info.rcWork.left, info.rcWork.right - WINDOW_WIDTH),
            y.clamp(info.rcWork.top, info.rcWork.bottom - WINDOW_HEIGHT),
        )
    }
}

fn persist_window_position(hwnd: HWND) {
    let state = STATE.get().expect("tray state");
    unsafe {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return;
        }
        *state.window_position.lock().expect("window position") = Some((rect.left, rect.top));
    }
    let _ = persist_sources();
}

fn tray_menu(hwnd: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        append_menu(menu, CMD_OPEN, "App \u{00f6}ffnen");
        append_menu(menu, CMD_ADD_SOURCE, "Ordner hinzuf\u{00fc}gen");
        append_menu(menu, CMD_SYNC, "Jetzt sichern");
        append_menu(menu, CMD_DRY_RUN, "Testlauf ohne Uploads");
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        append_menu(menu, CMD_OPEN_PHOTOS, "Google Photos \u{00f6}ffnen");
        append_menu(menu, CMD_OPEN_LOG, "Protokoll \u{00f6}ffnen");
        AppendMenuW(menu, MF_SEPARATOR, 0, null());
        append_menu(menu, CMD_EXIT, "Beenden");
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

unsafe fn append_menu(menu: HMENU, id: usize, label: &str) {
    unsafe { AppendMenuW(menu, MF_STRING, id, wide(label).as_ptr()) };
}

fn handle_command(hwnd: HWND, command: usize) {
    match command {
        CMD_OPEN => show_dashboard(hwnd),
        CMD_CLOSE => unsafe {
            ShowWindow(hwnd, SW_HIDE);
        },
        CMD_SYNC => request_sync(true, false),
        CMD_DRY_RUN => request_sync(true, true),
        CMD_PAUSE => toggle_pause(),
        CMD_ADD_SOURCE => add_source(hwnd),
        CMD_SAVE_ALBUM => save_album_name(),
        CMD_OPEN_SOURCE => open_selected_source(hwnd),
        CMD_TOGGLE_SOURCE => toggle_selected_source(),
        CMD_REMOVE_SOURCE => remove_selected_source(),
        CMD_SETUP_GOOGLE => setup_google(hwnd),
        CMD_SETUP_FOLDER => add_source(hwnd),
        CMD_SETUP_TAKEOUT => setup_takeout(hwnd),
        CMD_SETTINGS_TAKEOUT => setup_takeout(hwnd),
        CMD_SETUP_AUTOSTART => toggle_autostart(),
        CMD_SETUP_FINISH => finish_setup(hwnd),
        CMD_SETTINGS => toggle_settings(),
        CMD_SCHEDULE => cycle_schedule(),
        CMD_EXCLUDE => exclude_subfolder(hwnd),
        CMD_BACKUP => backup_database(hwnd),
        CMD_RESTORE => restore_database(hwnd),
        CMD_UPDATE => request_update(),
        CMD_DISCONNECT_GOOGLE => disconnect_google_account(hwnd),
        CMD_OPEN_LOG => open_path(hwnd, &STATE.get().expect("tray state").paths.log),
        CMD_OPEN_PHOTOS => open_target(hwnd, "https://photos.google.com/"),
        CMD_EXIT => unsafe {
            DestroyWindow(hwnd);
        },
        _ => {}
    }
}

fn state_ready_for_dashboard(paths: &AppPaths) -> bool {
    paths.credentials.is_file() && !paths.sources.is_empty()
}

fn refresh_mode_controls() {
    let Some(state) = STATE.get() else { return };
    let hwnd = state.hwnd.load(Ordering::Acquire) as HWND;
    if hwnd.is_null() {
        return;
    }
    let setup = state.setup_mode.load(Ordering::Acquire);
    let settings = state.settings_mode.load(Ordering::Acquire);
    let dashboard = [
        CMD_ADD_SOURCE,
        CMD_SOURCES,
        CMD_ALBUM,
        CMD_SAVE_ALBUM,
        CMD_OPEN_SOURCE,
        CMD_TOGGLE_SOURCE,
        CMD_REMOVE_SOURCE,
        CMD_SYNC,
        CMD_DRY_RUN,
        CMD_PAUSE,
        CMD_OPEN_LOG,
    ];
    let setup_controls = [
        CMD_SETUP_GOOGLE,
        CMD_SETUP_FOLDER,
        CMD_SETUP_TAKEOUT,
        CMD_SETUP_AUTOSTART,
        CMD_SETUP_FINISH,
    ];
    unsafe {
        for id in dashboard {
            ShowWindow(
                GetDlgItem(hwnd, id as i32),
                if setup || settings { SW_HIDE } else { SW_SHOW },
            );
        }
        for id in setup_controls {
            ShowWindow(
                GetDlgItem(hwnd, id as i32),
                if setup && !settings { SW_SHOW } else { SW_HIDE },
            );
        }
        ShowWindow(
            GetDlgItem(hwnd, CMD_SETTINGS as i32),
            if setup { SW_HIDE } else { SW_SHOW },
        );
        for id in [
            CMD_SCHEDULE,
            CMD_EXCLUDE,
            CMD_BACKUP,
            CMD_RESTORE,
            CMD_SETTINGS_TAKEOUT,
            CMD_UPDATE,
            CMD_DISCONNECT_GOOGLE,
        ] {
            ShowWindow(
                GetDlgItem(hwnd, id as i32),
                if settings { SW_SHOW } else { SW_HIDE },
            );
        }
        ShowWindow(
            GetDlgItem(hwnd, CMD_SETUP_AUTOSTART as i32),
            if setup || settings { SW_SHOW } else { SW_HIDE },
        );
        SetWindowTextW(
            GetDlgItem(hwnd, CMD_SETTINGS as i32),
            wide(if settings { "←" } else { "⋯" }).as_ptr(),
        );
        SetWindowTextW(
            GetDlgItem(hwnd, CMD_SETUP_GOOGLE as i32),
            wide(if state.paths.credentials.is_file() {
                "Google verbunden"
            } else {
                "Verstanden · Mit Google verbinden"
            })
            .as_ptr(),
        );
        SetWindowTextW(
            GetDlgItem(hwnd, CMD_SETUP_FOLDER as i32),
            wide(if state.sources.lock().expect("source state").is_empty() {
                "Sicherungsordner auswählen"
            } else {
                "Weiteren Sicherungsordner auswählen"
            })
            .as_ptr(),
        );
        SetWindowTextW(
            GetDlgItem(hwnd, CMD_SETUP_AUTOSTART as i32),
            wide(if state.autostart_enabled.load(Ordering::Acquire) {
                "Mit Windows starten: Ein"
            } else {
                "Mit Windows starten: Aus"
            })
            .as_ptr(),
        );
        SetWindowTextW(
            GetDlgItem(hwnd, CMD_SETTINGS_TAKEOUT as i32),
            wide(if state.takeout_imported_at.load(Ordering::Acquire) > 0 {
                "Google Takeout aktualisieren"
            } else {
                "Google Takeout importieren"
            })
            .as_ptr(),
        );
        let ready = !state.sources.lock().expect("source state").is_empty()
            && state.paths.credentials.is_file();
        EnableWindow(GetDlgItem(hwnd, CMD_SETUP_FINISH as i32), ready.into());
        let source = selected_source();
        SetWindowTextW(
            GetDlgItem(hwnd, CMD_SCHEDULE as i32),
            wide(&source.as_ref().map_or_else(
                || "Zuerst einen Ordner auswählen".to_owned(),
                |source| {
                    format!(
                        "Zeitplan für {}: {}",
                        source.album,
                        schedule_label(source.schedule_minutes)
                    )
                },
            ))
            .as_ptr(),
        );
        SetWindowTextW(
            GetDlgItem(hwnd, CMD_EXCLUDE as i32),
            wide(&source.as_ref().map_or_else(
                || "Zuerst einen Ordner auswählen".to_owned(),
                |source| {
                    format!(
                        "Unterordner ausschließen · {} aktiv",
                        source.excluded_subfolders.len()
                    )
                },
            ))
            .as_ptr(),
        );
        EnableWindow(
            GetDlgItem(hwnd, CMD_SCHEDULE as i32),
            source.is_some().into(),
        );
        EnableWindow(
            GetDlgItem(hwnd, CMD_EXCLUDE as i32),
            source.is_some().into(),
        );
    }
    invalidate();
}

fn setup_google(hwnd: HWND) {
    if STATE
        .get()
        .is_some_and(|state| state.working.swap(true, Ordering::AcqRel))
    {
        return;
    }
    let state = STATE.get().expect("tray state").clone();
    state.progress.begin();
    refresh_animation_timer();
    set_message("Google-Anmeldung", "Der Browser wird geöffnet");
    let selected = if embedded_oauth_client().is_none() {
        choose_json_file(hwnd)
    } else {
        None
    };
    if embedded_oauth_client().is_none() && selected.is_none() {
        state.working.store(false, Ordering::Release);
        refresh_animation_timer();
        return;
    }
    thread::spawn(move || {
        let result = if let Some(client) = embedded_oauth_client() {
            authorize_json(&state.paths, client)
        } else {
            authorize(
                &state.paths,
                selected.as_deref().expect("selected oauth file"),
            )
        };
        *state.pending_error.lock().expect("background result") =
            result.err().map(|error| error.to_string());
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(
                state.hwnd.load(Ordering::Acquire) as HWND,
                WM_WORK_FINISHED,
                WORK_GOOGLE,
                0,
            )
        };
    });
}

fn setup_takeout(hwnd: HWND) {
    let Some(folder) = choose_folder_titled(hwnd, "Entpackten Google-Takeout-Ordner auswählen")
    else {
        return;
    };
    let state = STATE.get().expect("tray state").clone();
    if state.working.swap(true, Ordering::AcqRel) {
        return;
    }
    state.progress.begin();
    refresh_animation_timer();
    set_message(
        "Takeout wird eingelesen",
        "Bestehende Inhalte werden lokal erkannt",
    );
    thread::spawn(move || {
        let result = import_takeout(
            &state.paths,
            &state.logger,
            &folder,
            Some(state.progress.clone()),
        );
        *state.pending_error.lock().expect("background result") =
            result.err().map(|error| error.to_string());
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(
                state.hwnd.load(Ordering::Acquire) as HWND,
                WM_WORK_FINISHED,
                WORK_TAKEOUT,
                0,
            )
        };
    });
}

fn toggle_autostart() {
    let state = STATE.get().expect("tray state");
    let enabled = !state.autostart_enabled.load(Ordering::Acquire);
    match std::env::current_exe().and_then(|path| {
        set_autostart_executable(&path, enabled)
            .map_err(|error| io::Error::other(error.to_string()))
    }) {
        Ok(()) => {
            state.autostart_enabled.store(enabled, Ordering::Release);
            let _ = persist_sources();
            refresh_mode_controls();
        }
        Err(error) => set_message("Autostart konnte nicht geändert werden", &error.to_string()),
    }
}

fn duplicate_protection_ready(state: &TrayState) -> bool {
    duplicate_guard_ready(
        match state.takeout_imported_at.load(Ordering::Acquire) {
            0 => None,
            value => Some(value),
        },
        state.takeout_not_required_confirmed.load(Ordering::Acquire),
    )
}

fn confirm_no_older_copies(hwnd: HWND) -> bool {
    unsafe {
        MessageBoxW(
            hwnd,
            wide(
                "Google darf ältere Fotos seit März 2025 nicht an diese App melden. Ohne Takeout kann die App deshalb nicht erkennen, ob Dateien aus den gewählten Ordnern schon in Google Fotos liegen.\n\nFahre nur ohne Takeout fort, wenn dort keine älteren Kopien aus diesen Ordnern vorhanden sind. Andernfalls wähle Nein und importiere zuerst Takeout.\n\nOhne Takeout fortfahren?",
            )
            .as_ptr(),
            wide("Sicher vor doppelten Uploads").as_ptr(),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        ) == IDYES
    }
}

fn finish_setup(hwnd: HWND) {
    let state = STATE.get().expect("tray state");
    if !state.paths.credentials.is_file() || state.sources.lock().expect("source state").is_empty()
    {
        set_message(
            "Einrichtung noch nicht fertig",
            "Google verbinden und mindestens einen Ordner auswählen",
        );
        return;
    }
    if !duplicate_protection_ready(state) {
        if !confirm_no_older_copies(hwnd) {
            set_message(
                "Takeout schützt vor doppelten Uploads",
                "Takeout auswählen, danach die Einrichtung abschließen",
            );
            return;
        }
        state
            .takeout_not_required_confirmed
            .store(true, Ordering::Release);
    }
    state.onboarding_completed.store(true, Ordering::Release);
    state.setup_mode.store(false, Ordering::Release);
    let _ = persist_sources();
    refresh_mode_controls();
    refresh_source_list();
    set_message("Bereit", "Nur neue Inhalte werden hochgeladen");
}

fn disconnect_google_account(hwnd: HWND) {
    let choice = unsafe {
        MessageBoxW(
            hwnd,
            wide(
                "Der Google-Zugriff wird widerrufen und der lokal verschlüsselte Zugang gelöscht. Deine Fotos bleiben unverändert.",
            )
            .as_ptr(),
            wide("Google wirklich trennen?").as_ptr(),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
        )
    };
    if choice != IDYES {
        return;
    }
    let state = STATE.get().expect("tray state").clone();
    if state.working.swap(true, Ordering::AcqRel) {
        return;
    }
    state.progress.begin();
    refresh_animation_timer();
    set_message("Google wird getrennt", "Der Zugriff wird sicher widerrufen");
    thread::spawn(move || {
        let result = disconnect_google(&state.paths);
        *state.pending_error.lock().expect("background result") =
            result.err().map(|error| error.to_string());
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(
                state.hwnd.load(Ordering::Acquire) as HWND,
                WM_WORK_FINISHED,
                WORK_DISCONNECT,
                0,
            )
        };
    });
}

fn finish_background_work(kind: usize) {
    let state = STATE.get().expect("tray state");
    state.working.store(false, Ordering::Release);
    refresh_animation_timer();
    if kind == WORK_UPDATE && state.auto_update.load(Ordering::Acquire) {
        unsafe {
            SetTimer(
                state.hwnd.load(Ordering::Acquire) as HWND,
                TIMER_UPDATE,
                UPDATE_INTERVAL_MS,
                None,
            );
        }
    }
    if let Some(error) = state
        .pending_error
        .lock()
        .expect("background result")
        .take()
    {
        set_message("Aktion erforderlich", &error);
        show_error_notification(&error);
    } else if kind == WORK_GOOGLE {
        set_message(
            "Google verbunden",
            "Die Zugangsdaten sind mit Windows geschützt",
        );
    } else if kind == WORK_TAKEOUT {
        state
            .takeout_imported_at
            .store(unix_seconds(), Ordering::Release);
        state
            .takeout_not_required_confirmed
            .store(false, Ordering::Release);
        let _ = persist_sources();
        set_message(
            "Takeout importiert",
            "Vorhandene Inhalte werden nicht erneut hochgeladen",
        );
    } else if kind == WORK_UPDATE {
        if state.update_ready.load(Ordering::Acquire) {
            set_message("Aktualisierung bereit", "Die App startet gleich neu");
            unsafe { DestroyWindow(state.hwnd.load(Ordering::Acquire) as HWND) };
            return;
        }
        set_message("Alles aktuell", "Du verwendest bereits die neueste Version");
    } else if kind == WORK_DISCONNECT {
        state.onboarding_completed.store(false, Ordering::Release);
        state.settings_mode.store(false, Ordering::Release);
        state.setup_mode.store(true, Ordering::Release);
        let _ = persist_sources();
        set_message(
            "Google getrennt",
            "Fotos und lokale Duplikatdaten bleiben unverändert",
        );
    }
    refresh_mode_controls();
    refresh_counts();
}

fn toggle_settings() {
    let state = STATE.get().expect("tray state");
    let open = !state.settings_mode.fetch_xor(true, Ordering::AcqRel);
    if open {
        set_message("Einstellungen", "Zeitpläne, Ausschlüsse und App-Daten");
    }
    refresh_mode_controls();
}

fn schedule_label(minutes: u32) -> String {
    match minutes {
        5 => "alle 5 Minuten".to_owned(),
        15 => "alle 15 Minuten".to_owned(),
        30 => "alle 30 Minuten".to_owned(),
        60 => "stündlich".to_owned(),
        180 => "alle 3 Stunden".to_owned(),
        360 => "alle 6 Stunden".to_owned(),
        720 => "alle 12 Stunden".to_owned(),
        1440 => "täglich".to_owned(),
        value => format!("alle {value} Minuten"),
    }
}

fn cycle_schedule() {
    const VALUES: &[u32] = &[5, 15, 30, 60, 180, 360, 720, 1440];
    let state = STATE.get().expect("tray state");
    let selected = state.selected.load(Ordering::Acquire) as usize;
    let mut sources = state.sources.lock().expect("source state");
    let Some(source) = sources.get_mut(selected) else {
        return;
    };
    let index = VALUES
        .iter()
        .position(|value| *value == source.schedule_minutes)
        .unwrap_or(1);
    source.schedule_minutes = VALUES[(index + 1) % VALUES.len()];
    let label = schedule_label(source.schedule_minutes);
    drop(sources);
    let _ = persist_sources();
    set_message("Zeitplan gespeichert", &label);
    refresh_mode_controls();
}

fn exclude_subfolder(hwnd: HWND) {
    let Some(source) = selected_source() else {
        return;
    };
    let Some(folder) = choose_folder_titled(hwnd, "Unterordner ausschließen") else {
        return;
    };
    let root = normalized_path(&source.path);
    let child = normalized_path(&folder);
    if child == root || !child.starts_with(&(root.clone() + "\\")) {
        set_message(
            "Kein Unterordner",
            "Der Ausschluss muss innerhalb des gewählten Sicherungsordners liegen",
        );
        return;
    }
    let state = STATE.get().expect("tray state");
    let selected = state.selected.load(Ordering::Acquire) as usize;
    let mut sources = state.sources.lock().expect("source state");
    let Some(stored) = sources.get_mut(selected) else {
        return;
    };
    if let Some(index) = stored
        .excluded_subfolders
        .iter()
        .position(|existing| normalized_path(existing) == child)
    {
        stored.excluded_subfolders.remove(index);
        drop(sources);
        let _ = persist_sources();
        set_message(
            "Ausschluss aufgehoben",
            "Medien aus diesem Unterordner werden wieder berücksichtigt",
        );
        refresh_counts();
        refresh_mode_controls();
        return;
    } else {
        stored.excluded_subfolders.push(folder);
    }
    drop(sources);
    let _ = persist_sources();
    set_message(
        "Unterordner ausgeschlossen",
        "Darin enthaltene Medien werden nicht hochgeladen",
    );
    refresh_counts();
    refresh_mode_controls();
}

fn backup_database(hwnd: HWND) {
    if STATE
        .get()
        .is_some_and(|state| state.syncing.load(Ordering::Acquire))
    {
        set_message(
            "Sicherung läuft noch",
            "App-Daten können danach gesichert werden",
        );
        return;
    }
    let Some(destination) = choose_folder_titled(hwnd, "Ziel für die Datensicherung auswählen")
    else {
        return;
    };
    let state = STATE.get().expect("tray state");
    match create_backup(&state.paths, &destination) {
        Ok(folder) => set_message("Datensicherung erstellt", &folder.to_string_lossy()),
        Err(error) => set_message("Datensicherung fehlgeschlagen", &error.to_string()),
    }
}

fn create_backup(paths: &AppPaths, destination: &Path) -> super::AppResult<PathBuf> {
    if paths.database.is_file() {
        let connection = open_database(&paths.database)?;
        connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    }
    let folder = destination.join(format!("GooglePhotosSync-Backup-{}", unix_seconds()));
    fs::create_dir_all(&folder)?;
    for source in [&paths.config, &paths.database, &paths.credentials] {
        if source.is_file() {
            fs::copy(
                source,
                folder.join(source.file_name().ok_or("Ungültiger Dateiname")?),
            )?;
        }
    }
    fs::write(
        folder.join("backup.json"),
        format!("{{\"format\":1,\"created_at\":{}}}", unix_seconds()),
    )?;
    Ok(folder)
}

fn restore_database(hwnd: HWND) {
    if STATE
        .get()
        .is_some_and(|state| state.syncing.load(Ordering::Acquire))
    {
        set_message(
            "Sicherung läuft noch",
            "App-Daten können danach wiederhergestellt werden",
        );
        return;
    }
    let Some(source) = choose_folder_titled(hwnd, "GooglePhotosSync-Backup auswählen") else {
        return;
    };
    let state = STATE.get().expect("tray state");
    match restore_backup(&state.paths, &source) {
        Ok(()) => {
            set_message("Daten wiederhergestellt", "Die App wird neu gestartet");
            if let Ok(executable) = std::env::current_exe() {
                let _ = std::process::Command::new(executable)
                    .args(["restart-after", &std::process::id().to_string()])
                    .spawn();
            }
            unsafe { DestroyWindow(hwnd) };
        }
        Err(error) => set_message("Wiederherstellung fehlgeschlagen", &error.to_string()),
    }
}

fn restore_backup(paths: &AppPaths, source: &Path) -> super::AppResult<()> {
    if !source.join("backup.json").is_file() || !source.join("gphotos-sync.json").is_file() {
        return Err("Dies ist keine gültige Google Photos Sync-Datensicherung.".into());
    }
    fs::create_dir_all(&paths.root)?;
    for name in [
        "gphotos-sync.json",
        "gphotos-rust.db",
        "gphotos-rust.credentials",
    ] {
        let input = source.join(name);
        if input.is_file() {
            fs::copy(input, paths.root.join(name))?;
        }
    }
    Ok(())
}

fn request_update() {
    let state = STATE.get().expect("tray state").clone();
    if state.working.swap(true, Ordering::AcqRel) {
        return;
    }
    state.progress.begin();
    refresh_animation_timer();
    set_message(
        "Suche Aktualisierung",
        "Die GitHub-Veröffentlichungen werden geprüft",
    );
    thread::spawn(move || {
        let result = download_update(&state.paths);
        match result {
            Ok(ready) => state.update_ready.store(ready, Ordering::Release),
            Err(error) => {
                *state.pending_error.lock().expect("background result") = Some(error.to_string());
            }
        }
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(
                state.hwnd.load(Ordering::Acquire) as HWND,
                WM_WORK_FINISHED,
                WORK_UPDATE,
                0,
            )
        };
    });
}

fn download_update(paths: &AppPaths) -> super::AppResult<bool> {
    let http = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .user_agent("google-photos-sync-rs-updater")
        .build()?;
    let response = http
        .get("https://api.github.com/repos/Henner4746/google-photos-sync-rs/releases/latest")
        .send()?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(false);
    }
    let release: serde_json::Value = response.error_for_status()?.json()?;
    let tag = release
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let latest = tag.trim_start_matches('v');
    if !version_is_newer(latest, env!("CARGO_PKG_VERSION")) {
        return Ok(false);
    }
    let asset = release
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .and_then(|assets| {
            assets.iter().find(|asset| {
                asset.get("name").and_then(serde_json::Value::as_str) == Some("gphotos-sync.exe")
            })
        })
        .ok_or("Die neue Version enthält keine Windows-App.")?;
    let url = asset
        .get("browser_download_url")
        .and_then(serde_json::Value::as_str)
        .ok_or("Downloadadresse der neuen Version fehlt.")?;
    let expected = asset
        .get("digest")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.strip_prefix("sha256:"))
        .ok_or("Die neue Version besitzt noch keinen prüfbaren SHA-256-Wert.")?;
    let bytes = http.get(url).send()?.error_for_status()?.bytes()?;
    let actual = hex_encode(&Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(expected) {
        return Err("Die heruntergeladene App hat eine ungültige Prüfsumme.".into());
    }
    let update_dir = paths.root.join("updates");
    fs::create_dir_all(&update_dir)?;
    let helper = update_dir.join(format!("gphotos-sync-{latest}.exe"));
    fs::write(&helper, &bytes)?;
    let target = std::env::current_exe()?;
    if let Err(error) = super::security::verify_update_candidate(&target, &helper) {
        let _ = fs::remove_file(&helper);
        return Err(error);
    }
    std::process::Command::new(helper)
        .arg("apply-update")
        .arg(target)
        .arg(std::process::id().to_string())
        .spawn()?;
    Ok(true)
}

fn version_is_newer(candidate: &str, current: &str) -> bool {
    fn parts(value: &str) -> Vec<u64> {
        value
            .split('.')
            .map(|part| part.split('-').next().unwrap_or("0").parse().unwrap_or(0))
            .collect()
    }
    let mut candidate = parts(candidate);
    let mut current = parts(current);
    let length = candidate.len().max(current.len());
    candidate.resize(length, 0);
    current.resize(length, 0);
    candidate > current
}

fn add_source(hwnd: HWND) {
    let Some(path) = choose_folder(hwnd) else {
        return;
    };
    let state = STATE.get().expect("tray state");
    let mut sources = state.sources.lock().expect("source state");
    let key = normalized_path(&path);
    if sources
        .iter()
        .any(|source| normalized_path(&source.path) == key)
    {
        set_message(
            "Bereits hinzugef\u{00fc}gt",
            "Dieser Ordner wird schon gesichert",
        );
        return;
    }
    let album = path
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Gesicherte Medien")
        .to_owned();
    let kind = detect_media_kind(&path);
    sources.push(SourceSpec {
        album,
        path,
        kind,
        enabled: true,
        schedule_minutes: DEFAULT_SCHEDULE_MINUTES,
        excluded_subfolders: Vec::new(),
        last_successful_sync: 0,
    });
    let index = sources.len() - 1;
    drop(sources);
    state.selected.store(index as u32, Ordering::Release);
    if let Err(error) = persist_sources() {
        set_message("Ordner nicht gespeichert", &error.to_string());
        return;
    }
    refresh_source_list();
    refresh_counts();
    set_message(
        "Ordner hinzugef\u{00fc}gt",
        "Neue Medien werden beim n\u{00e4}chsten Lauf gepr\u{00fc}ft",
    );
}

fn choose_folder(hwnd: HWND) -> Option<PathBuf> {
    choose_folder_titled(
        hwnd,
        "Ordner f\u{00fc}r die Google-Photos-Sicherung ausw\u{00e4}hlen",
    )
}

fn choose_folder_titled(hwnd: HWND, dialog_title: &str) -> Option<PathBuf> {
    unsafe {
        let mut display = [0_u16; 260];
        let title = wide(dialog_title);
        let info = BROWSEINFOW {
            hwndOwner: hwnd,
            pszDisplayName: display.as_mut_ptr(),
            lpszTitle: title.as_ptr(),
            ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX,
            ..Default::default()
        };
        let item = SHBrowseForFolderW(&info);
        if item.is_null() {
            return None;
        }
        let mut path = [0_u16; 32_768];
        let ok = SHGetPathFromIDListW(item, path.as_mut_ptr());
        CoTaskMemFree(item.cast());
        if ok == 0 {
            return None;
        }
        let length = path
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(path.len());
        Some(PathBuf::from(String::from_utf16_lossy(&path[..length])))
    }
}

fn choose_json_file(hwnd: HWND) -> Option<PathBuf> {
    unsafe {
        let mut file = [0_u16; 32_768];
        let filter: Vec<u16> = "Google OAuth JSON\0*.json\0Alle Dateien\0*.*\0\0"
            .encode_utf16()
            .collect();
        let title = wide("Google-OAuth-Datei ausw\u{00e4}hlen");
        let mut dialog = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: hwnd,
            lpstrFilter: filter.as_ptr(),
            lpstrFile: file.as_mut_ptr(),
            nMaxFile: file.len() as u32,
            lpstrTitle: title.as_ptr(),
            Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
            ..Default::default()
        };
        if GetOpenFileNameW(&mut dialog) == 0 {
            return None;
        }
        let length = file
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(file.len());
        Some(PathBuf::from(String::from_utf16_lossy(&file[..length])))
    }
}

fn detect_media_kind(path: &Path) -> MediaKind {
    let images = source_files(path, super::IMAGE_EXTENSIONS)
        .map(|files| files.len())
        .unwrap_or(0);
    let videos = source_files(path, super::VIDEO_EXTENSIONS)
        .map(|files| files.len())
        .unwrap_or(0);
    match (images > 0, videos > 0) {
        (true, false) => MediaKind::Images,
        (false, true) => MediaKind::Videos,
        _ => MediaKind::All,
    }
}

fn save_album_name() {
    let state = STATE.get().expect("tray state");
    let selected = state.selected.load(Ordering::Acquire);
    if selected == NO_SELECTION {
        return;
    }
    let hwnd = state.hwnd.load(Ordering::Acquire) as HWND;
    let value = read_control_text(unsafe { GetDlgItem(hwnd, CMD_ALBUM as i32) });
    let value = value.trim();
    if value.is_empty() {
        set_message(
            "Albumname fehlt",
            "Gib einen Namen f\u{00fc}r neue Medien ein",
        );
        return;
    }
    if value.chars().count() > 500 {
        set_message(
            "Albumname ist zu lang",
            "Google Photos erlaubt h\u{00f6}chstens 500 Zeichen",
        );
        return;
    }
    let mut sources = state.sources.lock().expect("source state");
    let Some(source) = sources.get_mut(selected as usize) else {
        return;
    };
    source.album = value.to_owned();
    drop(sources);
    if let Err(error) = persist_sources() {
        set_message("Albumname nicht gespeichert", &error.to_string());
        return;
    }
    refresh_source_list();
    set_message(
        "Zielalbum gespeichert",
        "Neue Medien aus diesem Ordner verwenden den neuen Namen",
    );
}

fn toggle_selected_source() {
    let state = STATE.get().expect("tray state");
    let selected = state.selected.load(Ordering::Acquire);
    let mut sources = state.sources.lock().expect("source state");
    let Some(source) = sources.get_mut(selected as usize) else {
        return;
    };
    source.enabled = !source.enabled;
    let enabled = source.enabled;
    drop(sources);
    let _ = persist_sources();
    refresh_source_list();
    refresh_counts();
    set_message(
        if enabled {
            "Ordner aktiviert"
        } else {
            "Ordner pausiert"
        },
        if enabled {
            "Er wird beim n\u{00e4}chsten Lauf wieder ber\u{00fc}cksichtigt"
        } else {
            "Seine Medien bleiben in Google Photos erhalten"
        },
    );
}

fn remove_selected_source() {
    let state = STATE.get().expect("tray state");
    let selected = state.selected.load(Ordering::Acquire);
    let mut sources = state.sources.lock().expect("source state");
    if selected == NO_SELECTION || selected as usize >= sources.len() {
        return;
    }
    sources.remove(selected as usize);
    let next = if sources.is_empty() {
        NO_SELECTION
    } else {
        (selected as usize).min(sources.len() - 1) as u32
    };
    state.selected.store(next, Ordering::Release);
    drop(sources);
    let _ = persist_sources();
    refresh_source_list();
    refresh_counts();
    set_message(
        "Ordner entfernt",
        "Lokale Dateien und Google-Photos-Medien wurden nicht gel\u{00f6}scht",
    );
}

fn open_selected_source(hwnd: HWND) {
    if let Some(source) = selected_source() {
        open_path(hwnd, &source.path);
    }
}

fn selected_source() -> Option<SourceSpec> {
    let state = STATE.get()?;
    let selected = state.selected.load(Ordering::Acquire);
    state.sources.lock().ok()?.get(selected as usize).cloned()
}

fn open_path(hwnd: HWND, path: &Path) {
    if !path.exists() {
        set_message("Pfad nicht gefunden", &path.to_string_lossy());
        return;
    }
    open_target(hwnd, &path.to_string_lossy());
}

fn open_target(hwnd: HWND, target: &str) {
    unsafe {
        ShellExecuteW(
            hwnd,
            wide("open").as_ptr(),
            wide(target).as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        );
    }
}

fn toggle_pause() {
    let state = STATE.get().expect("tray state");
    let paused = !state.paused.fetch_xor(true, Ordering::AcqRel);
    set_message(
        if paused {
            "Automatik pausiert"
        } else {
            "Automatik fortgesetzt"
        },
        if paused {
            "Manuelle Sicherungen bleiben verf\u{00fc}gbar"
        } else {
            "Die n\u{00e4}chste Pr\u{00fc}fung folgt innerhalb von 15 Minuten"
        },
    );
    let _ = persist_sources();
    refresh_controls();
}

fn request_sync(manual: bool, dry_run: bool) {
    let state = STATE.get().expect("tray state").clone();
    if state.paused.load(Ordering::Acquire) && !manual {
        state.next_run.store(unix_seconds() + 60, Ordering::Release);
        return;
    }
    if !dry_run && !duplicate_protection_ready(&state) {
        if manual && confirm_no_older_copies(state.hwnd.load(Ordering::Acquire) as HWND) {
            state
                .takeout_not_required_confirmed
                .store(true, Ordering::Release);
            let _ = persist_sources();
        } else {
            set_message(
                "Upload zum Schutz blockiert",
                "Zuerst Takeout importieren oder ausdrücklich bestätigen, dass kein Altbestand existiert",
            );
            return;
        }
    }
    if !state
        .sources
        .lock()
        .expect("source state")
        .iter()
        .any(|source| source.enabled)
    {
        set_message(
            "Kein aktiver Ordner",
            "F\u{00fc}ge einen Ordner hinzu oder aktiviere eine vorhandene Quelle",
        );
        return;
    }
    if state
        .syncing
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    state.dry_run.store(dry_run, Ordering::Release);
    state.progress.begin();
    refresh_animation_timer();
    set_message(
        if dry_run {
            "Testlauf l\u{00e4}uft"
        } else {
            "Sicherung l\u{00e4}uft"
        },
        if dry_run {
            "Neue Medien werden ermittelt, aber nicht hochgeladen"
        } else {
            "Inhalte werden gepr\u{00fc}ft und sicher \u{00fc}bertragen"
        },
    );
    refresh_controls();

    thread::spawn(move || {
        let all_sources = state.sources.lock().expect("source state").clone();
        let now = unix_seconds();
        let sources: Vec<SourceSpec> = all_sources
            .iter()
            .filter(|source| {
                manual
                    || source.last_successful_sync == 0
                    || now
                        >= source.last_successful_sync
                            + i64::from(source.schedule_minutes.max(5)) * 60
            })
            .cloned()
            .collect();
        if sources.is_empty() {
            state
                .next_run
                .store(next_due_time(&all_sources), Ordering::Release);
            state.syncing.store(false, Ordering::Release);
            state.dry_run.store(false, Ordering::Release);
            refresh_animation_timer();
            return;
        }
        let mut paths = state.paths.clone();
        paths.sources = sources.clone();
        paths.takeout_imported_at = match state.takeout_imported_at.load(Ordering::Acquire) {
            0 => None,
            value => Some(value),
        };
        paths.takeout_not_required_confirmed =
            state.takeout_not_required_confirmed.load(Ordering::Acquire);
        let progress = state.progress.clone();
        let result = match SingleInstance::acquire() {
            Ok(_instance) => sync(&paths, &state.logger, dry_run, None, Some(progress)),
            Err(error) => Err(error),
        };
        if result.is_ok() && !dry_run {
            let synced_paths: Vec<String> = sources
                .iter()
                .map(|source| normalized_path(&source.path))
                .collect();
            if let Ok(mut stored) = state.sources.lock() {
                for source in stored.iter_mut() {
                    if synced_paths.contains(&normalized_path(&source.path)) {
                        source.last_successful_sync = unix_seconds();
                    }
                }
            }
            let _ = persist_sources();
        }
        let current_sources = state.sources.lock().expect("source state").clone();
        let refreshed =
            counts(&state.paths, &current_sources).unwrap_or((0, current_sources.len(), 0));
        state.counts_loaded.store(true, Ordering::Release);
        if let Ok(mut view) = state.view.lock() {
            view.protected = refreshed.0;
            view.folders = refreshed.1;
            view.pending = refreshed.2;
            view.last_run = if dry_run {
                "Letzter Testlauf: gerade eben"
            } else {
                "Letzte Sicherung: gerade eben"
            }
            .to_owned();
            match result {
                Ok(()) => {
                    view.status = if dry_run {
                        "Testlauf abgeschlossen"
                    } else {
                        "Alles aktuell"
                    }
                    .to_owned();
                    view.detail = if dry_run {
                        format!("{} neue Medien w\u{00fc}rden gesichert", refreshed.2)
                    } else if refreshed.2 == 0 {
                        "Alle aktiven Ordner sind auf dem neuesten Stand".to_owned()
                    } else {
                        format!("Noch {} neue Medien vorgemerkt", refreshed.2)
                    };
                }
                Err(error) => {
                    view.status = "Aktion erforderlich".to_owned();
                    view.detail = error.to_string();
                    show_error_notification(&view.detail);
                }
            }
        }
        state
            .next_run
            .store(next_due_time(&current_sources), Ordering::Release);
        state.syncing.store(false, Ordering::Release);
        state.dry_run.store(false, Ordering::Release);
        refresh_animation_timer();
        refresh_controls();
        invalidate();
    });
}

fn persist_sources() -> super::AppResult<()> {
    let state = STATE.get().expect("tray state");
    let sources = state.sources.lock().expect("source state").clone();
    let position = *state.window_position.lock().expect("window position");
    save_config(
        &state.paths.config,
        &sources,
        position,
        state.paused.load(Ordering::Acquire),
        state.onboarding_completed.load(Ordering::Acquire),
        state.autostart_enabled.load(Ordering::Acquire),
        state.auto_update.load(Ordering::Acquire),
        match state.takeout_imported_at.load(Ordering::Acquire) {
            0 => None,
            value => Some(value),
        },
        state.takeout_not_required_confirmed.load(Ordering::Acquire),
    )
}

fn refresh_source_list() {
    let Some(state) = STATE.get() else {
        return;
    };
    let hwnd = state.hwnd.load(Ordering::Acquire) as HWND;
    if hwnd.is_null() {
        return;
    }
    let sources = state.sources.lock().expect("source state").clone();
    let selected = state.selected.load(Ordering::Acquire);
    unsafe {
        let list = GetDlgItem(hwnd, CMD_SOURCES as i32);
        SendMessageW(list, LB_RESETCONTENT, 0, 0);
        for source in &sources {
            let label = wide(&source.album);
            SendMessageW(list, LB_ADDSTRING, 0, label.as_ptr() as LPARAM);
        }
        if sources.is_empty() {
            ShowWindow(list, SW_HIDE);
            state.selected.store(NO_SELECTION, Ordering::Release);
        } else {
            ShowWindow(list, SW_SHOW);
            let selected = (selected as usize).min(sources.len() - 1);
            state.selected.store(selected as u32, Ordering::Release);
            SendMessageW(list, LB_SETCURSEL, selected, 0);
        }
    }
    refresh_source_controls();
    refresh_mode_controls();
    invalidate();
}

fn update_selection() {
    let state = STATE.get().expect("tray state");
    let hwnd = state.hwnd.load(Ordering::Acquire) as HWND;
    let selected =
        unsafe { SendMessageW(GetDlgItem(hwnd, CMD_SOURCES as i32), LB_GETCURSEL, 0, 0) };
    state.selected.store(
        if selected < 0 {
            NO_SELECTION
        } else {
            selected as u32
        },
        Ordering::Release,
    );
    refresh_source_controls();
    refresh_mode_controls();
    invalidate();
}

fn refresh_source_controls() {
    let Some(state) = STATE.get() else {
        return;
    };
    let hwnd = state.hwnd.load(Ordering::Acquire) as HWND;
    let source = selected_source();
    unsafe {
        let edit = GetDlgItem(hwnd, CMD_ALBUM as i32);
        ShowWindow(edit, if source.is_some() { SW_SHOW } else { SW_HIDE });
        SetWindowTextW(
            edit,
            wide(source.as_ref().map_or("", |source| &source.album)).as_ptr(),
        );
        EnableWindow(edit, source.is_some().into());
        for id in [
            CMD_SAVE_ALBUM,
            CMD_OPEN_SOURCE,
            CMD_TOGGLE_SOURCE,
            CMD_REMOVE_SOURCE,
        ] {
            let control = GetDlgItem(hwnd, id as i32);
            ShowWindow(control, if source.is_some() { SW_SHOW } else { SW_HIDE });
            EnableWindow(control, source.is_some().into());
        }
        SetWindowTextW(
            GetDlgItem(hwnd, CMD_TOGGLE_SOURCE as i32),
            wide(if source.as_ref().is_some_and(|source| source.enabled) {
                "Pausieren"
            } else {
                "Aktivieren"
            })
            .as_ptr(),
        );
    }
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
        SetWindowTextW(
            GetDlgItem(hwnd, CMD_SYNC as i32),
            wide(if syncing {
                "Sicherung l\u{00e4}uft"
            } else {
                "Jetzt sichern"
            })
            .as_ptr(),
        );
        SetWindowTextW(
            GetDlgItem(hwnd, CMD_PAUSE as i32),
            wide(if paused {
                "Fortsetzen"
            } else {
                "Automatik pausieren"
            })
            .as_ptr(),
        );
        EnableWindow(GetDlgItem(hwnd, CMD_SYNC as i32), (!syncing).into());
        EnableWindow(GetDlgItem(hwnd, CMD_DRY_RUN as i32), (!syncing).into());
        InvalidateRect(hwnd, null(), 0);
    }
    refresh_source_controls();
    refresh_mode_controls();
}

fn refresh_counts() {
    let state = STATE.get().expect("tray state");
    let sources = state.sources.lock().expect("source state").clone();
    if let Ok((protected, folders, pending)) = counts(&state.paths, &sources)
        && let Ok(mut view) = state.view.lock()
    {
        view.protected = protected;
        view.folders = folders;
        view.pending = pending;
        state.counts_loaded.store(true, Ordering::Release);
    }
    invalidate();
}

fn counts(paths: &AppPaths, sources: &[SourceSpec]) -> super::AppResult<(i64, usize, usize)> {
    let connection = open_database(&paths.database)?;
    let mut protected = 0_i64;
    let mut pending = 0_usize;
    for source in sources.iter().filter(|source| source.enabled) {
        if !source.path.is_dir() {
            continue;
        }
        for path in source_files_for_source(source)? {
            let metadata = fs::metadata(&path)?;
            let size = i64::try_from(metadata.len())?;
            let modified = super::modified_ns(&metadata)?;
            let path_text = path.to_string_lossy();
            let known = current_record(&connection, &source.album, &path_text, size, modified)?
                .is_some_and(|(_, _, state)| trusted_state(&state));
            if known {
                protected += 1;
            } else {
                pending += 1;
            }
        }
    }
    Ok((protected, sources.len(), pending))
}

fn set_message(status: &str, detail: &str) {
    if let Some(state) = STATE.get() {
        if let Ok(mut view) = state.view.lock() {
            view.status = status.to_owned();
            view.detail = detail.to_owned();
        }
        invalidate();
    }
}

fn normalized_path(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn next_due_time(sources: &[SourceSpec]) -> i64 {
    let now = unix_seconds();
    sources
        .iter()
        .filter(|source| source.enabled)
        .map(|source| {
            if source.last_successful_sync == 0 {
                now
            } else {
                source.last_successful_sync + i64::from(source.schedule_minutes.max(5)) * 60
            }
        })
        .min()
        .unwrap_or(now + i64::from(DEFAULT_SCHEDULE_MINUTES) * 60)
        .max(now)
}

fn initial_sync_is_due() -> bool {
    STATE
        .get()
        .is_some_and(|state| sync_is_due_at(state.next_run.load(Ordering::Acquire), unix_seconds()))
}

fn sync_is_due_at(next_run: i64, now: i64) -> bool {
    next_run <= now + 1
}

fn refresh_animation_timer() {
    let Some(state) = STATE.get() else { return };
    let hwnd = state.hwnd.load(Ordering::Acquire) as HWND;
    if hwnd.is_null() {
        return;
    }
    unsafe {
        if state.syncing.load(Ordering::Acquire) || state.working.load(Ordering::Acquire) {
            SetTimer(hwnd, TIMER_ANIMATION, ANIMATION_INTERVAL_MS, None);
        } else {
            KillTimer(hwnd, TIMER_ANIMATION);
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    if bytes as f64 >= MB {
        format!("{:.1} MB", bytes as f64 / MB)
    } else if bytes as f64 >= KB {
        format!("{:.0} KB", bytes as f64 / KB)
    } else {
        format!("{bytes} B")
    }
}

fn read_control_text(hwnd: HWND) -> String {
    unsafe {
        let length = GetWindowTextLengthW(hwnd);
        let mut buffer = vec![0_u16; length as usize + 1];
        let read = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..read.max(0) as usize])
    }
}

fn invalidate() {
    if let Some(state) = STATE.get() {
        let raw = state.hwnd.load(Ordering::Acquire);
        if raw != 0 && unsafe { IsWindowVisible(raw as HWND) } != 0 {
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
            state.dry_run.load(Ordering::Acquire),
            state.selected.load(Ordering::Acquire) != NO_SELECTION,
            state.animation.load(Ordering::Relaxed),
            state.next_run.load(Ordering::Acquire),
            state.setup_mode.load(Ordering::Acquire),
            state.paths.credentials.is_file(),
            !state.sources.lock().expect("source state").is_empty(),
            duplicate_protection_ready(state),
            state.autostart_enabled.load(Ordering::Acquire),
            state.settings_mode.load(Ordering::Acquire),
        );
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

const fn width(rect: RECT) -> i32 {
    rect.right - rect.left
}

const fn height(rect: RECT) -> i32 {
    rect.bottom - rect.top
}

fn screen_point(lparam: LPARAM) -> (i32, i32) {
    (
        (lparam as u32 & 0xffff) as i16 as i32,
        ((lparam as u32 >> 16) & 0xffff) as i16 as i32,
    )
}

const fn gray(value: u32) -> u32 {
    value | (value << 8) | (value << 16)
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

#[cfg(test)]
mod tests {
    use super::sync_is_due_at;

    #[test]
    fn startup_sync_only_runs_when_a_folder_is_due() {
        assert!(sync_is_due_at(1_000, 1_000));
        assert!(sync_is_due_at(1_001, 1_000));
        assert!(!sync_is_due_at(1_002, 1_000));
        assert!(!sync_is_due_at(1_900, 1_000));
    }
}
