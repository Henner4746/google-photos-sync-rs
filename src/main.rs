#![cfg_attr(not(windows), allow(dead_code))]
#![windows_subsystem = "windows"]

#[cfg(not(windows))]
compile_error!("gphotos-sync is intentionally Windows-only because credentials use DPAPI.");

use reqwest::Method;
use reqwest::blocking::{Client, Response};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::env;
use std::error::Error;
use std::ffi::{OsStr, c_void};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};

mod tray;

type AppResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const GOOGLE_API: &str = "https://photoslibrary.googleapis.com/v1";
const GOOGLE_UPLOADS: &str = "https://photoslibrary.googleapis.com/v1/uploads";
const GOOGLE_TOKEN: &str = "https://oauth2.googleapis.com/token";
const TASK_NAME: &str = "Google-Photos-Sync";

const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "heic", "heif", "tif", "tiff", "bmp",
];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "m4v", "mov", "mkv", "avi", "webm"];

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SourceSpec {
    album: String,
    path: PathBuf,
    kind: MediaKind,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum MediaKind {
    Images,
    Videos,
}

impl SourceSpec {
    fn extensions(&self) -> &'static [&'static str] {
        match self.kind {
            MediaKind::Images => IMAGE_EXTENSIONS,
            MediaKind::Videos => VIDEO_EXTENSIONS,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct AppConfig {
    sources: Vec<SourceSpec>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Credentials {
    client_id: String,
    client_secret: String,
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct OAuthClientFile {
    installed: OAuthDesktopClient,
}

#[derive(Debug, Deserialize)]
struct OAuthDesktopClient {
    client_id: String,
    client_secret: String,
    #[serde(default = "default_auth_uri")]
    auth_uri: String,
    #[serde(default = "default_token_uri")]
    token_uri: String,
}

#[derive(Clone, Debug)]
struct FileRecord {
    path: PathBuf,
    size: i64,
    mtime_ns: i64,
    sha256: String,
    upload_name: String,
}

#[derive(Debug)]
struct Candidate {
    primary: FileRecord,
    aliases: Vec<FileRecord>,
}

#[derive(Default)]
struct SyncStats {
    scanned: usize,
    unchanged: usize,
    recovered_remote: usize,
    content_duplicates: usize,
    planned: usize,
    uploaded: usize,
    failed: usize,
}

#[derive(Clone, Copy)]
struct SyncRunOptions {
    dry_run: bool,
    limit: Option<usize>,
}

struct Logger {
    file: Mutex<File>,
}

impl Logger {
    fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }

    fn log(&self, level: &str, message: impl AsRef<str>) {
        let line = format!("{} | {} | {}", unix_seconds(), level, message.as_ref());
        println!("{line}");
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{line}");
        }
    }
}

#[repr(C)]
struct DataBlob {
    cb_data: u32,
    pb_data: *mut u8,
}

#[link(name = "crypt32")]
unsafe extern "system" {
    fn CryptProtectData(
        input: *mut DataBlob,
        description: *const u16,
        entropy: *mut DataBlob,
        reserved: *mut c_void,
        prompt: *mut c_void,
        flags: u32,
        output: *mut DataBlob,
    ) -> i32;
    fn CryptUnprotectData(
        input: *mut DataBlob,
        description: *mut *mut u16,
        entropy: *mut DataBlob,
        reserved: *mut c_void,
        prompt: *mut c_void,
        flags: u32,
        output: *mut DataBlob,
    ) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
    fn CreateMutexW(attributes: *mut c_void, initial_owner: i32, name: *const u16) -> *mut c_void;
    fn GetLastError() -> u32;
    fn CloseHandle(object: *mut c_void) -> i32;
}

struct SingleInstance {
    handle: *mut c_void,
}

impl SingleInstance {
    fn acquire() -> AppResult<Self> {
        Self::acquire_named(r"Local\GooglePhotosSyncWorker")
    }

    fn acquire_tray() -> AppResult<Self> {
        Self::acquire_named(r"Local\GooglePhotosSyncTray")
    }

    fn acquire_named(name: &str) -> AppResult<Self> {
        let mut name: Vec<u16> = OsStr::new(name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null_mut(), 1, name.as_mut_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        if unsafe { GetLastError() } == 183 {
            unsafe {
                CloseHandle(handle);
            }
            return Err("Eine Synchronisierung laeuft bereits.".into());
        }
        Ok(Self { handle })
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

fn main() {
    if let Err(error) = real_main() {
        eprintln!("FEHLER: {error}");
        std::process::exit(1);
    }
}

fn real_main() -> AppResult<()> {
    let paths = AppPaths::discover()?;
    let logger = Logger::open(&paths.log)?;
    let args: Vec<String> = env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("sync");

    match command {
        "sync" => {
            let dry_run = args.iter().any(|arg| arg == "--dry-run");
            let limit = option_usize(&args, "--limit")?;
            let _instance = SingleInstance::acquire()?;
            sync(&paths, &logger, dry_run, limit)
        }
        "import-takeout" => {
            let input = args
                .get(1)
                .ok_or("Aufruf: gphotos-sync import-takeout <Ordner>")?;
            import_takeout(&paths, &logger, Path::new(input))
        }
        "protect-credentials" => {
            let input = args
                .get(1)
                .ok_or("Aufruf: gphotos-sync protect-credentials <credentials.json>")?;
            protect_credentials(&paths, Path::new(input))
        }
        "authorize" => {
            let input = args
                .get(1)
                .ok_or("Aufruf: gphotos-sync authorize <oauth-client.json>")?;
            authorize(&paths, Path::new(input))
        }
        "install" => install(&paths),
        "uninstall" => uninstall(),
        "status" => show_status(&paths),
        "tray" => tray::run(
            paths,
            logger,
            args.iter().any(|argument| argument == "--show"),
            !args.iter().any(|argument| argument == "--no-sync"),
        ),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(format!("Unbekannter Befehl: {other}").into()),
    }
}

fn print_help() {
    println!("gphotos-sync - schlanke Google-Fotos-Sicherung fuer Screenshots und AMD-Clips");
    println!();
    println!("  gphotos-sync sync [--dry-run]");
    println!("  gphotos-sync sync --limit <Anzahl je Album>");
    println!("  gphotos-sync import-takeout <entpackter Takeout-Ordner>");
    println!("  gphotos-sync protect-credentials <credentials.json>");
    println!("  gphotos-sync authorize <oauth-client.json>");
    println!("  gphotos-sync install");
    println!("  gphotos-sync uninstall");
    println!("  gphotos-sync status");
    println!("  gphotos-sync tray");
}

#[derive(Clone)]
struct AppPaths {
    credentials: PathBuf,
    database: PathBuf,
    log: PathBuf,
    config: PathBuf,
    sources: Vec<SourceSpec>,
}

impl AppPaths {
    fn discover() -> AppResult<Self> {
        let program_data = env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        let legacy_root = program_data.join("pc-backup");
        let root = env::var_os("GPHOTOS_SYNC_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                (legacy_root.join("gphotos-rust.db").is_file()
                    || legacy_root.join("gphotos-rust.credentials").is_file())
                .then_some(legacy_root)
            })
            .or_else(|| {
                env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .map(|path| path.join("GooglePhotosSync"))
            })
            .unwrap_or_else(|| program_data.join("GooglePhotosSync"));
        let config = root.join("gphotos-sync.json");
        let sources = load_or_create_config(&config)?.sources;
        Ok(Self {
            credentials: root.join("gphotos-rust.credentials"),
            database: root.join("gphotos-rust.db"),
            log: root.join("logs").join("gphotos").join("gphotos-rust.log"),
            config,
            sources,
        })
    }
}

fn install(paths: &AppPaths) -> AppResult<()> {
    let source = env::current_exe()?;
    let install_root = paths
        .database
        .parent()
        .ok_or("Installationsordner konnte nicht bestimmt werden.")?;
    fs::create_dir_all(install_root)?;
    let destination = install_root.join("gphotos-sync.exe");
    if source.canonicalize()? != destination.canonicalize().unwrap_or_default() {
        fs::copy(&source, &destination)?;
    }
    let action = format!("\"{}\" tray", destination.display());
    let status = Command::new("schtasks.exe")
        .args([
            "/Create", "/TN", TASK_NAME, "/SC", "ONLOGON", "/RL", "LIMITED", "/TR",
        ])
        .arg(action)
        .arg("/F")
        .status()?;
    if !status.success() {
        return Err("Autostart-Aufgabe konnte nicht erstellt werden.".into());
    }
    println!("Installiert: {}", destination.display());
    println!("Autostart: {TASK_NAME}");
    Ok(())
}

fn uninstall() -> AppResult<()> {
    let status = Command::new("schtasks.exe")
        .args(["/Delete", "/TN", TASK_NAME, "/F"])
        .status()?;
    if !status.success() {
        return Err("Autostart-Aufgabe konnte nicht entfernt werden.".into());
    }
    println!("Autostart wurde entfernt. Lokale Daten bleiben erhalten.");
    Ok(())
}

fn write_example_config(path: &Path) -> AppResult<()> {
    let profile = env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Users\Public"));
    let config = AppConfig {
        sources: vec![
            SourceSpec {
                album: "Screenshots".to_owned(),
                path: profile.join("Pictures").join("Screenshots"),
                kind: MediaKind::Images,
            },
            SourceSpec {
                album: "AMD-Clips".to_owned(),
                path: profile.join("Videos").join("Radeon ReLive"),
                kind: MediaKind::Videos,
            },
        ],
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&config)?)?;
    Ok(())
}

fn load_or_create_config(path: &Path) -> AppResult<AppConfig> {
    if path.is_file() {
        return Ok(serde_json::from_slice(&fs::read(path)?)?);
    }
    write_example_config(path)?;
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn load_credentials(path: &Path) -> AppResult<Credentials> {
    let encoded = fs::read_to_string(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "Gesch\u{00fc}tzte Zugangsdaten fehlen ({}). Zuerst protect-credentials ausf\u{00fc}hren.",
                path.display()
            ),
        )
    })?;
    let protected = hex_decode(encoded.trim())?;
    let plaintext = dpapi_unprotect(&protected)?;
    Ok(serde_json::from_slice(&plaintext)?)
}

fn protect_credentials(paths: &AppPaths, input: &Path) -> AppResult<()> {
    let plaintext = fs::read(input)?;
    let _: Credentials = serde_json::from_slice(&plaintext)?;
    let protected = dpapi_protect(&plaintext)?;
    if let Some(parent) = paths.credentials.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&paths.credentials, hex_encode(&protected))?;
    println!(
        "Zugangsdaten wurden mit Windows DPAPI gesch\u{00fc}tzt: {}",
        paths.credentials.display()
    );
    Ok(())
}

