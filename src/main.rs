//! DSH Launcher — one-click launcher for DeepSeek Harness (Windows).
//!
//! Behavior:
//! 1. If nothing is listening on 127.0.0.1:PORT yet, quietly start
//!    `npx @deepseek-ai/dsh web` with no console window ever visible.
//! 2. Poll until the server accepts TCP connections on the port.
//! 3. Open a fresh Chrome window (isolated temporary profile) at
//!    http://127.0.0.1:PORT/ and stay alive silently.
//! 4. When that Chrome window is closed, stop the server process tree and
//!    exit. A server that was already running before launch is left alone.
//!
//! Zero external dependencies: the few Win32 functions we need
//! (job objects, mutex, message box) are declared as raw `extern "system"`.
//!
//! Test-only knobs (see README): DSH_DATA_DIR, DSH_PORT, DSH_SERVER_EXTRA,
//! DSH_FAKE_CHROME, DSH_NO_UI, DSH_READY_TIMEOUT_SECS, DSH_CHROME.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::ffi::c_void;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DEFAULT_PORT: u16 = 3080;
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

// ---------------------------------------------------------------------------
// environment helpers
// ---------------------------------------------------------------------------

/// The tokenized URL `dsh web` prints on stdout. This is the only place its
/// per-process launch token can be read from: the token lives in an in-memory
/// weak map inside that node process and is never written to disk, and the bare
/// URL is rejected (401) unless a `dsh-auth-*` cookie was already minted from
/// it. Scanning only bytes appended after `from_offset` keeps a stale token from
/// a previous run from being reused.
fn find_launch_url(from_offset: u64) -> Option<String> {
    const PREFIX: &str = "http://127.0.0.1:";
    const TOKEN_SUFFIX: &str = "/?token=";
    let bytes = fs::read(server_log_path()).ok()?;
    let start = (from_offset as usize).min(bytes.len());
    let text = String::from_utf8_lossy(&bytes[start..]);
    for line in text.lines() {
        let Some(at) = line.find(PREFIX) else { continue };
        let rest = &line[at..];
        let Some(token_at) = rest.find(TOKEN_SUFFIX) else { continue };
        let digits = &rest[PREFIX.len()..token_at];
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let url = rest.split_whitespace().next().unwrap_or_default();
        if url.len() > token_at + TOKEN_SUFFIX.len() {
            return Some(url.to_string());
        }
    }
    None
}

/// Port of an `http://127.0.0.1:<port>/...` URL. Readiness has to be checked on
/// whichever port dsh actually bound, which is only known from its URL when it
/// was allowed to pick one itself (`--port 0`).
fn url_port(url: &str) -> Option<u16> {
    url.strip_prefix("http://127.0.0.1:")?.split('/').next()?.parse().ok()
}

fn port_from_env() -> u16 {
    env::var("DSH_PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(DEFAULT_PORT)
}

fn ready_timeout() -> Duration {
    let secs = env::var("DSH_READY_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(300);
    Duration::from_secs(secs)
}

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

/// Starts the server and also returns the length `server.log` had before the
/// spawn, so the launch URL can be searched for in this run's output only.
/// `port` is the port to ask dsh for, or `None` to let the OS pick a free one
/// (`--port 0`).
fn start_server(job: Option<&KillJob>, port: Option<u16>) -> std::io::Result<(Child, u64)> {
    let server_log = server_log_path();
    ensure_dir(server_log.parent().unwrap_or(Path::new(".")));
    let log_from = fs::metadata(&server_log).map(|m| m.len()).unwrap_or(0);
    let out = OpenOptions::new().create(true).append(true).open(&server_log)?;
    let err = out.try_clone()?;

    let mut args: Vec<String> = Vec::new();
    if let Ok(extra) = env::var("DSH_SERVER_EXTRA") {
        if !extra.trim().is_empty() {
            log_msg(&format!("[test] launching custom server: {extra}"));
            args.push("/C".to_string());
            args.push(extra);
        }
    }
    if args.is_empty() {
        // The exact command the user requested; --yes pre-answers npx's
        // interactive install prompt so a console-less run never hangs.
        // --no-open stops `dsh web` from ALSO opening the default browser
        // itself (its openBrowser defaults to true), which would produce a
        // second window on top of the one this launcher opens.
        args = ["/C", "npx", "--yes", "@deepseek-ai/dsh", "web", "--no-open"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Ask for the port explicitly: the port the launcher probes (and the one
        // the browser is sent to) must be the one dsh actually binds. Without
        // this, DSH_PORT only moved the probe and not the server.
        args.push("--port".to_string());
        args.push(port.map_or_else(|| "0".to_string(), |p| p.to_string()));
    }

    let mut cmd = Command::new("cmd.exe");
    cmd.args(&args);
    cmd.current_dir(
        env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".")),
    );
    cmd.creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));
    log_msg(&format!("server command: cmd {}", args.join(" ")));

    let child = cmd.spawn()?;
    log_msg(&format!("server child spawned (pid {}), waiting for the port", child.id()));
    if let Some(j) = job {
        j.assign(&child);
    }
    Ok((child, log_from))
}

