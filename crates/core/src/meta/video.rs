//! How long a video lasts and how big its picture is, as `ffprobe` says.
//!
//! `ffprobe` runs with the process runner: its arguments as a list, no shell, a short
//! environment, a timeout, and nothing of it left running afterwards.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use super::{Meta, Video};
use crate::ops::ResolvedProgram;
use crate::platform::process::{self, EnvPolicy, ProcessSpec};

/// The `ffprobe` to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ffprobe {
    pub program: PathBuf,
    /// Arguments put before rada's own; none for the real program. A stand-in in a test
    /// needs some to be told which part to play.
    pub lead: Vec<OsString>,
}

impl Ffprobe {
    pub fn at(found: ResolvedProgram) -> Ffprobe {
        Ffprobe {
            program: found.path,
            lead: Vec::new(),
        }
    }
}

pub(super) fn read(f: &Ffprobe, path: &Path, timeout: Duration, cancel: &dyn Fn() -> bool) -> Meta {
    let temp = std::env::temp_dir();
    let mut args = f.lead.clone();
    args.extend(
        [
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_entries",
            "format=duration:stream=codec_type,width,height,duration",
        ]
        .map(OsString::from),
    );
    // As a `file:` address, a name that starts with a dash or holds a colon is only a name.
    let mut input = OsString::from("file:");
    input.push(path.as_os_str());
    args.push(input);
    let spec = ProcessSpec {
        program: f.program.clone(),
        args,
        cwd: temp.clone(),
        env: EnvPolicy::new(temp),
        timeout,
        max_capture: process::DEFAULT_CAPTURE,
    };
    let Ok(out) = process::run(&spec, cancel, &mut |_| {}) else {
        return Meta::default();
    };
    if out.status != Some(0) || out.truncated {
        return Meta::default();
    }
    Meta {
        video: parse(&out.stdout_tail),
        ..Meta::default()
    }
}

#[derive(Deserialize, Default)]
struct Probe {
    #[serde(default)]
    format: Format,
    #[serde(default)]
    streams: Vec<Stream>,
}

#[derive(Deserialize, Default)]
struct Format {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    duration: Option<String>,
}

/// The answer of `ffprobe -print_format json`; nothing of what it does not say clearly. The
/// first JSON object printed is the answer: what comes before or after it is not read.
pub(super) fn parse(printed: &str) -> Video {
    let Some(start) = printed.find('{') else {
        return Video::default();
    };
    let mut objects = serde_json::Deserializer::from_str(&printed[start..]).into_iter::<Probe>();
    let Some(Ok(p)) = objects.next() else {
        return Video::default();
    };
    let picture = p
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video") && s.width.is_some_and(|w| w > 0));
    let seconds = p
        .format
        .duration
        .as_deref()
        .or_else(|| picture.and_then(|s| s.duration.as_deref()))
        .and_then(|d| d.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0 && *s < 1e9);
    Video {
        duration: seconds.map(Duration::from_secs_f64),
        width: picture.and_then(|s| s.width),
        height: picture.and_then(|s| s.height).filter(|h| *h > 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_of_ffprobe_is_understood() {
        let json = r#"{"programs":[],"streams":[{"codec_type":"audio"},
            {"codec_type":"video","width":1920,"height":1080}],
            "format":{"duration":"62.500000"}}"#;
        let v = parse(json);
        assert_eq!(v.duration, Some(Duration::from_millis(62_500)));
        assert_eq!((v.width, v.height), (Some(1920), Some(1080)));
    }

    #[test]
    fn a_strange_answer_gives_empty_fields() {
        assert_eq!(parse("not json"), Video::default());
        assert_eq!(parse(r#"{"format":{"duration":"N/A"}}"#), Video::default());
        assert_eq!(parse(r#"{"format":{"duration":"-3"}}"#).duration, None);
        let only_sound = r#"{"streams":[{"codec_type":"audio","duration":"3.0"}]}"#;
        assert_eq!(parse(only_sound), Video::default());
    }
}