fn default_auth_uri() -> String {
    "https://accounts.google.com/o/oauth2/v2/auth".to_owned()
}

fn default_token_uri() -> String {
    GOOGLE_TOKEN.to_owned()
}

fn authorize(paths: &AppPaths, input: &Path) -> AppResult<()> {
    let client_file: OAuthClientFile = serde_json::from_slice(&fs::read(input)?)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let state = oauth_state()?;
    let scope = "https://www.googleapis.com/auth/photoslibrary.appendonly https://www.googleapis.com/auth/photoslibrary.readonly.appcreateddata";
    let authorization_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&state={}",
        client_file.installed.auth_uri,
        url_encode(&client_file.installed.client_id),
        url_encode(&redirect_uri),
        url_encode(scope),
        url_encode(&state),
    );
    Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", &authorization_url])
        .spawn()?;
    println!("Google-Anmeldung wurde im Browser ge\u{00f6}ffnet.");

    let (mut stream, _) = listener.accept()?;
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    let mut request = [0_u8; 16 * 1024];
    let length = stream.read(&mut request)?;
    let request = String::from_utf8_lossy(&request[..length]);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or("Ung\u{00fc}ltige OAuth-Antwort.")?;
    let query = target.split_once('?').map(|(_, query)| query).unwrap_or("");
    let parameters: HashMap<String, String> = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.to_owned(), url_decode(value)))
        .collect();
    let valid_state = parameters.get("state").is_some_and(|value| value == &state);
    let code = parameters.get("code").filter(|_| valid_state).cloned();
    let (status, body) = if code.is_some() {
        (
            "200 OK",
            "Google Photos Sync ist verbunden. Dieses Fenster kann geschlossen werden.",
        )
    } else {
        (
            "400 Bad Request",
            "Die Google-Anmeldung konnte nicht abgeschlossen werden.",
        )
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    let code = code.ok_or("OAuth-Antwort enthielt keinen g\u{00fc}ltigen Code.")?;

    let value: Value = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()?
        .post(&client_file.installed.token_uri)
        .form(&[
            ("code", code.as_str()),
            ("client_id", client_file.installed.client_id.as_str()),
            (
                "client_secret",
                client_file.installed.client_secret.as_str(),
            ),
            ("redirect_uri", redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()?
        .error_for_status()?
        .json()?;
    let refresh_token = value.get("refresh_token").and_then(Value::as_str).ok_or(
        "Google hat kein refresh_token geliefert. Zugriff entfernen und erneut verbinden.",
    )?;
    let credentials = Credentials {
        client_id: client_file.installed.client_id,
        client_secret: client_file.installed.client_secret,
        refresh_token: refresh_token.to_owned(),
    };
    let protected = dpapi_protect(&serde_json::to_vec(&credentials)?)?;
    if let Some(parent) = paths.credentials.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&paths.credentials, hex_encode(&protected))?;
    println!("Google Photos wurde verbunden. Die OAuth-Datei kann gel\u{00f6}scht werden.");
    Ok(())
}

fn url_encode(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            result.push(byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

fn oauth_state() -> AppResult<String> {
    let mut bytes = [0_u8; 32];
    let status = unsafe {
        BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        return Err(format!("Sicherer Zufallswert konnte nicht erzeugt werden: {status}").into());
    }
    Ok(hex_encode(&bytes))
}

fn url_decode(value: &str) -> String {
    let mut result = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Ok(high), Ok(low)) =
                (hex_nibble(bytes[index + 1]), hex_nibble(bytes[index + 2]))
        {
            result.push((high << 4) | low);
            index += 3;
            continue;
        }
        result.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

fn dpapi_protect(plaintext: &[u8]) -> AppResult<Vec<u8>> {
    let mut input_bytes = plaintext.to_vec();
    let mut input = DataBlob {
        cb_data: u32::try_from(input_bytes.len())?,
        pb_data: input_bytes.as_mut_ptr(),
    };
    let mut output = DataBlob {
        cb_data: 0,
        pb_data: std::ptr::null_mut(),
    };
    let description: Vec<u16> = OsStr::new("Google Photos Sync credentials")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let ok = unsafe {
        CryptProtectData(
            &mut input,
            description.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            1,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pb_data, output.cb_data as usize) }.to_vec();
    unsafe {
        LocalFree(output.pb_data.cast());
    }
    Ok(bytes)
}

fn dpapi_unprotect(protected: &[u8]) -> AppResult<Vec<u8>> {
    let mut input_bytes = protected.to_vec();
    let mut input = DataBlob {
        cb_data: u32::try_from(input_bytes.len())?,
        pb_data: input_bytes.as_mut_ptr(),
    };
    let mut output = DataBlob {
        cb_data: 0,
        pb_data: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &mut input,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            1,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error().into());
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pb_data, output.cb_data as usize) }.to_vec();
    unsafe {
        LocalFree(output.pb_data.cast());
    }
    Ok(bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0f) as usize] as char);
    }
    result
}

