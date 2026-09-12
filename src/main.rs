//! DSH Launcher — one-click launcher for DeepSeek Harness (Windows).
//!
//! Behavior:
//! 1. If the preferred port already serves a usable DSH UI (no token needed),
//!    reuse it and just open the browser.
//! 2. Otherwise start `npx @deepseek-ai/dsh web` with no console window ever
//!    visible, and read the tokenized launch URL from its output.
//! 3. Open a fresh Chrome window (isolated temporary profile) at that URL and
//!    stay alive silently.
//! 4. When that Chrome window is closed, stop the server process tree and exit.
//!    A service that was already running before launch is left alone.
//!
//! Nothing about DSH's CLI is hard-coded beyond editable defaults: the command,
//! its extra arguments, the preferred port, the output marker that carries the
//! launch URL and the readiness timeout all come from `dsh-launcher.conf` next
//! to the exe (see `CONFIG_TEMPLATE`), and every launch step has a fallback so a
//! DSH update normally needs no rebuild.
//!
//! Zero external dependencies: the few Win32 functions we need
//! (job objects, mutex, message box) are declared as raw `extern "system"`.
//!
//! Test-only knobs (see README): DSH_DATA_DIR, DSH_PORT, DSH_SERVER_EXTRA,
//! DSH_FAKE_CHROME, DSH_NO_UI, DSH_READY_TIMEOUT_SECS, DSH_CHROME,
//! DSH_LAUNCHER_CONFIG, DSH_SKIP_SINGLE_INSTANCE.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::ffi::c_void;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DEFAULT_PORT: u16 = 3080;
const CONFIG_FILE_NAME: &str = "dsh-launcher.conf";
const PID_FILE_NAME: &str = "launcher.pid";
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const ERROR_INVALID_PARAMETER: u32 = 87;
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: u32 = 9; // JobObjectExtendedLimitInformation
const ERROR_ALREADY_EXISTS: u32 = 183;

// How long to keep waiting, after the port already accepts connections, for
// `dsh web` to print its tokenized URL on stdout.
const LAUNCH_URL_GRACE: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// minimal Win32 FFI (no external crates)
// ---------------------------------------------------------------------------

#[link(name = "kernel32")]
extern "system" {
    fn CreateJobObjectW(
        lp_job_attributes: *const c_void,
        lp_name: *const u16,
    ) -> *mut c_void;
    fn AssignProcessToJobObject(h_job: *mut c_void, h_process: *mut c_void) -> i32;
    fn SetInformationJobObject(
        h_job: *mut c_void,
        job_object_information_class: u32,
        lp_job_object_information: *const c_void,
        cb_job_object_information_length: u32,
    ) -> i32;
    fn CloseHandle(h_object: *mut c_void) -> i32;
    fn OpenProcess(
        dw_desired_access: u32,
        b_inherit_handle: i32,
        dw_process_id: u32,
    ) -> *mut c_void;
    fn CreateMutexW(
        lp_mutex_attributes: *const c_void,
        b_initial_owner: i32,
        lp_name: *const u16,
    ) -> *mut c_void;
    fn GetLastError() -> u32;
}

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(
        h_wnd: *mut c_void,
        lp_text: *const u16,
        lp_caption: *const u16,
        u_type: u32,
    ) -> i32;
}

// Layouts follow winnt.h; only x86_64 is targeted.
#[repr(C)]
struct JobObjectBasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[repr(C)]
struct IoCounters {
    read_operation_count: u64,
    write_operation_count: u64,
    other_operation_count: u64,
    read_transfer_count: u64,
    write_transfer_count: u64,
    other_transfer_count: u64,
}

#[repr(C)]
struct JobObjectExtendedLimitInformation {
    basic_limit_information: JobObjectBasicLimitInformation,
    io_info: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn no_ui() -> bool {
    env::var("DSH_NO_UI").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
}

fn show_message(caption: &str, text: &str) {
    if no_ui() {
        log_msg(&format!("[UI suppressed] {caption}: {text}"));
        return;
    }
    let c = wide(caption);
    let t = wide(text);
    // MB_OK | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST
    let flags: u32 = 0x0000_0010 | 0x0001_0000 | 0x0004_0000;
    unsafe {
        MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), flags);
    }
}

// ---------------------------------------------------------------------------
// logging (file only; GUI subsystem builds have no console)
// ---------------------------------------------------------------------------

fn data_dir() -> PathBuf {
    if let Ok(d) = env::var("DSH_DATA_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("dsh-launcher")
}

fn ensure_dir(p: &Path) {
    let _ = fs::create_dir_all(p);
}

fn log_path() -> PathBuf {
    data_dir().join("launcher.log")
}

fn server_log_path() -> PathBuf {
    data_dir().join("server.log")
}

fn now_ts() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("[{secs}]")
}

fn log_msg(msg: &str) {
    let p = log_path();
    ensure_dir(p.parent().unwrap_or(Path::new(".")));
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{} {}", now_ts(), msg);
    }
    #[cfg(debug_assertions)]
    eprintln!("{msg}");
}

