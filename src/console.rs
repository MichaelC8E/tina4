// Copyright (c) 2026 Code Infinity
// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Cross-platform console helpers — Unicode icons with ASCII fallbacks for old Windows terminals.

/// Enable ANSI escape codes on Windows (virtual terminal processing) and switch
/// the console to UTF-8 output. Call once at startup. No-op on non-Windows.
pub fn enable_ansi() {
    #[cfg(target_os = "windows")]
    {
        // The `colored` crate calls this internally, but doing it explicitly
        // ensures it runs before any output. Safe to call multiple times.
        let _ = colored::control::set_virtual_terminal(true);

        // Switch the console to UTF-8 (code page 65001). Rust writes UTF-8 to
        // stdout, but a default US Windows console is cp437/cp850, so any
        // non-ASCII byte (an em dash, ellipsis, or Unicode icon) renders as
        // mojibake — the reported "Text is messed up" garbage. Setting the
        // output code page makes the console interpret our bytes correctly.
        // Raw kernel32 FFI keeps this dependency-free.
        extern "system" {
            fn SetConsoleOutputCP(wCodePageID: u32) -> i32;
        }
        const CP_UTF8: u32 = 65001;
        unsafe {
            let _ = SetConsoleOutputCP(CP_UTF8);
        }
    }
}

/// Returns true if the terminal is likely to render Unicode glyphs correctly.
fn supports_unicode() -> bool {
    if !cfg!(target_os = "windows") {
        return true;
    }
    // Windows Terminal sets WT_SESSION; VS Code sets TERM_PROGRAM
    std::env::var("WT_SESSION").is_ok() || std::env::var("TERM_PROGRAM").is_ok()
}

// ── Icon helpers ──────────────────────────────────────────────
pub fn icon_ok() -> &'static str {
    if supports_unicode() { "✓" } else { "+" }
}

pub fn icon_fail() -> &'static str {
    if supports_unicode() { "✗" } else { "x" }
}

pub fn icon_play() -> &'static str {
    if supports_unicode() { "▶" } else { ">" }
}

pub fn icon_info() -> &'static str {
    if supports_unicode() { "ℹ" } else { "i" }
}

pub fn icon_dash() -> &'static str {
    if supports_unicode() { "—" } else { "-" }
}

pub fn icon_warn() -> &'static str {
    if supports_unicode() { "⚠" } else { "!" }
}

pub fn icon_eye() -> &'static str {
    if supports_unicode() { "👁" } else { "*" }
}

/// Returns true when running on Windows.
pub fn is_windows() -> bool {
    cfg!(target_os = "windows")
}

/// Run a shell command string cross-platform.
/// On Unix uses `sh -c`, on Windows uses `cmd /C`.
pub fn shell_exec(cmd: &str) -> std::io::Result<std::process::ExitStatus> {
    if is_windows() {
        std::process::Command::new("cmd")
            .args(["/C", cmd])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
    } else {
        std::process::Command::new("sh")
            .args(["-c", cmd])
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
    }
}

/// Run a shell command string and capture output (cross-platform).
pub fn shell_output(cmd: &str) -> std::io::Result<std::process::Output> {
    if is_windows() {
        std::process::Command::new("cmd")
            .args(["/C", cmd])
            .output()
    } else {
        std::process::Command::new("sh")
            .args(["-c", cmd])
            .output()
    }
}

/// Get the correct Python command for the platform.
/// Windows only has `python`, Unix prefers `python3`.
pub fn python_cmd() -> &'static str {
    if is_windows() {
        "python"
    } else if which::which("python3").is_ok() {
        "python3"
    } else {
        "python"
    }
}