fn hex_decode(text: &str) -> AppResult<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return Err("Ungueltige DPAPI-Datei.".into());
    }
    let bytes = text.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        result.push((high << 4) | low);
    }
    Ok(result)
}

fn hex_nibble(byte: u8) -> AppResult<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("Ungueltige DPAPI-Datei.".into()),
    }
}

struct GoogleClient {
    http: Client,
    credentials: Credentials,
    access_token: String,
    token_acquired: Instant,
}

impl GoogleClient {
    fn connect(credentials: Credentials) -> AppResult<Self> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(900))
            .user_agent("google-photos-sync-rs/1.0")
            .build()?;
        let mut client = Self {
            http,
            credentials,
            access_token: String::new(),
            token_acquired: Instant::now(),
        };
        client.refresh_access_token()?;
        Ok(client)
    }

    fn refresh_access_token(&mut self) -> AppResult<()> {
        let response = self
            .http
            .post(GOOGLE_TOKEN)
            .form(&[
                ("client_id", self.credentials.client_id.as_str()),
                ("client_secret", self.credentials.client_secret.as_str()),
                ("refresh_token", self.credentials.refresh_token.as_str()),
                ("grant_type", "refresh_token"),
            ])
            .send()?;
        let status = response.status();
        let value: Value = response.json()?;
        if !status.is_success() {
            return Err(format!(
                "OAuth-Aktualisierung fehlgeschlagen ({status}): {}",
                safe_google_error(&value)
            )
            .into());
        }
        self.access_token = value
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or("Google hat kein access_token geliefert.")?
            .to_owned();
        self.token_acquired = Instant::now();
        Ok(())
    }

    fn ensure_fresh_access_token(&mut self) -> AppResult<()> {
        if self.token_acquired.elapsed() >= Duration::from_secs(45 * 60) {
            self.refresh_access_token()?;
        }
        Ok(())
    }

    fn json_request(
        &mut self,
        method: Method,
        url: &str,
        query: &[(String, String)],
        body: Option<&Value>,
    ) -> AppResult<Value> {
        let mut refreshed = false;
        for attempt in 0..6 {
            let mut request = self
                .http
                .request(method.clone(), url)
                .bearer_auth(&self.access_token)
                .query(query);
            if let Some(body) = body {
                request = request.json(body);
            }
            let response = request.send()?;
            if response.status().as_u16() == 401 && !refreshed {
                self.refresh_access_token()?;
                refreshed = true;
                continue;
            }
            if retryable(response.status().as_u16()) && attempt < 5 {
                wait_before_retry(&response, attempt);
                continue;
            }
            let status = response.status();
            let value: Value = response.json()?;
            if !status.is_success() {
                return Err(format!(
                    "Google Photos API ({status}): {}",
                    safe_google_error(&value)
                )
                .into());
            }
            return Ok(value);
        }
        Err("Google Photos API blieb nach mehreren Versuchen nicht erreichbar.".into())
    }

    fn find_or_create_album(&mut self, title: &str) -> AppResult<String> {
        let mut page_token = String::new();
        loop {
            let mut query = vec![("pageSize".to_owned(), "50".to_owned())];
            if !page_token.is_empty() {
                query.push(("pageToken".to_owned(), page_token.clone()));
            }
            let value =
                self.json_request(Method::GET, &format!("{GOOGLE_API}/albums"), &query, None)?;
            if let Some(albums) = value.get("albums").and_then(Value::as_array) {
                for album in albums {
                    if album.get("title").and_then(Value::as_str) == Some(title) {
                        return album
                            .get("id")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                            .ok_or_else(|| "Google-Album ohne ID.".into());
                    }
                }
            }
            page_token = value
                .get("nextPageToken")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if page_token.is_empty() {
                break;
            }
        }

        let body = json!({ "album": { "title": title } });
        let value = self.json_request(
            Method::POST,
            &format!("{GOOGLE_API}/albums"),
            &[],
            Some(&body),
        )?;
        value
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("Google hat fuer Album {title} keine ID geliefert.").into())
    }

    fn album_media(&mut self, album_id: &str) -> AppResult<HashMap<String, String>> {
        let mut result = HashMap::new();
        let mut page_token = String::new();
        loop {
            let mut body = json!({ "albumId": album_id, "pageSize": 100 });
            if !page_token.is_empty() {
                body["pageToken"] = Value::String(page_token.clone());
            }
            let value = self.json_request(
                Method::POST,
                &format!("{GOOGLE_API}/mediaItems:search"),
                &[],
                Some(&body),
            )?;
            if let Some(items) = value.get("mediaItems").and_then(Value::as_array) {
                for item in items {
                    if let Some(filename) = item.get("filename").and_then(Value::as_str) {
                        let id = item.get("id").and_then(Value::as_str).unwrap_or_default();
                        result.insert(filename.to_owned(), id.to_owned());
                    }
                }
            }
            page_token = value
                .get("nextPageToken")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if page_token.is_empty() {
                break;
            }
        }
        Ok(result)
    }

    fn raw_upload(&self, path: &Path, mime: &str, size: u64) -> AppResult<String> {
        for attempt in 0..6 {
            let file = File::open(path)?;
            let response = self
                .http
                .post(GOOGLE_UPLOADS)
                .bearer_auth(&self.access_token)
                .header("Content-Type", "application/octet-stream")
                .header("X-Goog-Upload-Content-Type", mime)
                .header("X-Goog-Upload-Protocol", "raw")
                .header("Content-Length", size)
                .body(file)
                .send()?;
            if response.status().as_u16() == 401 {
                return Err("Der Google-Zugang ist waehrend des Uploads abgelaufen.".into());
            }
            if retryable(response.status().as_u16()) && attempt < 5 {
                wait_before_retry(&response, attempt);
                continue;
            }
            let status = response.status();
            let text = response.text()?;
            if !status.is_success() {
                return Err(format!(
                    "Upload-Token fehlgeschlagen ({status}): {}",
                    truncate(&text, 300)
                )
                .into());
            }
            if text.trim().is_empty() {
                return Err("Google hat einen leeren Upload-Token geliefert.".into());
            }
            return Ok(text);
        }
        Err("Upload blieb nach mehreren Versuchen nicht erreichbar.".into())
    }

    fn batch_create(&mut self, album_id: &str, items: &[(String, String)]) -> AppResult<Value> {
        let new_items: Vec<Value> = items
            .iter()
            .map(|(token, filename)| {
                json!({
                    "description": "",
                    "simpleMediaItem": {
                        "uploadToken": token,
                        "fileName": filename
                    }
                })
            })
            .collect();
        let body = json!({ "albumId": album_id, "newMediaItems": new_items });
        self.json_request(
            Method::POST,
            &format!("{GOOGLE_API}/mediaItems:batchCreate"),
            &[],
            Some(&body),
        )
    }
}