fn read_tail(path: &Path, max_bytes: usize) -> String {
    match fs::read(path) {
        Ok(bytes) => {
            let start = bytes.len().saturating_sub(max_bytes);
            String::from_utf8_lossy(&bytes[start..]).into_owned()
        }
        Err(_) => String::new(),
    }
}

/// Logs are append-only across runs; drop an oversized one instead of letting it
/// grow forever (readiness scanning and error dialogs only ever look at the tail,
/// and `start_server` records its offset after this runs).
fn rotate_log(path: &Path, max_bytes: u64) {
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > max_bytes && fs::remove_file(path).is_ok() {
            log_msg(&format!(
                "rotated oversized log {} ({} bytes)",
                path.display(),
                meta.len()
            ));
        }
    }
}

/// Removes `profiles\run-*` directories left behind by earlier runs (Chrome may
/// still hold locks when the launcher exits, so cleanup is best-effort there).
/// A directory whose recorded launcher pid is still alive is never touched, so a
/// profile belonging to a launcher that is still open cannot be deleted — even
/// one whose Chrome window has been open for days.
fn prune_profiles(fallback_age: Duration) {
    let dir = data_dir().join("profiles");
    let Ok(entries) = fs::read_dir(&dir) else { return };
    let mut removed = 0u32;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("run-") {
            continue;
        }
        let path = entry.path();
        // Owned by a live launcher (possibly another Windows session)? Keep it.
        if let Ok(text) = fs::read_to_string(path.join(PID_FILE_NAME)) {
            if let Ok(pid) = text.trim().parse::<u32>() {
                if pid_alive(pid) {
                    continue;
                }
            }
        }
        // No pid file (an older version's directory) or the owner is gone:
        // remove it once it is old enough.
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .map(|age| age > fallback_age)
            .unwrap_or(false);
        if stale && fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        log_msg(&format!("pruned {removed} stale Chrome profile dir(s)"));
    }
}

/// Whether a process with that pid currently exists. An access-denied answer
/// still proves existence (the pid belongs to someone).
fn pid_alive(pid: u32) -> bool {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return GetLastError() != ERROR_INVALID_PARAMETER;
        }
        CloseHandle(handle);
        true
    }
}

/// Chrome offers to "install" the DSH page as an app (the blue Install chip in
/// the address bar) because the DSH web UI ships a web app manifest. This
/// launcher's Chrome profile is thrown away when the window closes, so
/// installing into it is pointless — block the installation content setting up
/// front by seeding the profile's preferences before Chrome starts.
///
/// The keys are the ones Chrome itself uses for Settings → Site settings →
/// "Web app installation"; `2` is BLOCK. Best effort: if a Chrome release stops
/// honouring them the chip simply comes back, nothing else breaks.
fn seed_chrome_prefs(profile: &Path, block_web_app_install: bool) {
    let default_dir = profile.join("Default");
    ensure_dir(&default_dir);
    let prefs_path = default_dir.join("Preferences");
    if prefs_path.exists() {
        return;
    }
    let prefs = if block_web_app_install {
        concat!(
            r#"{"profile":{"default_content_setting_values":{"web_app_installation":2},"#,
            r#""content_settings":{"exceptions":{"web_app_installation":{"*,*":{"setting":2}}}}}}"#
        )
    } else {
        "{}"
    };
    match fs::write(&prefs_path, prefs) {
        Ok(()) if block_web_app_install => {
            log_msg("seeded Chrome prefs: web app installation blocked (no Install chip)")
        }
        Ok(()) => {}
        Err(e) => log_msg(&format!("could not seed Chrome prefs: {e}")),
    }
}

// ---------------------------------------------------------------------------
// configuration
//
// Everything about how DSH is started lives here, so a future DSH release can
// be accommodated by editing a text file instead of rebuilding the launcher:
// the built-in values below are only defaults.
// ---------------------------------------------------------------------------

const CONFIG_TEMPLATE: &str = "\
# dsh-launcher configuration — every entry is optional.
# The values below are the built-in defaults; delete this file to restore them.
# Edit this file (no rebuild needed) when a DSH release changes its CLI.

# Command that serves the DSH web UI. The launcher appends the port argument.
command = npx --yes @deepseek-ai/dsh web

# Extra arguments for the first launch attempt (space separated).
# --no-open keeps dsh from opening a browser of its own.
extra_args = --no-open

# Preferred port. If a token-less DSH UI already serves it, that service is
# reused; if it is busy with anything else, the launcher starts its own instance
# on a free port. Use 0 to always let dsh pick a free port.
port = 3080

# A page already served on the preferred port is only reused when its body
# contains this text, so the launcher never adopts an unrelated local web app.
# Set it to an empty value to reuse any page that answers HTTP 200.
ui_marker = DeepSeek Harness

# Text that marks the line of dsh output carrying the launch URL; the first
# http(s) URL after it is handed to the browser.
url_marker = dsh web:

# How long to wait for the server to become ready, in seconds.
timeout_secs = 300