/// Find an available port starting from `start`, trying up to `max_tries` ports.
/// Returns the first available port, or the original if all are taken.
pub fn find_available_port(start: u16, max_tries: u16) -> u16 {
    for offset in 0..max_tries {
        let port = start + offset;
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    start
}

/// What `take_over_port` found on a busy port, and what it did about it.
#[derive(Debug, PartialEq, Eq)]
pub enum Takeover {
    /// The holder was this project's own Tina4 dev server; it was stopped and
    /// the port is free again.
    Reclaimed(Vec<u32>),
    /// The port is held by something that is not this project's Tina4 dev
    /// server. Nothing was signalled.
    Foreign(Vec<u32>),
    /// The port is busy but its holder could not be identified or stopped.
    Failed,
}

/// The per-port PID file a Tina4 dev server writes when it binds.
///
/// The same path every framework uses — `tina4_python/core/port_takeover.py`,
/// `Tina4/PortTakeover.php`, `lib/tina4/cli.rb`, `packages/core/src/server.ts`
/// — so a server started by any of them is recognised here.
pub fn serve_pidfile(project_dir: &std::path::Path, port: u16) -> std::path::PathBuf {
    project_dir.join("data").join(format!(".tina4-serve-{}.pid", port))
}

/// The PID recorded in a serve PID file, or None when absent or not a number.
fn read_serve_pidfile(path: &std::path::Path) -> Option<u32> {
    let text = std::fs::read_to_string(path).ok()?;
    text.split_whitespace().next()?.parse().ok()
}

/// The holders that may be signalled: those whose PID is the one the dev server
/// recorded. Everything else on the port is somebody else's.
///
/// PID 0 and 1 are never signalled whatever the file says — `kill(0, ..)`
/// signals our own process group, and a garbage file must not be able to make
/// `tina4 serve` stop itself — and neither is this process.
fn tina4_holders(holders: &[u32], recorded: Option<u32>, me: u32) -> Vec<u32> {
    match recorded {
        Some(pid) if pid > 1 && pid != me => {
            holders.iter().copied().filter(|h| *h == pid).collect()
        }
        _ => Vec::new(),
    }
}

/// PIDs listening on `port`, as the platform reports them.
fn port_listeners(port: u16) -> Vec<u32> {
    #[cfg(unix)]
    {
        // LISTEN only: without it lsof also names every client connected to the
        // port, and those were being killed along with the server.
        std::process::Command::new("lsof")
            .args(["-ti", &format!("tcp:{}", port), "-sTCP:LISTEN"])
            .output()
            .map(|o| parse_pids(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    }
    #[cfg(windows)]
    {
        std::process::Command::new("netstat")
            .arg("-ano")
            .output()
            .map(|o| parse_netstat_listeners(&String::from_utf8_lossy(&o.stdout), port))
            .unwrap_or_default()
    }
}

/// Numeric tokens only, de-duplicated: a stray word never becomes PID 0.
fn parse_pids(text: &str) -> Vec<u32> {
    let mut pids = Vec::new();
    for pid in text.split_whitespace().filter_map(|t| t.parse::<u32>().ok()) {
        if !pids.contains(&pid) {
            pids.push(pid);
        }
    }
    pids
}

/// The owning PIDs of `LISTENING` rows in `netstat -ano` output whose local
/// address ends in `:<port>`.
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_netstat_listeners(text: &str, port: u16) -> Vec<u32> {
    let suffix = format!(":{}", port);
    let mut pids = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        // Proto, Local Address, Foreign Address, State, PID
        if cols.len() == 5 && cols[0] == "TCP" && cols[1].ends_with(&suffix) && cols[3] == "LISTENING" {
            if let Ok(pid) = cols[4].parse::<u32>() {
                if !pids.contains(&pid) {
                    pids.push(pid);
                }
            }
        }
    }
    pids
}

fn stop_pid(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output();
    }
}

/// Reclaim a busy `port` only from this project's own Tina4 dev server.
///
/// This used to signal every PID on the port, with no check of what it was: an
/// unrelated server, a database, a browser tab connected to it. The framework
/// ports settled this already (TAKEOVER-DEC-01): a dev server writes
/// `data/.tina4-serve-<port>.pid` when it binds, and only the process named
/// there is taken over. Anything else is refused and left running — the worst
/// case is that the developer frees the port by hand.
pub fn take_over_port(port: u16, project_dir: &std::path::Path) -> Takeover {
    let holders = port_listeners(port);
    if holders.is_empty() {
        return Takeover::Failed;
    }
    let pidfile = serve_pidfile(project_dir, port);
    let ours = tina4_holders(&holders, read_serve_pidfile(&pidfile), std::process::id());
    if ours.is_empty() {
        return Takeover::Foreign(holders);
    }
    for pid in &ours {
        stop_pid(*pid);
    }
    let _ = std::fs::remove_file(&pidfile);
    std::thread::sleep(std::time::Duration::from_millis(500));
    if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
        Takeover::Reclaimed(ours)
    } else {
        Takeover::Failed
    }
}