enum WaitReady {
    Ready,
    ServerExited(ExitStatus),
    Timeout,
}

/// `probe_port` is the port that was asked for, or `None` when dsh picks one
/// itself — in that case its tokenized URL is the only readiness signal.
fn wait_ready(mut server: Option<&mut Child>, probe_port: Option<u16>, log_from: u64) -> WaitReady {
    let deadline = Instant::now() + ready_timeout();
    let mut port_open_since: Option<Instant> = None;
    loop {
        if let Some(url) = find_launch_url(log_from) {
            if url_port(&url).is_none_or(port_open) {
                return WaitReady::Ready;
            }
        }
        if let Some(port) = probe_port {
            if port_open(port) {
                // A dsh without token auth never prints that URL; do not wait
                // for it forever.
                let since = *port_open_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= LAUNCH_URL_GRACE {
                    log_msg("no tokenized launch URL in server output; using the bare URL");
                    return WaitReady::Ready;
                }
            }
        }
        if let Some(c) = server.as_deref_mut() {
            if let Some(st) = c.try_wait().unwrap_or(None) {
                return WaitReady::ServerExited(st);
            }
        }
        if Instant::now() >= deadline {
            return WaitReady::Timeout;
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

fn fresh_profile_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    data_dir().join("profiles").join(format!("run-{nanos}"))
}

fn spawn_browser(url: &str, profile: &Path) -> std::io::Result<Child> {
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

fn cleanup_profile(profile: &Path) {
    if !profile.exists() {
        return;
    }
    for _ in 0..10 {
        match fs::remove_dir_all(profile) {
            Ok(()) => {
                log_msg("profile dir removed");
                return;
            }
            Err(_) => sleep(Duration::from_millis(200)),
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

    let wanted_port = port_from_env();
    // A dsh already listening on the wanted port cannot be reused: every UI
    // request is authenticated with a token that exists only inside that node
    // process and is never persisted, so there is no way to drive it. Start our
    // own dsh web on an OS-picked free port instead, and leave that one alone.
    let port_busy = port_open(wanted_port);
    let bind_port = if port_busy { None } else { Some(wanted_port) };
    let url: String;
    log_msg(&format!(
        "port {wanted_port} already in use: {port_busy}{}",
        if port_busy { " — starting our own dsh web on a free port" } else { "" }
    ));

    // The job object is an extra safety net for abrupt termination (e.g. the
    // launcher itself is killed). Normal shutdown always uses taskkill below,
    // so job creation is best-effort and optional.
    let job = KillJob::create();
    let (server_child, server_log_from) = match start_server(job.as_ref(), bind_port) {
        Ok((c, log_from)) => (c, log_from),
        Err(e) => {
            log_msg(&format!("failed to spawn server: {e}"));
            show_message(
                "DSH 启动器",
                &format!(
                    "无法启动 npx @deepseek-ai/dsh web:\n{e}\n\n请确认已安装 Node.js 并已加入 PATH。\n\n日志:{}\n服务输出:{}\n",
                    log_path().display(),
                    server_log_path().display()
                ),
            );
            return 1;
        }
    };
    let server_pid = server_child.id();
    let mut server = Some(server_child);

    match wait_ready(server.as_mut(), bind_port, server_log_from) {
        WaitReady::Ready => {
            // The URL carries this process's launch token; without it dsh rejects
            // the request (401) and the browser shows an error page instead of
            // the UI.
            url = find_launch_url(server_log_from).unwrap_or_else(|| url_for(wanted_port));
            log_msg(&format!("server ready: {url}"));
        }
        WaitReady::ServerExited(st) => {
            log_msg(&format!("server exited early with {st}"));
            show_message(
                "DSH 启动器",
                &format!(
                    "dsh web 未能启动(进程已退出:{st})。\n\n最近的输出:\n{}\n\n完整日志:{}\n\n建议:打开一个终端手动运行:\nnpx @deepseek-ai/dsh web\n以查看详细错误。",
                    read_tail(&server_log_path(), 2000),
                    server_log_path().display()
                ),
            );
            return 1;
        }
        WaitReady::Timeout => {
            log_msg("server did not become ready in time");
            show_message(
                "DSH 启动器",
                &format!(
                    "等待 dsh web 就绪超时({} 秒)。\n\n最近的输出:\n{}\n\n完整日志:{}\n\n建议:打开一个终端手动运行:\nnpx @deepseek-ai/dsh web\n以查看详细错误。",
                    ready_timeout().as_secs(),
                    read_tail(&server_log_path(), 2000),
                    server_log_path().display()
                ),
            );
            return 1;
        }
    }

    let profile = fresh_profile_dir();
    let mut browser = match spawn_browser(&url, &profile) {
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
            return 1;
        }
    }

    log_msg(&format!("stopping server tree (pid {server_pid})"));
    kill_tree(server_pid);
    drop(job); // kill-on-close terminates anything still in the job
    if let Some(mut s) = server {
        let _ = s.kill();
        let _ = s.wait();
    }

    cleanup_profile(&profile);
    log_msg("=== DSH launcher exit ===");
    0
}

fn main() {
    let code = run();
    std::process::exit(code);
}