# The DSH web UI ships a web app manifest, so Chrome adds an Install chip to the
# address bar. The launcher's Chrome profile is temporary (deleted when the
# window closes), so installing into it is pointless: true blocks that content
# setting before Chrome starts. Set false to get the Install chip back.
block_web_app_install = true
";

struct LaunchConfig {
    command: String,
    extra_args: Vec<String>,
    port: u16,
    ui_marker: String,
    url_marker: String,
    timeout: Duration,
    block_web_app_install: bool,
    /// True when DSH_SERVER_EXTRA replaced the command: the launcher then runs
    /// that command verbatim (regression scenarios rely on this).
    test_command: bool,
    path: PathBuf,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        LaunchConfig {
            command: "npx --yes @deepseek-ai/dsh web".to_string(),
            extra_args: vec!["--no-open".to_string()],
            port: DEFAULT_PORT,
            ui_marker: "DeepSeek Harness".to_string(),
            url_marker: "dsh web:".to_string(),
            timeout: Duration::from_secs(300),
            block_web_app_install: true,
            test_command: false,
            path: PathBuf::new(),
        }
    }
}

/// `DSH_LAUNCHER_CONFIG` wins; otherwise `<exe dir>\dsh-launcher.conf` is used
/// and created from the template on first run; if the exe directory is not
/// writable the config lives in the data directory instead.
fn config_path() -> PathBuf {
    if let Ok(p) = env::var("DSH_LAUNCHER_CONFIG") {
        if !p.is_empty() {
            if let Some(dir) = Path::new(&p).parent() {
                ensure_dir(dir);
            }
            if !Path::new(&p).exists() {
                let _ = fs::write(&p, CONFIG_TEMPLATE);
            }
            return PathBuf::from(p);
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(CONFIG_FILE_NAME);
            if candidate.exists() {
                return candidate;
            }
            if fs::write(&candidate, CONFIG_TEMPLATE).is_ok() {
                log_msg(&format!("wrote default config: {}", candidate.display()));
                return candidate;
            }
            log_msg(&format!("cannot write {}; using the data directory", candidate.display()));
        }
    }
    let fallback = data_dir().join(CONFIG_FILE_NAME);
    if !fallback.exists() {
        ensure_dir(&data_dir());
        let _ = fs::write(&fallback, CONFIG_TEMPLATE);
    }
    fallback
}

impl LaunchConfig {
    fn load() -> LaunchConfig {
        let path = config_path();
        let mut cfg = LaunchConfig { path: path.clone(), ..LaunchConfig::default() };

        match fs::read(&path) {
            Ok(bytes) => {
                // Tolerate a UTF-8 BOM and non-UTF-8 files (e.g. Notepad saving
                // as ANSI): decode lossily so ASCII keys and values still work
                // instead of silently falling back to every default.
                let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
                let text = String::from_utf8_lossy(bytes);
                for line in text.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    let Some((key, value)) = line.split_once('=') else { continue };
                    let key = key.trim();
                    let value = value.trim();
                    match key {
                        "command" if !value.is_empty() => cfg.command = value.to_string(),
                        "extra_args" => {
                            cfg.extra_args = value.split_whitespace().map(str::to_string).collect()
                        }
                        "port" => match value.parse() {
                            Ok(p) => cfg.port = p,
                            Err(_) => log_msg(&format!("config: bad port {value:?}; keeping {}", cfg.port)),
                        },
                        "url_marker" => cfg.url_marker = value.to_string(),
                        // An empty value is meaningful here: reuse any HTTP 200 page.
                        "ui_marker" => cfg.ui_marker = value.to_string(),
                        "timeout_secs" => match value.parse::<u64>() {
                            Ok(s) => cfg.timeout = Duration::from_secs(s),
                            Err(_) => log_msg(&format!("config: bad timeout_secs {value:?}")),
                        },
                        "block_web_app_install" => match value.to_ascii_lowercase().as_str() {
                            "true" | "1" | "yes" | "on" => cfg.block_web_app_install = true,
                            "false" | "0" | "no" | "off" => cfg.block_web_app_install = false,
                            _ => log_msg(&format!("config: bad block_web_app_install {value:?}")),
                        },
                        _ => log_msg(&format!("config: ignoring unknown key {key:?}")),
                    }
                }
            }
            Err(e) => log_msg(&format!("config {} unreadable ({e}); using defaults", path.display())),
        }

        // Environment overrides, used by the regression scenarios.
        if let Ok(v) = env::var("DSH_PORT") {
            if let Ok(p) = v.parse() {
                cfg.port = p;
            }
        }
        if let Ok(v) = env::var("DSH_READY_TIMEOUT_SECS") {
            if let Ok(s) = v.parse::<u64>() {
                cfg.timeout = Duration::from_secs(s);
            }
        }
        if let Ok(v) = env::var("DSH_SERVER_EXTRA") {
            if !v.trim().is_empty() {
                cfg.command = v;
                cfg.test_command = true;
            }
        }