fn retryable(status: u16) -> bool {
    status == 429 || status >= 500
}

fn wait_before_retry(response: &Response, attempt: usize) {
    let header_seconds = response
        .headers()
        .get("Retry-After")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let fallback = if response.status().as_u16() == 429 {
        30_u64.saturating_mul(2_u64.pow((attempt as u32).min(2)))
    } else {
        2_u64.pow((attempt as u32).min(5))
    };
    let seconds = header_seconds.unwrap_or(fallback).clamp(1, 120);
    thread::sleep(Duration::from_secs(seconds));
}

fn safe_google_error(value: &Value) -> String {
    value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .or_else(|| value.get("error_description").and_then(Value::as_str))
        .unwrap_or("Unbekannter Google-Fehler")
        .to_owned()
}

fn sync(
    paths: &AppPaths,
    logger: &Logger,
    dry_run: bool,
    limit_per_source: Option<usize>,
) -> AppResult<()> {
    logger.log(
        "START",
        if dry_run {
            "Rust-Synchronisierung (Testlauf, keine Uploads)"
        } else {
            "Rust-Synchronisierung"
        },
    );
    let mut database = open_database(&paths.database)?;
    let mut google: Option<GoogleClient> = None;
    let mut total = SyncStats::default();
    let options = SyncRunOptions {
        dry_run,
        limit: limit_per_source,
    };

    for source in &paths.sources {
        let source_path = source.path.as_path();
        if !source_path.is_dir() {
            logger.log(
                "WARN",
                format!("Quelle fehlt, uebersprungen: {}", source_path.display()),
            );
            continue;
        }
        if let Some(stats) = fully_known_source(&database, source)? {
            logger.log(
                "SCHNELLPRUEFUNG",
                format!(
                    "{}: {} unveraenderte Dateien, kein Netzwerkzugriff noetig",
                    source.album, stats.scanned
                ),
            );
            total.scanned += stats.scanned;
            total.unchanged += stats.unchanged;
            continue;
        }
        if google.is_none() {
            let credentials = load_credentials(&paths.credentials)?;
            google = Some(GoogleClient::connect(credentials)?);
        }
        let google = google.as_mut().expect("Google client initialized");
        logger.log("INFO", format!("Pruefe Album {}", source.album));
        let album_id = google.find_or_create_album(&source.album)?;
        let remote = google.album_media(&album_id)?;
        logger.log(
            "INFO",
            format!(
                "{}: {} von dieser App sichtbare Google-Fotos-Eintraege",
                source.album,
                remote.len()
            ),
        );
        let stats = sync_source(
            &mut database,
            google,
            logger,
            source,
            &album_id,
            &remote,
            options,
        )?;
        logger.log(
            "ERGEBNIS",
            format!(
                "{}: Dateien={}, unveraendert={}, remote_erkannt={}, Inhaltsduplikate={}, geplant={}, hochgeladen={}, Fehler={}",
                source.album,
                stats.scanned,
                stats.unchanged,
                stats.recovered_remote,
                stats.content_duplicates,
                stats.planned,
                stats.uploaded,
                stats.failed
            ),
        );
        total.scanned += stats.scanned;
        total.unchanged += stats.unchanged;
        total.recovered_remote += stats.recovered_remote;
        total.content_duplicates += stats.content_duplicates;
        total.planned += stats.planned;
        total.uploaded += stats.uploaded;
        total.failed += stats.failed;
    }

    logger.log(
        "ENDE",
        format!(
            "Dateien={}, geplant={}, hochgeladen={}, nicht_erneut_hochgeladen={}, Fehler={}",
            total.scanned,
            total.planned,
            total.uploaded,
            total.unchanged + total.recovered_remote + total.content_duplicates,
            total.failed
        ),
    );
    if total.failed > 0 {
        return Err(format!("{} Datei(en) konnten nicht gesichert werden.", total.failed).into());
    }
    Ok(())
}

