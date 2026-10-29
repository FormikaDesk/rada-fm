//! The little HTTP client updates use. Nothing is sent but the request itself and a
//! `User-Agent` of `rada/<version>`.

use super::install::create_new_file;
use crate::i18n::{Key, tf};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

pub struct Http {
    api: ureq::Agent,
    files: ureq::Agent,
}

fn agent(
    user_agent: &str,
    allow_plain: bool,
    global: Option<Duration>,
    body: Duration,
) -> ureq::Agent {
    let mut builder = ureq::Agent::config_builder();
    if allow_plain {
        // A mirror or a test server on this machine: no proxy from the environment between.
        builder = builder.proxy(None);
    }
    builder
        .user_agent(user_agent)
        // Release files come over HTTPS; plain HTTP is for a mirror or a test server that the
        // address of the API itself already said was plain.
        .https_only(!allow_plain)
        .timeout_global(global)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(20)))
        .timeout_recv_body(Some(body))
        .build()
        .into()
}

fn describe(e: ureq::Error, url: &str) -> String {
    match e {
        ureq::Error::StatusCode(404) => tf(Key::UpHttpNotFound, &[&url]),
        ureq::Error::StatusCode(c @ (403 | 429)) => tf(Key::UpHttpRefused, &[&url, &c]),
        ureq::Error::StatusCode(c) => tf(Key::UpHttpStatus, &[&url, &c]),
        other => tf(Key::UpHttpUnreachable, &[&url, &other]),
    }
}

impl Http {
    pub fn new(user_agent: &str, allow_plain: bool) -> Http {
        Http {
            api: agent(
                user_agent,
                allow_plain,
                Some(Duration::from_secs(25)),
                Duration::from_secs(20),
            ),
            files: agent(user_agent, allow_plain, None, Duration::from_secs(900)),
        }
    }

    /// A small text document (the list of releases, the checksums).
    pub fn get_text(&self, url: &str, max_bytes: u64) -> Result<String, String> {
        let mut resp = self
            .api
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .call()
            .map_err(|e| describe(e, url))?;
        resp.body_mut()
            .with_config()
            .limit(max_bytes)
            .read_to_string()
            .map_err(|e| tf(Key::UpHttpAnswer, &[&url, &e]))
    }

    /// A small file as it is, byte for byte (what a signature is checked against).
    pub fn get_bytes(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
        let mut resp = self
            .api
            .get(url)
            .header("Accept", "application/octet-stream")
            .call()
            .map_err(|e| describe(e, url))?;
        resp.body_mut()
            .with_config()
            .limit(max_bytes)
            .read_to_vec()
            .map_err(|e| tf(Key::UpHttpAnswer, &[&url, &e]))
    }

    /// Download `url` into the new file `dest`. `progress(done, total)` is called as bytes
    /// arrive; returning `false` stops the download. A file larger than `max_bytes` is
    /// refused. What is half written is removed.
    pub fn download(
        &self,
        url: &str,
        dest: &Path,
        max_bytes: u64,
        progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
    ) -> Result<(), String> {
        let mut resp = self
            .files
            .get(url)
            .header("Accept", "application/octet-stream")
            .call()
            .map_err(|e| describe(e, url))?;
        let total = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        if total.is_some_and(|t| t > max_bytes) {
            return Err(tf(Key::UpHttpLarge, &[&url, &max_bytes]));
        }
        let mut reader = resp.body_mut().with_config().limit(max_bytes).reader();
        let mut out =
            create_new_file(dest).map_err(|e| tf(Key::UpCannotCreate, &[&dest.display(), &e]))?;
        let written = write_body(&mut reader, &mut out, url, dest, total, progress);
        drop(out);
        if written.is_err() {
            let _ = fs::remove_file(dest);
        }
        written
    }
}

fn write_body(
    reader: &mut impl Read,
    out: &mut fs::File,
    url: &str,
    dest: &Path,
    total: Option<u64>,
    progress: &mut dyn FnMut(u64, Option<u64>) -> bool,
) -> Result<(), String> {
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| tf(Key::UpHttpBroke, &[&url, &e]))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .map_err(|e| tf(Key::UpCannotWrite, &[&dest.display(), &e]))?;
        done += n as u64;
        if !progress(done, total) {
            return Err("stopped".into());
        }
    }
    out.flush()
        .map_err(|e| tf(Key::UpCannotWrite, &[&dest.display(), &e]))?;
    if let Some(t) = total
        && done != t
    {
        return Err(tf(Key::UpHttpEarly, &[&url, &done, &t]));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testweb::Server;
    use std::fs;

    fn fetch(server: &Server, dest: &Path) -> Result<(), String> {
        Http::new("rada/test", true).download(&server.url("/file"), dest, 1 << 20, &mut |_, _| true)
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_place_of_the_destination_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        fs::write(&target, b"precious").unwrap();
        let dest = dir.path().join("download");
        std::os::unix::fs::symlink(&target, &dest).unwrap();
        let server = Server::start();
        server.route("/file", 200, b"new bytes".to_vec());

        assert!(fetch(&server, &dest).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"precious");
    }

    #[test]
    fn a_file_already_there_is_not_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("download");
        fs::write(&dest, b"already here").unwrap();
        let server = Server::start();
        server.route("/file", 200, b"new bytes".to_vec());

        assert!(fetch(&server, &dest).is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"already here");
    }

    #[test]
    fn a_download_that_breaks_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("download");
        let server = Server::start();
        server.route_truncated("/file", b"half of it".to_vec());

        assert!(fetch(&server, &dest).is_err());
        assert!(!dest.exists());
    }

    #[test]
    fn a_download_that_is_stopped_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("download");
        let server = Server::start();
        server.route("/file", 200, b"new bytes".to_vec());

        let stopped = Http::new("rada/test", true).download(
            &server.url("/file"),
            &dest,
            1 << 20,
            &mut |_, _| false,
        );
        assert!(stopped.is_err());
        assert!(!dest.exists());
    }

    #[test]
    fn a_whole_download_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("download");
        let server = Server::start();
        server.route("/file", 200, b"new bytes".to_vec());

        fetch(&server, &dest).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"new bytes");
    }
}