        log_msg(&format!(
            "config {}: command={:?} extra_args=[{}] port={} url_marker={:?} timeout={}s",
            path.display(),
            cfg.command,
            cfg.extra_args.join(" "),
            cfg.port,
            cfg.url_marker,
            cfg.timeout.as_secs()
        ));
        cfg
    }
}

// ---------------------------------------------------------------------------
// launch URL discovery
// ---------------------------------------------------------------------------

/// The tokenized URL `dsh web` prints on stdout. This is the only place its
/// per-process launch token can be read from: the token lives in an in-memory
/// weak map inside that node process and is never written to disk, and the bare
/// URL is rejected (401) unless a `dsh-auth-*` cookie was already minted from
/// it. Scanning only bytes appended after `from_offset` keeps a stale token from
/// a previous run from being reused.
///
/// Two independent shapes are accepted so a renamed marker or a reshaped URL
/// does not break the launcher: the configured `marker` line first, then any
/// loopback URL that carries a token query anywhere in this run's output.
fn find_launch_url(from_offset: u64, marker: &str) -> Option<String> {
    let mut file = fs::File::open(server_log_path()).ok()?;
    let len = file.metadata().ok()?.len();
    let start = from_offset.min(len);
    if start == len {
        return None; // nothing appended since the spawn
    }
    // Seek instead of reading the whole (append-only, ever growing) log.
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);

    let strip = |tok: &str| tok.trim_end_matches([')', ',', '.']).to_string();
    // Only the local UI address may ever reach the browser: never a LAN address
    // from the "(LAN: …)" suffix or anything else dsh might print.
    let is_loopback = |url: &str| {
        url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:")
    };

    if !marker.is_empty() {
        for line in text.lines() {
            let Some(at) = line.find(marker) else { continue };
            for token in line[at + marker.len()..].split_whitespace() {
                let url = strip(token);
                if is_loopback(&url) {
                    return Some(url);
                }
            }
        }
    }

    for line in text.lines() {
        for token in line.split_whitespace() {
            let url = strip(token);
            if is_loopback(&url) && (url.contains("?token=") || url.contains("&token=")) {
                return Some(url);
            }
        }
    }
    None
}

/// Port of an `http://127.0.0.1:<port>/...` URL. Readiness has to be checked on
/// whichever port dsh actually bound, which is only known from its URL when it
/// was allowed to pick one itself (`--port 0`).
fn url_port(url: &str) -> Option<u16> {
    let rest = url
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| url.strip_prefix("http://localhost:"))?;
    rest.split(['/', '?']).next()?.parse().ok()
}

/// `GET /` on the port: HTTP status code plus the first `max_body` bytes of the
/// response body. `None` when nothing answers HTTP there.
fn http_get(port: u16, max_body: usize) -> Option<(u16, String)> {
    let mut stream = TcpStream::connect_timeout(&addr_for(port), Duration::from_millis(600)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok()?;
    let request = format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUser-Agent: dsh-launcher\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;

    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut chunk = [0u8; 2048];
    while buf.len() <= max_body + 512 {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => break, // timeout or reset: whatever arrived is enough
        }
    }

    let text = String::from_utf8_lossy(&buf);
    let status: u16 = text.lines().next()?.split_whitespace().nth(1)?.parse().ok()?;
    let body = match text.find("\r\n\r\n") {
        Some(at) => text[at + 4..].to_string(),
        None => String::new(),
    };
    Some((status, body))
}

// ---------------------------------------------------------------------------
// small helpers
// ---------------------------------------------------------------------------

fn url_for(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}

fn addr_for(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

fn port_open(port: u16) -> bool {
    TcpStream::connect_timeout(&addr_for(port), Duration::from_millis(400)).is_ok()
}

// ---------------------------------------------------------------------------
// single instance
// ---------------------------------------------------------------------------

struct SingleInstance(*mut c_void);

impl SingleInstance {
    fn acquire() -> Option<SingleInstance> {
        // Regression tests run while a real launcher (and the DSH session it
        // serves) is already up; without this they would exit silently on the
        // single-instance lock.
        if env::var("DSH_SKIP_SINGLE_INSTANCE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        {
            log_msg("single-instance check skipped (DSH_SKIP_SINGLE_INSTANCE)");
            return Some(SingleInstance(std::ptr::null_mut()));
        }
        let name = wide("Local\\dsh-launcher-single");
        unsafe {
            let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if h.is_null() {
                log_msg("single-instance mutex creation failed; continuing");
                return Some(SingleInstance(std::ptr::null_mut()));
            }
            let err = GetLastError();
            if err == ERROR_ALREADY_EXISTS {
                CloseHandle(h);
                log_msg("another launcher instance is running; exiting");
                return None;
            }
            Some(SingleInstance(h))
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

// ---------------------------------------------------------------------------
// job object (kill whole tree on exit, even on abrupt termination)
// ---------------------------------------------------------------------------

struct KillJob(*mut c_void);

impl KillJob {
    fn create() -> Option<KillJob> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                log_msg("CreateJobObjectW failed");
                return None;
            }
            let mut info: JobObjectExtendedLimitInformation = std::mem::zeroed();
            info.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                &info as *const _ as *const c_void,
                std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
            );
            if ok == 0 {
                log_msg("SetInformationJobObject failed");
                CloseHandle(job);
                return None;
            }
            log_msg("kill-on-close job object created");
            Some(KillJob(job))
        }
    }

    fn assign(&self, child: &Child) {
        unsafe {
            let h = child.as_raw_handle();
            if AssignProcessToJobObject(self.0, h) == 0 {
                log_msg("AssignProcessToJobObject failed; taskkill fallback will be used");
            } else {
                log_msg("server process assigned to job");
            }
        }
    }
}