fn sync_source(
    database: &mut Connection,
    google: &mut GoogleClient,
    logger: &Logger,
    source: &SourceSpec,
    album_id: &str,
    remote: &HashMap<String, String>,
    options: SyncRunOptions,
) -> AppResult<SyncStats> {
    let mut stats = SyncStats::default();
    let files = source_files(&source.path, source.extensions())?;
    stats.scanned = files.len();
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut candidate_by_hash: HashMap<String, usize> = HashMap::new();

    for path in files {
        let metadata = fs::metadata(&path)?;
        let size = i64::try_from(metadata.len())?;
        let mtime_ns = modified_ns(&metadata)?;
        let path_text = path.to_string_lossy();

        if let Some((hash, upload_name, state)) =
            current_record(database, &source.album, &path_text, size, mtime_ns)?
        {
            let still_known = trusted_state(&state)
                || (!upload_name.is_empty() && remote.contains_key(&upload_name));
            if still_known {
                stats.unchanged += 1;
                continue;
            }
            logger.log(
                "INFO",
                format!(
                    "Remote-Eintrag wird erneut geprueft: {} ({})",
                    path.display(),
                    &hash[..12.min(hash.len())]
                ),
            );
        }

        let hash = sha256_file(&path)?;
        let upload_name = content_addressed_name(&path, &hash);
        let record = FileRecord {
            path: path.clone(),
            size,
            mtime_ns,
            sha256: hash.clone(),
            upload_name: upload_name.clone(),
        };

        if let Some((known_name, media_id, state)) = record_by_hash(database, &source.album, &hash)?
        {
            upsert_record(
                database,
                &source.album,
                &record,
                &known_name,
                &media_id,
                &state,
            )?;
            stats.content_duplicates += 1;
            continue;
        }
        if known_takeout_hash(database, &hash)? {
            upsert_record(database, &source.album, &record, "", "", "takeout-existing")?;
            stats.content_duplicates += 1;
            continue;
        }

        let legacy_name = legacy_content_name(&path, &hash);
        let original_name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
        let remote_match = [upload_name.as_str(), legacy_name.as_str(), original_name]
            .iter()
            .find_map(|name| {
                remote
                    .get(*name)
                    .map(|id| ((*name).to_owned(), id.to_owned()))
            });
        if let Some((remote_name, media_id)) = remote_match {
            upsert_record(
                database,
                &source.album,
                &record,
                &remote_name,
                &media_id,
                "remote-existing",
            )?;
            stats.recovered_remote += 1;
            continue;
        }

        if let Some(index) = candidate_by_hash.get(&hash).copied() {
            candidates[index].aliases.push(record);
            stats.content_duplicates += 1;
        } else {
            candidate_by_hash.insert(hash, candidates.len());
            candidates.push(Candidate {
                primary: record,
                aliases: Vec::new(),
            });
        }
    }

    if let Some(limit) = options.limit {
        candidates.truncate(limit);
    }
    stats.planned = candidates.len();
    if options.dry_run {
        for candidate in &candidates {
            logger.log(
                "TEST",
                format!(
                    "Wuerde hochladen: {} -> {}",
                    candidate.primary.path.display(),
                    source.album
                ),
            );
        }
        return Ok(stats);
    }

    for chunk in candidates.chunks(50) {
        let mut tokens = Vec::new();
        let mut token_to_index = HashMap::new();
        google.ensure_fresh_access_token()?;
        let next = AtomicUsize::new(0);
        let upload_results = Mutex::new(Vec::<(usize, Result<String, String>)>::new());
        let workers = chunk.len().min(4);
        let google_ref: &GoogleClient = google;
        thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(candidate) = chunk.get(index) else {
                            break;
                        };
                        logger.log(
                            "UPLOAD",
                            format!("{} -> {}", candidate.primary.path.display(), source.album),
                        );
                        let result = google_ref
                            .raw_upload(
                                &candidate.primary.path,
                                mime_type(&candidate.primary.path),
                                candidate.primary.size as u64,
                            )
                            .map_err(|error| error.to_string());
                        upload_results
                            .lock()
                            .expect("upload result lock")
                            .push((index, result));
                    }
                });
            }
        });
        let mut upload_results = upload_results.into_inner().expect("upload result lock");
        upload_results.sort_by_key(|(index, _)| *index);
        for (index, result) in upload_results {
            match result {
                Ok(token) => {
                    token_to_index.insert(token.clone(), index);
                    tokens.push((token, chunk[index].primary.upload_name.clone()));
                }
                Err(error) => {
                    stats.failed += 1;
                    logger.log(
                        "FEHLER",
                        format!("{}: {error}", chunk[index].primary.path.display()),
                    );
                }
            }
        }
        if tokens.is_empty() {
            continue;
        }

        let response = match google.batch_create(album_id, &tokens) {
            Ok(response) => response,
            Err(error) => {
                stats.failed += tokens.len();
                logger.log(
                    "FEHLER",
                    format!("Album-Bestaetigung fuer {}: {error}", source.album),
                );
                continue;
            }
        };
        let results = response
            .get("newMediaItemResults")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut responded = HashSet::new();
        for result in results {
            let token = result
                .get("uploadToken")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Some(index) = token_to_index.get(token).copied() else {
                continue;
            };
            responded.insert(index);
            let code = result
                .pointer("/status/code")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if code != 0 || result.get("mediaItem").is_none() {
                let message = result
                    .pointer("/status/message")
                    .and_then(Value::as_str)
                    .unwrap_or("Google hat das Medium nicht bestaetigt.");
                stats.failed += 1;
                logger.log(
                    "FEHLER",
                    format!("{}: {message}", chunk[index].primary.path.display()),
                );
                continue;
            }
            let media_id = result
                .pointer("/mediaItem/id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            persist_candidate(database, &source.album, &chunk[index], media_id)?;
            stats.uploaded += 1;
        }
        for (_, index) in token_to_index {
            if !responded.contains(&index) {
                stats.failed += 1;
                logger.log(
                    "FEHLER",
                    format!(
                        "Keine Bestaetigung fuer {}",
                        chunk[index].primary.path.display()
                    ),
                );
            }
        }
    }

    Ok(stats)
}

