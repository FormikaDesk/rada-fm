//! Starting a program that may have been written a moment ago.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

/// A path as an argument of a program: a file, never an option. A relative path gets `./` in
/// front, so that a name like `-rf` or `--help` cannot be read as a flag; an absolute path
/// cannot start with a dash and is returned as it is.
pub fn operand(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(".").join(path)
    }
}

/// Start `cmd`. A program that was written a moment ago can still be "busy" for writing
/// ("Text file busy", ETXTBSY): another process that was being started at that very moment
/// holds a copy of the file's descriptor until it has run its own program. That passes in a
/// few milliseconds, so it is tried again rather than reported.
pub fn spawn_retrying_busy(cmd: &mut Command) -> io::Result<Child> {
    const TRIES: u32 = 40;
    let mut tried = 0;
    loop {
        match cmd.spawn() {
            Err(e) if is_text_file_busy(&e) && tried < TRIES => {
                tried += 1;
                std::thread::sleep(Duration::from_millis(25));
            }
            other => return other,
        }
    }
}

/// Whether starting a program failed because it is open for writing.
pub fn is_text_file_busy(e: &io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(libc::ETXTBSY)
    }
    #[cfg(not(unix))]
    {
        let _ = e;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_relative_path_cannot_be_read_as_an_option_and_an_absolute_one_is_left_alone() {
        for name in ["-rf", "--help", "-", "a/-b", "--"] {
            let o = operand(Path::new(name));
            assert!(o.starts_with("."), "{o:?}");
            assert_eq!(o.file_name(), Path::new(name).file_name());
        }
        let abs = std::env::temp_dir().join("-rf");
        assert_eq!(operand(&abs), abs);
    }
}
