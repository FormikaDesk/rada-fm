//! Starting a program that may have been written a moment ago.

use std::io;
use std::process::{Child, Command};
use std::time::Duration;

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
