//! What the file under the cursor says about itself, for the details pane: asked for when the
//! pane is drawn, read by the background worker (`rada_core::meta`), shown when it arrives.
//! Nothing here waits for it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use rada_core::i18n::Key;
use rada_core::meta::{Meta, MetaEvent, MetaKind};
use rada_core::model::Entry;

use super::App;

/// The file asked about, as the list saw it: a change of size or time asks again.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Asked {
    path: PathBuf,
    size: u64,
    mtime: Option<SystemTime>,
}

#[derive(Default)]
pub struct FileMeta {
    /// Grows with each request; an answer of an older one is dropped.
    epoch: u64,
    asked: Option<Asked>,
    /// The answer for the file asked about.
    shown: Option<Arc<Meta>>,
}

impl App {
    /// Ask for what `e` says about itself, unless it was already asked as it is now.
    pub(crate) fn want_meta(&mut self, e: &Entry) {
        if e.is_dir() || MetaKind::of(&e.path).is_none() {
            return;
        }
        let now = Asked {
            path: e.path.clone(),
            size: e.size,
            mtime: e.mtime,
        };
        let m = &mut self.folder_info.meta;
        if m.asked.as_ref() == Some(&now) {
            return;
        }
        m.epoch += 1;
        m.shown = None;
        self.svc.meta.newest(m.epoch);
        self.svc.meta.request(m.epoch, vec![now.path.clone()]);
        m.asked = Some(now);
    }

    pub(super) fn on_meta_event(&mut self, ev: MetaEvent) {
        let m = &mut self.folder_info.meta;
        if ev.epoch == m.epoch && m.asked.as_ref().is_some_and(|a| a.path == ev.path) {
            m.shown = Some(ev.meta);
        }
    }

    /// What is known of the file `e`, once the answer has come.
    pub fn meta_of(&self, e: &Entry) -> Option<&Meta> {
        let m = &self.folder_info.meta;
        let asked = m.asked.as_ref()?;
        if asked.path != e.path || asked.size != e.size || asked.mtime != e.mtime {
            return None;
        }
        m.shown.as_deref()
    }
}

/// The rows of the details pane for what a file says about itself, under a heading; none when
/// it says nothing worth a row. A PDF's title, author and pages are left to the preview, which
/// shows them already, and so is the size of a picture.
pub fn rows(meta: &Meta, has_dimensions: bool) -> Vec<(Key, String)> {
    let mut rows = photo_rows(meta);
    rows.extend(song_rows(meta));
    rows.extend(video_rows(meta, has_dimensions));
    if !rows.is_empty() {
        rows.insert(0, (Key::PropMetaHeading, String::new()));
    }
    rows
}

fn photo_rows(meta: &Meta) -> Vec<(Key, String)> {
    let e = &meta.exif;
    let mut rows = Vec::new();
    let taken = match (e.date, e.time) {
        (Some(d), Some(t)) => Some(format!("{d} {}", t.strftime("%H:%M:%S"))),
        (Some(d), None) => Some(d.to_string()),
        _ => None,
    };
    rows.extend(taken.map(|t| (Key::PropTaken, t)));
    rows.extend(e.camera.clone().map(|c| (Key::PropCamera, c)));
    rows.extend(e.place.clone().map(|p| (Key::PropPlace, p)));
    rows.extend(
        e.gps
            .map(|g| (Key::PropGps, format!("{:.5}, {:.5}", g.lat, g.lon))),
    );
    rows
}

fn song_rows(meta: &Meta) -> Vec<(Key, String)> {
    let a = &meta.audio;
    let mut rows = Vec::new();
    rows.extend(a.artist.clone().map(|v| (Key::PropArtist, v)));
    rows.extend(a.title.clone().map(|v| (Key::PropTitle, v)));
    rows.extend(a.album.clone().map(|v| (Key::PropAlbum, v)));
    rows.extend(a.track.map(|n| (Key::PropTrack, n.to_string())));
    rows.extend(a.year.map(|y| (Key::PropYear, y.to_string())));
    rows
}

fn video_rows(meta: &Meta, has_dimensions: bool) -> Vec<(Key, String)> {
    let v = &meta.video;
    let mut rows = Vec::new();
    rows.extend(
        v.duration
            .map(|d| (Key::PropDuration, duration(d.as_secs()))),
    );
    if !has_dimensions && let (Some(w), Some(h)) = (v.width, v.height) {
        rows.push((Key::PropDimensions, format!("{w} × {h}")));
    }
    rows
}

/// "0:42", "3:07", "1:02:05".
fn duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rada_core::meta::{Audio, Exif, Gps, Video};
    use std::time::Duration;

    #[test]
    fn a_photo_has_a_heading_and_its_rows() {
        let meta = Meta {
            exif: Exif {
                date: Some(jiff::civil::date(2024, 5, 1)),
                time: Some(jiff::civil::time(14, 30, 5, 0)),
                camera: Some("Canon EOS R6".into()),
                gps: Some(Gps {
                    lat: 41.9,
                    lon: -12.5,
                }),
                ..Exif::default()
            },
            ..Meta::default()
        };
        let r = rows(&meta, false);
        assert_eq!(r[0].0, Key::PropMetaHeading);
        assert_eq!(r[1], (Key::PropTaken, "2024-05-01 14:30:05".into()));
        assert_eq!(r[2], (Key::PropCamera, "Canon EOS R6".into()));
        assert_eq!(r[3], (Key::PropGps, "41.90000, -12.50000".into()));
    }

    #[test]
    fn nothing_worth_a_row_gives_no_heading() {
        assert!(rows(&Meta::default(), false).is_empty());
    }

    #[test]
    fn a_song_and_a_video() {
        let song = Meta {
            audio: Audio {
                artist: Some("Mina".into()),
                track: Some(7),
                ..Audio::default()
            },
            ..Meta::default()
        };
        let keys: Vec<Key> = rows(&song, false).into_iter().map(|r| r.0).collect();
        assert_eq!(
            keys,
            [Key::PropMetaHeading, Key::PropArtist, Key::PropTrack]
        );
        let video = Meta {
            video: Video {
                duration: Some(Duration::from_secs(3725)),
                width: Some(1920),
                height: Some(1080),
            },
            ..Meta::default()
        };
        assert_eq!(
            rows(&video, false)[1],
            (Key::PropDuration, "1:02:05".into())
        );
        assert_eq!(
            rows(&video, false)[2],
            (Key::PropDimensions, "1920 × 1080".into())
        );
        assert_eq!(
            rows(&video, true).len(),
            2,
            "the picture's size is shown once"
        );
        assert_eq!(duration(42), "0:42");
    }
}
