//! Console detection and interrupt handling, the two places the CLI depends on the operating system.

use std::fs::OpenOptions;
use std::path::PathBuf;

/// Whether the process can open its console (`CONIN$` on Windows, `/dev/tty`
/// elsewhere), which is where `ssh` reads password and passphrase prompts.
pub fn has_console() -> bool {
    let device = if cfg!(windows) { "CONIN$" } else { "/dev/tty" };
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(device)
        .is_ok()
}

/// The home directory: `USERPROFILE` on Windows, `HOME` elsewhere, as `ssh` expands `~`.
pub fn home_dir() -> PathBuf {
    let variable = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// Keeps a console interrupt from ending this process, so that it waits for
/// `ssh` and reports the outcome. A handler function is installed rather than
/// the interrupt being ignored, because an ignored interrupt would be inherited
/// by `ssh` and keep the user from cancelling it.
pub fn outlive_interrupts() {
    imp::outlive_interrupts();
}

#[cfg(windows)]
mod imp {
    type Handler = unsafe extern "system" fn(u32) -> i32;

    unsafe extern "system" {
        fn SetConsoleCtrlHandler(handler: Option<Handler>, add: i32) -> i32;
    }

    const CTRL_C_EVENT: u32 = 0;
    const CTRL_BREAK_EVENT: u32 = 1;

    /// Claims Ctrl-C and Ctrl-Break, and leaves other console events to the default handler.
    unsafe extern "system" fn claim_interrupt(event: u32) -> i32 {
        i32::from(event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT)
    }

    pub fn outlive_interrupts() {
        unsafe {
            SetConsoleCtrlHandler(Some(claim_interrupt), 1);
        }
    }
}

#[cfg(unix)]
mod imp {
    unsafe extern "C" {
        fn signal(signum: i32, handler: extern "C" fn(i32)) -> usize;
    }

    const SIGINT: i32 = 2;

    /// Does nothing; `exec` resets a handled signal to its default in the child.
    extern "C" fn claim_interrupt(_: i32) {}

    pub fn outlive_interrupts() {
        unsafe {
            signal(SIGINT, claim_interrupt);
        }
    }
}
