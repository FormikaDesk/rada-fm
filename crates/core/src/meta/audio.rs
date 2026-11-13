//! The tags of a song (ID3, Vorbis comments, MP4 atoms, APE…), through lofty.
//!
//! Only the tags are read: not the sound, which is what makes a song long, and not the cover
//! art, which is what makes a tag big.

use std::fs::File;
use std::io::BufReader;

use lofty::config::{ParseOptions, ParsingMode};
use lofty::prelude::{Accessor, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::Tag;

use super::{Audio, tidy};

pub(super) fn read(file: File) -> Audio {
    let options = ParseOptions::new()
        .read_properties(false)
        .read_cover_art(false)
        .parsing_mode(ParsingMode::Relaxed);
    let Ok(probe) = Probe::new(BufReader::new(file)).guess_file_type() else {
        return Audio::default();
    };
    let Ok(tagged) = probe.options(options).read() else {
        return Audio::default();
    };
    // The format's own tag first; another one (an ID3v1 at the end of an MP3) fills the gaps.
    let mut tags: Vec<&Tag> = tagged.primary_tag().into_iter().collect();
    tags.extend(
        tagged
            .tags()
            .iter()
            .filter(|t| Some(t.tag_type()) != tagged.primary_tag().map(|p| p.tag_type())),
    );
    let mut audio = Audio::default();
    for tag in tags {
        fill(&mut audio, tag);
    }
    audio
}

fn fill(a: &mut Audio, tag: &Tag) {
    let text = |s: Option<std::borrow::Cow<'_, str>>| s.as_deref().and_then(tidy);
    a.artist = a.artist.take().or_else(|| text(tag.artist()));
    a.title = a.title.take().or_else(|| text(tag.title()));
    a.album = a.album.take().or_else(|| text(tag.album()));
    a.track = a.track.or(tag.track().filter(|n| *n > 0));
    a.year = a
        .year
        .or(tag.date().map(|d| i32::from(d.year)).filter(|y| *y > 0));
}
