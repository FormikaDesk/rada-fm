use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::platform::Opener;
use crate::{Error, Result};

pub struct XdgOpener;

fn spawn_detached(program: &str, arg: &Path) -> Result<()> {
    let mut child = Command::new(program)
        .arg(arg)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| Error::io("start xdg-open for", arg, e))?;
    // Reap it without blocking anybody.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

impl Opener for XdgOpener {
    fn open(&self, path: &Path) -> Result<()> {
        spawn_detached("xdg-open", path)
    }

    fn reveal(&self, path: &Path) -> Result<()> {
        spawn_detached("xdg-open", path.parent().unwrap_or(path))
    }
}
