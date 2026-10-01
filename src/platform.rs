//! Console detection, interrupt handling, and the files the CLI opens on the local machine.

use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Whether `path` names a regular file this process can open for reading.
pub fn is_readable_file(path: &Path) -> bool {
    std::fs::File::open(path)
        .and_then(|file| file.metadata())
        .is_ok_and(|metadata| metadata.is_file())
}

/// Creates a new empty file in the temporary directory for one `ssh -E` log and
/// returns its path. Each call creates a different file; on Unix only the owner
/// can read or write it.
pub fn create_log_file() -> io::Result<PathBuf> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    loop {
        let path = std::env::temp_dir().join(format!(
            "ssh-copy-id-{}-{}.log",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match owner_only().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}

#[cfg(unix)]
fn owner_only() -> OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.mode(0o600);
    options
}

#[cfg(not(unix))]
fn owner_only() -> OpenOptions {
    OpenOptions::new()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ssh-copy-id-platform-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn p01_a_regular_readable_file_is_readable() {
        let dir = scratch("p01");
        let file = dir.join("key");
        std::fs::write(&file, b"private").unwrap();
        assert!(is_readable_file(&file));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn p02_a_directory_is_not_a_readable_file() {
        let dir = scratch("p02");
        assert!(!is_readable_file(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn p03_a_missing_path_is_not_a_readable_file() {
        let dir = scratch("p03");
        assert!(!is_readable_file(&dir.join("absent")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn p04_each_log_file_is_new_and_empty() {
        let first = create_log_file().unwrap();
        let second = create_log_file().unwrap();
        assert_ne!(first, second);
        for path in [&first, &second] {
            assert!(path.starts_with(std::env::temp_dir()), "{path:?}");
            assert_eq!(std::fs::read(path).unwrap(), b"");
            std::fs::remove_file(path).unwrap();
        }
    }
}