impl Drop for KillJob {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

// ---------------------------------------------------------------------------
// process helpers
// ---------------------------------------------------------------------------

fn kill_tree(pid: u32) {
    let _ = Command::new("taskkill.exe")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .and_then(|mut c| c.wait());
}

fn spawn_hidden(args: &[String]) -> std::io::Result<Child> {
    Command::new("cmd.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

// ---------------------------------------------------------------------------
// server
// ---------------------------------------------------------------------------

/// One way of starting dsh. Attempts are tried in order and the next one is
/// used only when the previous exits before becoming ready — that is what makes
/// the launcher survive a flag being renamed or removed in a DSH release.
struct LaunchAttempt {
    label: &'static str,
    cmdline: String,
    /// Port the launcher may probe for readiness, when it knows it; `None` when
    /// dsh picks the port itself and only its printed URL can reveal it.
    probe_port: Option<u16>,
}

fn build_attempts(cfg: &LaunchConfig, bind_port: Option<u16>) -> Vec<LaunchAttempt> {
    if cfg.test_command {
        return vec![LaunchAttempt {
            label: "test command",
            cmdline: cfg.command.clone(),
            probe_port: bind_port,
        }];
    }
    let extra = if cfg.extra_args.is_empty() {
        String::new()
    } else {
        format!(" {}", cfg.extra_args.join(" "))
    };
    let port_arg = match bind_port {
        Some(p) => format!(" --port {p}"),
        None => " --port 0".to_string(),
    };
    // The port is kept in every attempt that still passes flags: dsh's own
    // default is 3080, which is exactly the port a user is likely to have in
    // use, so dropping --port (the 2nd attempt) would collide instead of
    // starting our own instance. Only the last resort drops all flags.
    vec![
        LaunchAttempt {
            label: "command + extra args + port",
            cmdline: format!("{}{}{}", cfg.command, extra, port_arg),
            probe_port: bind_port,
        },
        LaunchAttempt {
            label: "command + a free port (no extra args)",
            cmdline: format!("{} --port 0", cfg.command),
            probe_port: None,
        },
        LaunchAttempt {
            label: "command only (no flags at all)",
            cmdline: cfg.command.clone(),
            probe_port: None,
        },
    ]
}

/// Starts the server and also returns the length `server.log` had before the
/// spawn, so the launch URL can be searched for in this run's output only.
fn start_server(job: Option<&KillJob>, cmdline: &str) -> std::io::Result<(Child, u64)> {
    let server_log = server_log_path();
    ensure_dir(server_log.parent().unwrap_or(Path::new(".")));
    let log_from = fs::metadata(&server_log).map(|m| m.len()).unwrap_or(0);
    let out = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&server_log)
        .map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!("无法写入日志目录 {} : {e}", server_log.display()),
            )
        })?;
    let err = out.try_clone()?;

    let mut cmd = Command::new("cmd.exe");
    // `raw_arg` passes the assembled command line through verbatim. Using
    // `args(["/C", cmdline])` would escape embedded quotes as \" which cmd.exe
    // does not understand, so a user-supplied command such as
    // `"C:\Program Files\nodejs\npx.cmd" web` would fail to start.
    cmd.raw_arg("/C ").raw_arg(cmdline);
    cmd.current_dir(
        env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".")),
    );
    cmd.creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));
    log_msg(&format!("server command: cmd /C {cmdline}"));

    let child = cmd.spawn()?;
    log_msg(&format!("server child spawned (pid {}), waiting for the port", child.id()));
    if let Some(j) = job {
        j.assign(&child);
    }
    Ok((child, log_from))
}

enum WaitReady {
    /// The URL to hand to the browser.
    Ready(String),
    /// The port serves something, but it rejects the bare URL and no tokenized
    /// URL was printed: a DSH release changed its output, so the launcher cannot
    /// authorize itself.
    NoLaunchUrl { port: u16 },
    ServerExited(ExitStatus),
    Timeout,
}

