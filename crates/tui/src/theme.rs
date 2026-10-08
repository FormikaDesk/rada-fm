//! One palette, used everywhere. Truecolor where the terminal says it supports it,
//! plain ANSI colors otherwise (so it stays readable over SSH or on a Linux console).

use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Debug)]
pub struct Theme {
    pub text: Color,
    pub subtle: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub dir: Color,
    pub link: Color,
    pub exec: Color,
    pub ok: Color,
    pub warn: Color,
    pub danger: Color,
    pub border: Color,
    pub border_focus: Color,
    /// Background of the row under the cursor.
    pub cursor_bg: Color,
    /// Background of marked (selected) rows.
    pub mark_bg: Color,
    pub gauge_empty: Color,
    pub truecolor: bool,
}

impl Theme {
    pub fn detect() -> Theme {
        let ct = std::env::var("COLORTERM")
            .unwrap_or_default()
            .to_lowercase();
        let truecolor = ct.contains("truecolor")
            || ct.contains("24bit")
            || std::env::var_os("VELA_TRUECOLOR").is_some();
        if truecolor {
            Theme::truecolor()
        } else {
            Theme::ansi()
        }
    }

    pub fn truecolor() -> Theme {
        Theme {
            text: Color::Rgb(205, 214, 244),
            subtle: Color::Rgb(166, 173, 200),
            muted: Color::Rgb(108, 112, 134),
            accent: Color::Rgb(137, 180, 250),
            accent_soft: Color::Rgb(116, 199, 236),
            dir: Color::Rgb(137, 180, 250),
            link: Color::Rgb(148, 226, 213),
            exec: Color::Rgb(166, 227, 161),
            ok: Color::Rgb(166, 227, 161),
            warn: Color::Rgb(249, 226, 175),
            danger: Color::Rgb(243, 139, 168),
            border: Color::Rgb(69, 71, 90),
            border_focus: Color::Rgb(137, 180, 250),
            cursor_bg: Color::Rgb(49, 50, 68),
            mark_bg: Color::Rgb(58, 66, 94),
            gauge_empty: Color::Rgb(49, 50, 68),
            truecolor: true,
        }
    }

    pub fn ansi() -> Theme {
        Theme {
            text: Color::Reset,
            subtle: Color::Gray,
            muted: Color::DarkGray,
            accent: Color::Blue,
            accent_soft: Color::Cyan,
            dir: Color::Blue,
            link: Color::Cyan,
            exec: Color::Green,
            ok: Color::Green,
            warn: Color::Yellow,
            danger: Color::Red,
            border: Color::DarkGray,
            border_focus: Color::Blue,
            cursor_bg: Color::DarkGray,
            mark_bg: Color::Indexed(238),
            gauge_empty: Color::DarkGray,
            truecolor: false,
        }
    }

    pub fn fg(&self, c: Color) -> Style {
        Style::default().fg(c)
    }

    pub fn base(&self) -> Style {
        Style::default().fg(self.text)
    }

    pub fn dim(&self) -> Style {
        Style::default().fg(self.muted)
    }

    pub fn title(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    pub fn key(&self) -> Style {
        Style::default()
            .fg(self.accent_soft)
            .add_modifier(Modifier::BOLD)
    }
}