fn persist_candidate(
    database: &mut Connection,
    album: &str,
    candidate: &Candidate,
    media_id: &str,
) -> AppResult<()> {
    let transaction = database.transaction()?;
    upsert_record_on(
        &transaction,
        album,
        &candidate.primary,
        &candidate.primary.upload_name,
        media_id,
        "uploaded",
    )?;
    for alias in &candidate.aliases {
        upsert_record_on(
            &transaction,
            album,
            alias,
            &candidate.primary.upload_name,
            media_id,
            "content-duplicate",
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn open_database(path: &Path) -> AppResult<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let connection = Connection::open(path)?;
    connection.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS media (
             album TEXT NOT NULL,
             path TEXT NOT NULL,
             size INTEGER NOT NULL,
             mtime_ns INTEGER NOT NULL,
             sha256 TEXT NOT NULL,
             upload_name TEXT NOT NULL,
             media_id TEXT NOT NULL,
             state TEXT NOT NULL,
             updated_at INTEGER NOT NULL,
             PRIMARY KEY (album, path)
         );
         CREATE INDEX IF NOT EXISTS media_album_hash ON media(album, sha256);
         CREATE TABLE IF NOT EXISTS known_hashes (
             sha256 TEXT PRIMARY KEY,
             source TEXT NOT NULL,
             imported_at INTEGER NOT NULL
         );",
    )?;
    Ok(connection)
}

fn fully_known_source(
    connection: &Connection,
    source: &SourceSpec,
) -> AppResult<Option<SyncStats>> {
    let files = source_files(&source.path, source.extensions())?;
    for path in &files {
        let metadata = fs::metadata(path)?;
        let size = i64::try_from(metadata.len())?;
        let mtime_ns = modified_ns(&metadata)?;
        let path_text = path.to_string_lossy();
        let Some((_, _, state)) =
            current_record(connection, &source.album, &path_text, size, mtime_ns)?
        else {
            return Ok(None);
        };
        if !trusted_state(&state) {
            return Ok(None);
        }
    }
    Ok(Some(SyncStats {
        scanned: files.len(),
        unchanged: files.len(),
        ..SyncStats::default()
    }))
}

fn trusted_state(state: &str) -> bool {
    matches!(
        state,
        "uploaded" | "remote-existing" | "takeout-existing" | "content-duplicate"
    )
}

fn current_record(
    connection: &Connection,
    album: &str,
    path: &str,
    size: i64,
    mtime_ns: i64,
) -> AppResult<Option<(String, String, String)>> {
    Ok(connection
        .query_row(
            "SELECT sha256, upload_name, state FROM media
             WHERE album = ?1 AND path = ?2 AND size = ?3 AND mtime_ns = ?4",
            params![album, path, size, mtime_ns],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?)
}

fn record_by_hash(
    connection: &Connection,
    album: &str,
    hash: &str,
) -> AppResult<Option<(String, String, String)>> {
    Ok(connection
        .query_row(
            "SELECT upload_name, media_id, state FROM media WHERE album = ?1 AND sha256 = ?2 LIMIT 1",
            params![album, hash],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?)
}

fn known_takeout_hash(connection: &Connection, hash: &str) -> AppResult<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM known_hashes WHERE sha256 = ?1",
            params![hash],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn upsert_record(
    connection: &Connection,
    album: &str,
    record: &FileRecord,
    upload_name: &str,
    media_id: &str,
    state: &str,
) -> AppResult<()> {
    upsert_record_on(connection, album, record, upload_name, media_id, state)
}

fn upsert_record_on(
    connection: &Connection,
    album: &str,
    record: &FileRecord,
    upload_name: &str,
    media_id: &str,
    state: &str,
) -> AppResult<()> {
    connection.execute(
        "INSERT INTO media(album, path, size, mtime_ns, sha256, upload_name, media_id, state, updated_at)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(album, path) DO UPDATE SET
           size=excluded.size,
           mtime_ns=excluded.mtime_ns,
           sha256=excluded.sha256,
           upload_name=excluded.upload_name,
           media_id=excluded.media_id,
           state=excluded.state,
           updated_at=excluded.updated_at",
        params![
            album,
            record.path.to_string_lossy(),
            record.size,
            record.mtime_ns,
            record.sha256,
            upload_name,
            media_id,
            state,
            unix_seconds()
        ],
    )?;
    Ok(())
}

fn import_takeout(paths: &AppPaths, logger: &Logger, root: &Path) -> AppResult<()> {
    if !root.is_dir() {
        return Err(format!("Takeout-Ordner nicht gefunden: {}", root.display()).into());
    }
    let connection = open_database(&paths.database)?;
    let extensions: HashSet<&str> = IMAGE_EXTENSIONS
        .iter()
        .chain(VIDEO_EXTENSIONS.iter())
        .copied()
        .collect();
    let files = source_files(root, &extensions.iter().copied().collect::<Vec<_>>())?;
    logger.log(
        "START",
        format!("Importiere Hashes aus {} Mediendateien", files.len()),
    );
    let mut inserted = 0;
    for (index, path) in files.iter().enumerate() {
        let hash = sha256_file(path)?;
        inserted += connection.execute(
            "INSERT OR IGNORE INTO known_hashes(sha256, source, imported_at) VALUES(?1, ?2, ?3)",
            params![hash, root.to_string_lossy(), unix_seconds()],
        )?;
        if (index + 1) % 250 == 0 {
            logger.log(
                "INFO",
                format!("{} / {} Takeout-Dateien geprueft", index + 1, files.len()),
            );
        }
    }
    logger.log(
        "OK",
        format!(
            "Takeout-Import fertig: {} neue eindeutige Inhalte",
            inserted
        ),
    );
    Ok(())
}

fn show_status(paths: &AppPaths) -> AppResult<()> {
    let connection = open_database(&paths.database)?;
    println!("Rust Google-Fotos-Sync");
    println!("Aufgabe: {TASK_NAME}");
    println!("Datenbank: {}", paths.database.display());
    println!("Protokoll: {}", paths.log.display());
    println!(
        "Zugang geschuetzt: {}",
        if paths.credentials.is_file() {
            "ja"
        } else {
            "nein"
        }
    );
    println!("Konfiguration: {}", paths.config.display());
    for source in &paths.sources {
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM media WHERE album = ?1",
            params![source.album],
            |row| row.get(0),
        )?;
        println!("{}: {} bekannte lokale Dateien", source.album, count);
    }
    let takeout: i64 =
        connection.query_row("SELECT COUNT(*) FROM known_hashes", [], |row| row.get(0))?;
    println!("Takeout-Hashes: {takeout}");
    Ok(())
}

fn source_files(root: &Path, extensions: &[&str]) -> AppResult<Vec<PathBuf>> {
    let allowed: HashSet<&str> = extensions.iter().copied().collect();
    let mut files = Vec::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let extension = entry
            .path()
            .extension()
            .and_then(OsStr::to_str)
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if allowed.contains(extension.as_str()) {
            files.push(entry.into_path());
        }
    }
    files.sort_by(|left, right| left.to_string_lossy().cmp(&right.to_string_lossy()));
    Ok(files)
}

fn sha256_file(path: &Path) -> AppResult<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    Ok(hex_encode(&digest))
}