/// `probe_port` is the port that was asked for, or `None` when dsh picks one
/// itself — in that case its tokenized URL is the only readiness signal.
fn wait_ready(
    mut server: Option<&mut Child>,
    probe_port: Option<u16>,
    log_from: u64,
    cfg: &LaunchConfig,
) -> WaitReady {
    let deadline = Instant::now() + cfg.timeout;
    let mut port_open_since: Option<Instant> = None;
    let mut bare_probe_done = false;
    let mut tokenless_probe_at: Option<Instant> = None;
    loop {
        if let Some(url) = find_launch_url(log_from, &cfg.url_marker) {
            if url_port(&url).is_none_or(port_open) {
                if url.contains("token=") {
                    // Carries this process's launch token: ready to use as is.
                    return WaitReady::Ready(url);
                }
                // A dsh without token auth prints a plain URL. Accept it only
                // when the page is really served, otherwise the browser would
                // show a 401 page; probe at most once per grace period.
                let now = Instant::now();
                let due = tokenless_probe_at
                    .map(|at| now.duration_since(at) >= LAUNCH_URL_GRACE)
                    .unwrap_or(true);
                if due {
                    tokenless_probe_at = Some(now);
                    match url_port(&url).and_then(|p| http_get(p, 4096)) {
                        Some((200, _)) | Some((303, _)) => return WaitReady::Ready(url),
                        Some((status, _)) => log_msg(&format!(
                            "printed URL without a token answered {status}; still waiting for a tokenized one"
                        )),
                        None => {}
                    }
                }
            }
        }
        if let Some(port) = probe_port {
            if port_open(port) {
                // The port can be bound before the tokenized URL is printed
                // (dsh listens first and prints it once its loader settles), so a
                // non-usable bare URL here means "still starting", never "failed".
                let since = *port_open_since.get_or_insert_with(Instant::now);
                if !bare_probe_done && since.elapsed() >= LAUNCH_URL_GRACE {
                    bare_probe_done = true;
                    match http_get(port, 4096) {
                        Some((200, _)) | Some((303, _)) => {
                            log_msg("no tokenized launch URL in output; the bare URL is usable, using it");
                            return WaitReady::Ready(url_for(port));
                        }
                        Some((status, _)) => log_msg(&format!(
                            "bare URL on port {port} answered {status}; waiting for the tokenized launch URL"
                        )),
                        None => {}
                    }
                }
            }
        }
        if let Some(c) = server.as_deref_mut() {
            if let Some(st) = c.try_wait().unwrap_or(None) {
                return WaitReady::ServerExited(st);
            }
        }
        if Instant::now() >= deadline {
            // Something is serving the port but never printed a usable URL:
            // that is the shape of a DSH release that changed its output.
            return match probe_port {
                Some(port) if port_open(port) => WaitReady::NoLaunchUrl { port },
                _ => WaitReady::Timeout,
            };
        }
        sleep(Duration::from_millis(300));
    }
}

// ---------------------------------------------------------------------------
// browser
// ---------------------------------------------------------------------------

fn chrome_path() -> Option<PathBuf> {
    if let Ok(p) = env::var("DSH_CHROME") {
        if !p.is_empty() && Path::new(&p).is_file() {
            return Some(PathBuf::from(p));
        }
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    let pf = env::var_os("PROGRAMFILES").map(PathBuf::from);
    let pf86 = env::var_os("PROGRAMFILES(X86)").map(PathBuf::from);
    let lad = env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let mut push = |base: Option<PathBuf>, rel: &str| {
        if let Some(b) = base {
            candidates.push(b.join(rel));
        }
    };
    push(pf, r"Google\Chrome\Application\chrome.exe");
    push(pf86, r"Google\Chrome\Application\chrome.exe");
    push(lad, r"Google\Chrome\Application\chrome.exe");
    candidates.into_iter().find(|p| p.is_file())
}

/// A profile directory for this run, tagged with our pid so a later run can tell
/// whether the directory is still owned by a live launcher before pruning it.
fn fresh_profile_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = data_dir().join("profiles").join(format!("run-{nanos}"));
    ensure_dir(&dir);
    let _ = fs::write(dir.join(PID_FILE_NAME), std::process::id().to_string());
    dir
}

