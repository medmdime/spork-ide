//! Create Terminal's process: the IDE's own binary run in a terminal with [`EXEC_VAR`] set,
//! bridging the terminal to a shell the daemon runs in the container (an exec with a TTY over a
//! hijacked connection), as `docker exec -it` would without the Docker CLI. The terminal's keys go
//! to the shell raw, its size follows the terminal's, and it ends with the shell's exit code.

use std::io::{Read as _, Write as _};
use std::time::Duration;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::engine::Engine;

/// Set to a [`ExecRequest`] as JSON, the IDE's binary runs the bridge instead of the IDE.
pub const EXEC_VAR: &str = "SPORK_DOCKER_EXEC";

/// The shell IntelliJ's Create Terminal starts: bash where the image has it, sh otherwise.
const SHELL: [&str; 3] = [
    "/bin/sh",
    "-c",
    "if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ExecRequest {
    /// The Engine API URL and the version the IDE settled on.
    pub url: String,
    pub version: String,
    pub container: String,
    /// `root` for As Root User, empty for the container's user.
    pub user: String,
}

/// Runs the bridge and exits, when the binary was started for it.
pub fn exec_if_asked() {
    let Some(request) = std::env::var_os(EXEC_VAR) else {
        return;
    };
    let code = match run(&request.to_string_lossy()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error:#}\r");
            1
        }
    };
    std::process::exit(code);
}

fn run(request: &str) -> Result<i32> {
    let request: ExecRequest = serde_json::from_str(request).context("a bad exec request")?;
    let engine = Engine::with_version(&request.url, &request.version)?;
    let exec = engine.exec_create(&request.container, &SHELL, true, &request.user)?;
    let hijacked = engine.exec_start(&exec, true)?;
    let raw = RawMode::enable();
    if let Some((rows, columns)) = terminal_size() {
        engine.exec_resize(&exec, rows, columns)?;
    }
    std::thread::spawn({
        let (engine, exec) = (engine.clone(), exec.clone());
        move || follow_size(&engine, &exec)
    });
    let mut input = hijacked.input;
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let mut buffer = [0u8; 4096];
        while let Ok(read @ 1..) = stdin.read(&mut buffer) {
            if input.write_all(&buffer[..read]).is_err() {
                break;
            }
        }
    });
    let mut output = hijacked.output;
    let mut stdout = std::io::stdout();
    let mut buffer = [0u8; 8192];
    loop {
        match output.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                stdout.write_all(&buffer[..read])?;
                stdout.flush()?;
            }
        }
    }
    drop(raw);
    let code = engine.exec_exit_code(&exec)?;
    Ok(code.and_then(|code| i32::try_from(code).ok()).unwrap_or(0))
}

/// Resizes the shell's TTY when the terminal's size changes.
// shortcut: polls the size four times a second; a SIGWINCH handler would answer at once
fn follow_size(engine: &Engine, exec: &str) {
    let mut last = terminal_size();
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let size = terminal_size();
        if size != last {
            if let Some((rows, columns)) = size
                && engine.exec_resize(exec, rows, columns).is_err()
            {
                return;
            }
            last = size;
        }
    }
}

#[cfg(unix)]
fn terminal_size() -> Option<(u16, u16)> {
    let mut size = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: TIOCGWINSZ writes a `winsize` into the one passed
    let result = unsafe { libc::ioctl(libc::STDIN_FILENO, libc::TIOCGWINSZ, &mut size) };
    (result == 0 && size.ws_row > 0).then_some((size.ws_row, size.ws_col))
}

/// The terminal in raw mode (no echo, no line editing: the container's TTY does both) until
/// dropped.
#[cfg(unix)]
struct RawMode(Option<libc::termios>);

#[cfg(unix)]
impl RawMode {
    fn enable() -> RawMode {
        // SAFETY: `termios` is plain data that tcgetattr fills in
        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        // SAFETY: stdin's descriptor and a valid `termios` to fill
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut saved) } != 0 {
            return RawMode(None);
        }
        let mut raw = saved;
        // SAFETY: cfmakeraw only changes the flags of the `termios` given
        unsafe { libc::cfmakeraw(&mut raw) };
        // SAFETY: stdin's descriptor and a `termios` tcgetattr gave
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) } != 0 {
            return RawMode(None);
        }
        RawMode(Some(saved))
    }
}

#[cfg(unix)]
impl Drop for RawMode {
    fn drop(&mut self) {
        if let Some(saved) = &self.0 {
            // SAFETY: restores the attributes tcgetattr gave
            unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, saved) };
        }
    }
}

// shortcut: the Windows console's size and raw mode aren't read, so the container's shell keeps
// its default size and the console echoes; read them with the console API when Windows is supported.
#[cfg(not(unix))]
fn terminal_size() -> Option<(u16, u16)> {
    None
}

#[cfg(not(unix))]
struct RawMode;

#[cfg(not(unix))]
impl RawMode {
    fn enable() -> RawMode {
        RawMode
    }
}