fn content_addressed_name(path: &Path, hash: &str) -> String {
    let stem = path.file_stem().and_then(OsStr::to_str).unwrap_or("media");
    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let safe_stem: String = stem
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ' ') {
                character
            } else {
                '_'
            }
        })
        .take(100)
        .collect();
    format!("{}--{}.{}", safe_stem.trim(), &hash[..12], extension)
}

fn legacy_content_name(path: &Path, hash: &str) -> String {
    let stem = path.file_stem().and_then(OsStr::to_str).unwrap_or("media");
    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    format!("{stem}--{}.{}", &hash[..12], extension)
}

fn mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("heic" | "heif") => "image/heic",
        Some("tif" | "tiff") => "image/tiff",
        Some("bmp") => "image/bmp",
        Some("mp4" | "m4v") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("mkv") => "video/x-matroska",
        Some("avi") => "video/x-msvideo",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

fn modified_ns(metadata: &fs::Metadata) -> AppResult<i64> {
    let duration = metadata.modified()?.duration_since(UNIX_EPOCH)?;
    Ok(i64::try_from(duration.as_nanos())?)
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn truncate(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn option_usize(args: &[String], name: &str) -> AppResult<Option<usize>> {
    let Some(index) = args.iter().position(|argument| argument == name) else {
        return Ok(None);
    };
    let value = args
        .get(index + 1)
        .ok_or_else(|| format!("Nach {name} fehlt eine Zahl."))?;
    Ok(Some(value.parse()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let input = b"dpapi-test\0\xff";
        assert_eq!(hex_decode(&hex_encode(input)).unwrap(), input);
    }

    #[test]
    fn upload_name_is_content_addressed_and_ascii_safe() {
        let path = Path::new(r"D:\Pictures\Ueber größe!.PNG");
        let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(
            content_addressed_name(path, hash),
            "Ueber gr__e_--0123456789ab.png"
        );
    }

    #[test]
    fn parses_optional_limit() {
        let args = vec!["sync".to_owned(), "--limit".to_owned(), "4".to_owned()];
        assert_eq!(option_usize(&args, "--limit").unwrap(), Some(4));
        assert_eq!(option_usize(&args, "--missing").unwrap(), None);
    }

    #[test]
    fn media_types_cover_sources() {
        assert_eq!(mime_type(Path::new("screen.png")), "image/png");
        assert_eq!(mime_type(Path::new("clip.mp4")), "video/mp4");
    }

    #[test]
    fn oauth_query_values_roundtrip() {
        let value = "https://localhost/callback?scope=one two&state=ä";
        assert_eq!(url_decode(&url_encode(value)), value);
    }

    #[test]
    fn config_roundtrip_preserves_sources() {
        let config = AppConfig {
            sources: vec![SourceSpec {
                album: "AMD-Clips".to_owned(),
                path: PathBuf::from(r"D:\Captures"),
                kind: MediaKind::Videos,
            }],
        };
        let encoded = serde_json::to_vec(&config).unwrap();
        let decoded: AppConfig = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.sources[0].album, "AMD-Clips");
        assert_eq!(decoded.sources[0].path, PathBuf::from(r"D:\Captures"));
        assert!(matches!(decoded.sources[0].kind, MediaKind::Videos));
    }
}
