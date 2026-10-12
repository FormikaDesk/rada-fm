//! The little HTTP client updates use. Nothing is sent but the request itself and a
//! `User-Agent` of `rada/<version>`.

use std::fs::File;
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
        ureq::Error::StatusCode(404) => format!("{url} was not found (404)"),
        ureq::Error::StatusCode(c @ (403 | 429)) => format!(
            "{url} refused the request ({c}); GitHub limits how often releases are read, try again later"
        ),
        ureq::Error::StatusCode(c) => format!("{url} answered with status {c}"),
        other => format!("cannot reach {url}: {other}"),
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
            .map_err(|e| format!("cannot read the answer of {url}: {e}"))
    }

    /// Download `url` into the new file `dest`. `progress(done, total)` is called as bytes
    /// arrive; returning `false` stops the download. A file larger than `max_bytes` is
    /// refused.
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
            return Err(format!(
                "{url} is larger than the {max_bytes} bytes allowed"
            ));
        }
        let mut reader = resp.body_mut().with_config().limit(max_bytes).reader();
        let mut out =
            File::create(dest).map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
        let mut buf = vec![0u8; 64 * 1024];
        let mut done = 0u64;
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| format!("the download of {url} broke off: {e}"))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])
                .map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
            done += n as u64;
            if !progress(done, total) {
                return Err("stopped".into());
            }
        }
        out.flush()
            .map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
        if let Some(t) = total
            && done != t
        {
            return Err(format!(
                "the download of {url} ended early ({done} of {t} bytes)"
            ));
        }
        Ok(())
    }
}
