//! Is the terminal using a Nerd Font? A terminal cannot be asked, but its configuration
//! can be read: if the font it is set to use is a Nerd Font, the icons will render.

use std::path::{Path, PathBuf};

fn mentions_nerd_font(text: &str) -> bool {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && !l.starts_with("--") && !l.starts_with("//"))
        .any(|l| {
            let lower = l.to_lowercase();
            lower.contains("font")
                && (lower.contains("nerd")
                    || lower.contains(" nf")
                    || lower.contains("nfm")
                    || lower.contains("symbols"))
        })
}

fn candidates(home: &Path) -> Vec<PathBuf> {
    let c = home.join(".config");
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let (prog, term) = (env("TERM_PROGRAM").to_lowercase(), env("TERM"));
    let mut v = Vec::new();
    if prog == "ghostty" || term.contains("ghostty") {
        v.extend([c.join("ghostty/config"), c.join("ghostty/config.ghostty")]);
    }
    if std::env::var_os("KITTY_WINDOW_ID").is_some() || term.contains("kitty") {
        v.push(c.join("kitty/kitty.conf"));
    }
    if prog == "wezterm" || std::env::var_os("WEZTERM_EXECUTABLE").is_some() {
        v.extend([c.join("wezterm/wezterm.lua"), home.join(".wezterm.lua")]);
    }
    if std::env::var_os("ALACRITTY_LOG").is_some() || term.contains("alacritty") {
        v.extend([
            c.join("alacritty/alacritty.toml"),
            c.join("alacritty/alacritty.yml"),
        ]);
    }
    if term.starts_with("foot") {
        v.push(c.join("foot/foot.ini"));
    }
    v
}

/// True when the current terminal's own configuration selects a Nerd Font.
pub fn terminal_uses_nerd_font(home: &Path) -> bool {
    candidates(home)
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .any(|t| mentions_nerd_font(&t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_nerd_font_settings() {
        assert!(mentions_nerd_font(
            "font-size = 11\nfont-family = \"JetBrainsMono Nerd Font\"\n"
        ));
        assert!(mentions_nerd_font(
            "font_family      FiraCode Nerd Font Mono"
        ));
        assert!(!mentions_nerd_font("font-family = \"JetBrains Mono\"\n"));
        assert!(!mentions_nerd_font("# font-family = \"X Nerd Font\"\n"));
        assert!(!mentions_nerd_font("theme = nerd-dark"));
    }
}