/// Open the default browser to the given URL. Cross-platform.
pub fn open_browser(url: &str) {
    let _ = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
            .arg(url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    } else {
        std::process::Command::new("xdg-open")
            .arg(url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    };
}

/// Resolve a command name to its full path.
/// On Windows this is critical: `which` finds `composer.bat` but
/// `Command::new("composer")` does NOT — it only searches for `.exe`.
/// By resolving the full path first, `.bat` and `.cmd` wrappers work correctly.
pub fn resolve_cmd(cmd: &str) -> String {
    which::which(cmd)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| cmd.to_string())
}

/// Get the PHP vendor binary path.
/// Always returns the PHP script path (not the .bat wrapper),
/// since we invoke it via `php <path>`.
pub fn php_vendor_bin(name: &str) -> String {
    if is_windows() {
        format!("vendor\\bin\\{}", name)
    } else {
        format!("vendor/bin/{}", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_recorded_pid_is_ours() {
        assert_eq!(tina4_holders(&[40, 41], Some(41), 7), vec![41]);
    }

    #[test]
    fn a_holder_with_no_record_is_foreign() {
        assert!(tina4_holders(&[40], None, 7).is_empty());
    }

    #[test]
    fn a_record_naming_another_pid_makes_the_holder_foreign() {
        assert!(tina4_holders(&[40], Some(999_999), 7).is_empty());
    }

    #[test]
    fn a_garbage_record_never_names_our_group_init_or_ourselves() {
        assert!(tina4_holders(&[0, 1, 7], Some(0), 7).is_empty());
        assert!(tina4_holders(&[0, 1, 7], Some(1), 7).is_empty());
        assert!(tina4_holders(&[0, 1, 7], Some(7), 7).is_empty());
    }

    #[test]
    fn pid_tokens_are_numbers_only() {
        assert_eq!(parse_pids("123\nabc\n123\n 456 \n"), vec![123, 456]);
        assert!(parse_pids("lsof: WARNING").is_empty());
    }

    /// `netstat -ano` as Windows prints it: IPv4 and IPv6 rows, clients, UDP.
    #[test]
    fn netstat_rows_yield_listeners_on_the_port_only() {
        let out = "\r\nActive Connections\r\n\r\n  Proto  Local Address          Foreign Address        State           PID\r\n  \
                   TCP    0.0.0.0:7146           0.0.0.0:0              LISTENING       4120\r\n  \
                   TCP    127.0.0.1:7146         127.0.0.1:51000        ESTABLISHED     4120\r\n  \
                   TCP    127.0.0.1:51000        127.0.0.1:7146         ESTABLISHED     9001\r\n  \
                   TCP    0.0.0.0:71460          0.0.0.0:0              LISTENING       5000\r\n  \
                   TCP    [::]:7146              [::]:0                 LISTENING       4120\r\n  \
                   UDP    0.0.0.0:7146           *:*                                    6000\r\n";
        assert_eq!(parse_netstat_listeners(out, 7146), vec![4120]);
    }

    #[test]
    fn the_pidfile_is_where_the_frameworks_write_it() {
        assert_eq!(
            serve_pidfile(std::path::Path::new("/p"), 7146),
            std::path::Path::new("/p/data/.tina4-serve-7146.pid")
        );
    }
}
