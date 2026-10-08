//! Colours come from the theme and nowhere else. This reads the sources and fails if a
//! colour is written by hand outside `theme.rs`.

use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn no_colour_is_written_by_hand_outside_the_theme() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    let mut offenders = Vec::new();
    for f in files {
        if f.file_name().is_some_and(|n| n == "theme.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        for (i, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            let uses = ["Color::", "Rgb(", "Indexed(", "Color::from"]
                .iter()
                .any(|n| line.contains(n));
            if !uses {
                continue;
            }
            // Allowed: transforming a colour that already came from the theme (dimming the
            // backdrop) and "no colour at all".
            let allowed = line.contains("Color::Reset")
                || line.contains("Color::Rgb(r, g, b) => Color::Rgb(")
                || (t.starts_with("fn ") && line.contains("Color,"))
                || line.contains(": Color")
                || line.contains("-> Color")
                || line.contains("Option<Color>")
                || line.contains("use ratatui");
            if !allowed {
                offenders.push(format!("{}:{}: {}", f.display(), i + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "hand-written colours:\n{}",
        offenders.join("\n")
    );
}