fn spawn_browser(url: &str, profile: &Path, block_web_app_install: bool) -> std::io::Result<Child> {
    // Fake chrome (tests): pings loopback for a while, then "closes".
    if let Ok(v) = env::var("DSH_FAKE_CHROME") {
        if let Ok(secs) = v.parse::<u64>() {
            let n = (secs * 2).max(2);
            log_msg(&format!("[test] fake chrome for about {secs}s"));
            let args = vec![
                "/C".to_string(),
                "ping".to_string(),
                "-n".to_string(),
                n.to_string(),
                "127.0.0.1".to_string(),
                ">".to_string(),
                "nul".to_string(),
            ];
            return spawn_hidden(&args);
        }
    }

    let Some(chrome) = chrome_path() else {
        return Err(std::io::Error::other("Chrome not found"));
    };
    ensure_dir(profile);
    // Must happen before Chrome starts: it reads the profile's preferences once.
    seed_chrome_prefs(profile, block_web_app_install);
    log_msg(&format!("opening {url} with chrome: {}", chrome.display()));
    // A fresh dedicated profile dir means this is a brand-new, isolated Chrome
    // instance: its window opens with ONLY the DSH page — no bookmarks,
    // extensions, logins, or other websites from the user's everyday Chrome.
    // --new-window makes the single-page window explicit; the URL argument is
    // the only content requested, so no extra tabs or "restore session" UI.
    Command::new(&chrome)
        .arg(format!("--user-data-dir={}", profile.display()))
        .args([
            "--new-window",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-mode",
            "--disable-session-crashed-bubble",
        ])
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

enum WaitResult {
    BrowserClosed,
    ServerDied(ExitStatus),
}

fn wait_until_browser_closes(browser: &mut Child, mut server: Option<&mut Child>) -> WaitResult {
    loop {
        if browser.try_wait().unwrap_or(None).is_some() {
            return WaitResult::BrowserClosed;
        }
        if let Some(c) = server.as_deref_mut() {
            if let Some(st) = c.try_wait().unwrap_or(None) {
                return WaitResult::ServerDied(st);
            }
        }
        sleep(Duration::from_millis(250));
    }
}

/// Chrome's helper processes (crashpad, GPU) can hold handles for a few seconds
/// after the window closes, so retry for a while before giving up; anything left
/// behind is collected by `prune_profiles` on a later run.
fn cleanup_profile(profile: &Path) {
    if !profile.exists() {
        return;
    }
    for _ in 0..15 {
        match fs::remove_dir_all(profile) {
            Ok(()) => {
                log_msg("profile dir removed");
                return;
            }
            Err(_) => sleep(Duration::from_millis(500)),
        }
    }
    log_msg("profile dir cleanup deferred (files still locked)");
}

// ---------------------------------------------------------------------------
// main flow
// ---------------------------------------------------------------------------

fn run() -> i32 {
    ensure_dir(&data_dir());
    log_msg("=== DSH launcher start ===");

    let Some(_single) = SingleInstance::acquire() else {
        return 0;
    };

    let cfg = LaunchConfig::load();

    // Housekeeping before anything else: keep the append-only logs bounded and
    // drop profile directories that earlier runs could not delete (Chrome locks).
    rotate_log(&log_path(), 1 << 20); // 1 MiB
    rotate_log(&server_log_path(), 8 << 20); // 8 MiB
    prune_profiles(Duration::from_secs(24 * 3600));
    // Reuse an existing service only when it needs no token at all (a DSH old
    // enough not to authenticate) AND it really looks like the DSH UI — an
    // unrelated app on the preferred port must not be adopted. A token-protected
    // DSH cannot be driven from outside: its token lives only in that process.
    let mut url: Option<String> = None;
    let mut bind_port: Option<u16> = None;
    if cfg.port != 0 {
        match http_get(cfg.port, 8192) {
            Some((200, body)) if cfg.ui_marker.is_empty() || body.contains(&cfg.ui_marker) => {
                log_msg(&format!(
                    "port {} already serves the DSH UI without a token; reusing it",
                    cfg.port
                ));
                url = Some(url_for(cfg.port));
            }
            Some((200, _)) => {
                log_msg(&format!(
                    "port {} answers 200 but does not contain {:?}; starting a separate instance",
                    cfg.port, cfg.ui_marker
                ));
            }
            Some((status, _)) => {
                log_msg(&format!(
                    "port {} is served but requires its own token (HTTP {status}); starting a separate instance",
                    cfg.port
                ));
            }
            None if port_open(cfg.port) => {
                log_msg(&format!(
                    "port {} is open but does not answer HTTP; starting a separate instance",
                    cfg.port
                ));
            }
            None => {
                log_msg(&format!("port {} is free; starting dsh web there", cfg.port));
                bind_port = Some(cfg.port);
            }
        }
    } else {
        log_msg("configured port is 0; letting dsh pick a free port");
    }

    let mut own_server = false;
    let mut server: Option<Child> = None;
    let mut job: Option<KillJob> = None;
    let mut server_pid: Option<u32> = None;

    if url.is_none() {
        let attempts = build_attempts(&cfg, bind_port);
        for (index, attempt) in attempts.iter().enumerate() {
            let is_last = index + 1 == attempts.len();
            log_msg(&format!(
                "launch attempt {}/{} [{}]: {}",
                index + 1,
                attempts.len(),
                attempt.label,
                attempt.cmdline
            ));

            // The job object is an extra safety net for abrupt termination (e.g.
            // the launcher itself is killed). Normal shutdown always uses
            // taskkill, so job creation is best-effort and optional.
            let attempt_job = KillJob::create();
            let (child, log_from) = match start_server(attempt_job.as_ref(), &attempt.cmdline) {
                Ok(v) => v,
                Err(e) => {
                    log_msg(&format!("failed to spawn server: {e}"));
                    show_message(
                        "DSH 启动器",
                        &format!(
                            "无法启动 DSH 服务:\n{e}\n\n当前命令:{}\n(可在 {} 中修改 command)\n\n常见原因:\n1) 未安装 Node.js 或未加入 PATH;\n2) 日志目录不可写(见下面的服务输出路径)。\n\n日志:{}\n服务输出:{}\n",
                            attempt.cmdline,
                            cfg.path.display(),
                            log_path().display(),
                            server_log_path().display()
                        ),
                    );
                    return 1;
                }
            };
            let pid = child.id();
            let mut child = child;

            match wait_ready(Some(&mut child), attempt.probe_port, log_from, &cfg) {
                WaitReady::Ready(found) => {
                    log_msg(&format!("server ready: {found}"));
                    url = Some(found);
                    own_server = true;
                    server_pid = Some(pid);
                    job = attempt_job;
                    server = Some(child);
                    break;
                }
                WaitReady::NoLaunchUrl { port } => {
                    log_msg(&format!("port {port} rejects the bare URL and no launch URL was printed"));
                    kill_tree(pid);
                    show_message(
                        "DSH 启动器",
                        &format!(
                            "dsh web 已在端口 {port} 上运行,但启动器无法取得它的访问令牌:本次输出里没有找到带 token 的地址。\n\n这通常意味着 DSH 更新后改变了输出格式。请检查配置文件的 url_marker 设置(当前为 {:?}):\n{}\n\n服务输出:\n{}\n\n日志:{}",
                            cfg.url_marker,
                            cfg.path.display(),
                            read_tail(&server_log_path(), 2000),
                            log_path().display()
                        ),
                    );
                    return 1;
                }
                WaitReady::ServerExited(st) => {
                    log_msg(&format!("attempt failed: server exited early with {st}"));
                    kill_tree(pid);
                    if is_last {
                        show_message(
                            "DSH 启动器",
                            &format!(
                                "dsh web 未能启动(进程已退出:{st})。\n\n最后的命令:{}\n(可在 {} 中修改 command / extra_args)\n\n最近的输出:\n{}\n\n完整日志:{}\n\n建议:打开一个终端手动运行以查看详细错误。",
                                attempt.cmdline,
                                cfg.path.display(),
                                read_tail(&server_log_path(), 2000),
                                server_log_path().display()
                            ),
                        );
                        return 1;
                    }
                    log_msg("retrying with a simpler command line");
                }
                WaitReady::Timeout => {
                    log_msg("server did not become ready in time");
                    kill_tree(pid);
                    show_message(
                        "DSH 启动器",
                        &format!(
                            "等待 dsh web 就绪超时({} 秒)。\n\n命令:{}\n\n最近的输出:\n{}\n\n完整日志:{}\n\n建议:打开一个终端手动运行以查看详细错误;若 DSH 更新后启动方式有变,可在 {} 中调整 command / extra_args / timeout_secs。",
                            cfg.timeout.as_secs(),
                            attempt.cmdline,
                            read_tail(&server_log_path(), 2000),
                            server_log_path().display(),
                            cfg.path.display()
                        ),
                    );
                    return 1;
                }
            }
        }
    }

    let Some(url) = url else {
        // The attempt loop always reports before exhausting its list.
        log_msg("no launch URL could be obtained");
        return 1;
    };

    let profile = fresh_profile_dir();
    let mut browser = match spawn_browser(&url, &profile, cfg.block_web_app_install) {
        Ok(b) => b,
        Err(e) => {
            log_msg(&format!("failed to open browser: {e}"));
            cleanup_profile(&profile);
            show_message(
                "DSH 启动器",
                &format!(
                    "无法打开 Chrome 浏览器:\n{e}\n\n请确认已安装 Google Chrome。\n也可以手动在浏览器中打开:\n{url}",
                ),
            );
            return 1;
        }
    };
    log_msg("browser window opened — launcher stays alive until it is closed");

    let result = wait_until_browser_closes(&mut browser, server.as_mut());

    match result {
        WaitResult::BrowserClosed => log_msg("browser closed — shutting down"),
        WaitResult::ServerDied(st) => {
            log_msg(&format!("server exited unexpectedly ({st}) while browser was open"));
            show_message(
                "DSH 启动器",
                &format!(
                    "dsh web 服务已意外退出({st})。\n\n最近输出:\n{}\n\n完整日志:{}\n",
                    read_tail(&server_log_path(), 2000),
                    server_log_path().display()
                ),
            );
            let _ = browser.kill();
            cleanup_profile(&profile);
            return 1;
        }
    }

    if own_server {
        // Only walk the process tree while our own child is still alive: if it
        // already exited its pid could in principle have been reused by then.
        let still_running = server
            .as_mut()
            .map(|s| s.try_wait().unwrap_or(None).is_none())
            .unwrap_or(false);
        if let Some(pid) = server_pid {
            if still_running {
                log_msg(&format!("stopping server tree (pid {pid})"));
                kill_tree(pid);
            } else {
                log_msg("server had already exited; nothing to stop");
            }
        }
        drop(job); // kill-on-close terminates anything still in the job
        if let Some(mut s) = server {
            let _ = s.kill();
            let _ = s.wait();
        }
    } else {
        log_msg("the reused service is left untouched");
    }

    cleanup_profile(&profile);
    log_msg("=== DSH launcher exit ===");
    0
}

fn main() {
    let code = run();
    std::process::exit(code);
}
