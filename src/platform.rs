//! Console detection, interrupt handling, and the files the CLI opens on the local machine.

use std::fs::OpenOptions;
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Whether `path` names a regular file this process can open for reading.
pub fn is_readable_file(path: &Path) -> bool {
    std::fs::File::open(path)
        .and_then(|file| file.metadata())
        .is_ok_and(|metadata| metadata.is_file())
}

/// Creates a new directory named `ssh-copy-id.<random>` inside `parent` and
/// returns its path, as upstream's `mktemp -d ~/.ssh/ssh-copy-id.XXXXXXXXXX`.
/// On Unix only the owner can enter it. `parent` is not created, and an existing
/// entry is never reused.
pub fn create_scratch_dir(parent: &Path) -> io::Result<PathBuf> {
    const ATTEMPTS: usize = 100;
    for _ in 0..ATTEMPTS {
        let path = parent.join(format!("ssh-copy-id.{}", random_suffix()));
        match create_private_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "every generated name already exists",
    ))
}

/// Removes a directory and everything in it; a failure is ignored.
pub fn remove_scratch_dir(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

fn random_suffix() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    hasher.write_u64(NEXT.fetch_add(1, Ordering::Relaxed));
    format!("{:010x}", hasher.finish() & 0xff_ffff_ffff)
}

fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    owner_only(&mut builder);
    builder.create(path)
}

#[cfg(unix)]
fn owner_only(builder: &mut std::fs::DirBuilder) {
    use std::os::unix::fs::DirBuilderExt;
    builder.mode(0o700);
}

#[cfg(not(unix))]
fn owner_only(_: &mut std::fs::DirBuilder) {}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Whether a console interrupt has arrived since `outlive_interrupts` was called.
pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

fn record_interrupt() {
    INTERRUPTED.store(true, Ordering::SeqCst);
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
/// `ssh` and reports the outcome, and records the interrupt for `interrupted`.
/// A handler function is installed rather than the interrupt being ignored,
/// because an ignored interrupt would be inherited by `ssh` and keep the user
/// from cancelling it.
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

    /// Claims and records Ctrl-C and Ctrl-Break, and leaves other console events to the default handler.
    unsafe extern "system" fn claim_interrupt(event: u32) -> i32 {
        let claimed = event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT;
        if claimed {
            super::record_interrupt();
        }
        i32::from(claimed)
    }

    pub fn outlive_interrupts() {
        unsafe {
            SetConsoleCtrlHandler(Some(claim_interrupt), 1);
        }
    }

    #[cfg(test)]
    pub fn simulate_interrupt() {
        unsafe {
            claim_interrupt(CTRL_C_EVENT);
        }
    }
}

#[cfg(unix)]
mod imp {
    unsafe extern "C" {
        fn signal(signum: i32, handler: extern "C" fn(i32)) -> usize;
        #[cfg(test)]
        fn geteuid() -> u32;
    }

    const SIGINT: i32 = 2;

    /// Records the interrupt; `exec` resets a handled signal to its default in the child.
    extern "C" fn claim_interrupt(_: i32) {
        super::record_interrupt();
    }

    pub fn outlive_interrupts() {
        unsafe {
            signal(SIGINT, claim_interrupt);
        }
    }

    #[cfg(test)]
    pub fn simulate_interrupt() {
        claim_interrupt(SIGINT);
    }

    #[cfg(test)]
    pub fn is_root() -> bool {
        unsafe { geteuid() == 0 }
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
    fn p05_each_scratch_directory_is_new_and_inside_the_parent() {
        let parent = scratch("p05");
        let first = create_scratch_dir(&parent).unwrap();
        let second = create_scratch_dir(&parent).unwrap();
        assert_ne!(first, second);
        for dir in [&first, &second] {
            assert_eq!(dir.parent(), Some(parent.as_path()));
            let name = dir.file_name().unwrap().to_str().unwrap();
            assert!(name.starts_with("ssh-copy-id."), "{name}");
            assert!(std::fs::metadata(dir).unwrap().is_dir());
            assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0);
        }
        std::fs::remove_dir_all(&parent).unwrap();
    }

    #[test]
    fn p06_an_existing_name_is_not_reused() {
        let parent = scratch("p06");
        let existing = parent.join("taken");
        std::fs::create_dir(&existing).unwrap();
        assert_eq!(
            create_private_dir(&existing).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        std::fs::remove_dir_all(&parent).unwrap();
    }

    #[test]
    fn p07_a_missing_parent_is_not_created() {
        let parent = scratch("p07");
        let missing = parent.join("absent");
        assert!(create_scratch_dir(&missing).is_err());
        assert!(!missing.exists());
        std::fs::remove_dir_all(&parent).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn p08_a_scratch_directory_is_private_to_the_owner() {
        use std::os::unix::fs::PermissionsExt;
        let parent = scratch("p08");
        let dir = create_scratch_dir(&parent).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        std::fs::remove_dir_all(&parent).unwrap();
    }

    #[test]
    fn p09_removing_a_scratch_directory_removes_its_contents() {
        let parent = scratch("p09");
        let dir = create_scratch_dir(&parent).unwrap();
        std::fs::write(dir.join("check.log"), b"log").unwrap();
        remove_scratch_dir(&dir);
        assert!(!dir.exists());
        remove_scratch_dir(&dir);
        std::fs::remove_dir_all(&parent).unwrap();
    }

    #[test]
    fn p10_a_claimed_interrupt_is_recorded() {
        assert!(!interrupted());
        imp::simulate_interrupt();
        assert!(interrupted());
    }

    #[cfg(unix)]
    #[test]
    fn p11_an_unreadable_regular_file_is_not_readable() {
        use std::os::unix::fs::PermissionsExt;
        if imp::is_root() {
            return;
        }
        let dir = scratch("p11");
        let file = dir.join("key");
        std::fs::write(&file, b"private").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
        assert!(!is_readable_file(&file));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
