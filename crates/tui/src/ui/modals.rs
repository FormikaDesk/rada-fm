//! Windows in front of the interface: the plan, the jump palette, errors, prompts.
//! One window at a time, centred, with everything behind it dimmed.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph};
use unicode_width::UnicodeWidthStr;
use vela_core::display;
use vela_core::fs::FileKind;
use vela_core::journal::{EntryStatus, UndoState};
use vela_core::ops::{ConflictPolicy, ItemAction, OpKind, RunStatus, Severity, Step};

use super::widgets::{SPIN, button, centered, hit_spans, pad, pad_left, tail};
use crate::app::*;
use crate::fmt;
use crate::hits::{Hits, Target};
use crate::keymap::{Action, Keymap, Scheme};
use crate::palette::{PaletteKind, PaletteView};
use crate::theme::Theme;
use crossterm::event::KeyCode;

/// Frame of a window; returns the usable inner area.
fn frame(f: &mut Frame, th: &Theme, r: Rect, border: Color) -> Rect {
    f.render_widget(Clear, r);
    let blk = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(th.fg(border))
        .padding(Padding::new(2, 2, 1, 1));
    let inner = blk.inner(r);
    f.render_widget(blk, r);
    inner
}

fn chip<'a>(th: &Theme, text: &str, color: Color) -> Span<'a> {
    Span::styled(
        format!(" {text} "),
        Style::default()
            .fg(th.on_accent)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

pub fn draw_modal(f: &mut Frame, app: &mut App, area: Rect) {
    let th = app.th.clone();
    let home = app.home().to_path_buf();
    let spinner = SPIN[app.spinner % SPIN.len()];
    let Some(modal) = &app.modal else { return };
    match modal {
        Modal::Scanning {
            what,
            files,
            dirs,
            bytes,
            current,
            ..
        } => {
            let r = centered(area, 66, 9);
            let inner = frame(f, &th, r, th.accent);
            let lines = vec![
                Line::from(vec![
                    Span::styled(format!("{spinner} "), th.accent_style()),
                    Span::styled(what.clone(), th.base().add_modifier(Modifier::BOLD)),
                ]),
                Line::raw(""),
                Line::from(Span::styled(
                    format!(
                        "{} files · {} folders · {}",
                        fmt::thousands(*files),
                        fmt::thousands(*dirs),
                        fmt::size(*bytes)
                    ),
                    th.base(),
                )),
                Line::from(Span::styled(
                    tail(&display::path(current), inner.width as usize),
                    th.dim(),
                )),
                Line::raw(""),
                Line::from(vec![
                    Span::styled("Esc", th.key()),
                    Span::styled(" cancel", th.dim()),
                ]),
            ];
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::Plan(pv) => draw_plan(f, &th, pv, &home, area, &mut app.hits),
        Modal::Palette(p) => draw_palette(f, &th, p, area, &mut app.hits),
        Modal::Failure { info, .. } => {
            let w = 88.min(area.width.saturating_sub(4));
            let msg = fmt::wrap(&info.message, (w as usize).saturating_sub(6));
            let h = (msg.len() as u16 + 9).min(area.height.saturating_sub(2));
            let r = centered(area, w, h);
            let inner = frame(f, &th, r, th.error);
            let mut lines = vec![
                Line::from(vec![
                    chip(&th, "Error", th.error),
                    Span::styled(
                        "  Something went wrong",
                        th.base().add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::raw(""),
            ];
            for l in msg {
                lines.push(Line::from(Span::styled(l, th.fg(th.error))));
            }
            if info.attempt > 1 {
                lines.push(Line::from(Span::styled(
                    format!("attempt {}", info.attempt),
                    th.dim(),
                )));
            }
            lines.push(Line::raw(""));
            let btns = vec![
                button(&th, "s", "Skip", Some(th.accent), true),
                Span::raw("  "),
                button(&th, "S", "Skip all", None, true),
                Span::raw("  "),
                button(&th, "r", "Retry", None, true),
                Span::raw("  "),
                button(&th, "a", "Abort", None, true),
            ];
            hit_spans(
                &mut app.hits,
                inner.x,
                inner.y + lines.len() as u16,
                &btns,
                &[
                    (0, Target::Key(KeyCode::Char('s'))),
                    (2, Target::Key(KeyCode::Char('S'))),
                    (4, Target::Key(KeyCode::Char('r'))),
                    (6, Target::Key(KeyCode::Char('a'))),
                ],
            );
            lines.push(Line::from(btns));
            lines.push(Line::from(Span::styled(
                "Abort keeps what is already done; u undoes it.",
                th.faint(),
            )));
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::Input(iv) => draw_input(f, &th, iv, area),
        Modal::History(h) => draw_history(f, &th, h, area, spinner, &mut app.hits),
        Modal::Result(r) => {
            let w = 90.min(area.width.saturating_sub(4));
            let wrapped: Vec<(ToastKind, String, bool)> = r
                .lines
                .iter()
                .flat_map(|(k, t)| {
                    fmt::wrap(t, (w as usize).saturating_sub(8))
                        .into_iter()
                        .enumerate()
                        .map(move |(i, l)| (*k, l, i == 0))
                })
                .collect();
            let h = (wrapped.len() as u16 + 8).min(area.height.saturating_sub(2));
            let rect = centered(area, w, h);
            let inner = frame(f, &th, rect, th.warn);
            let title = display::truncate(&r.title, (w as usize).saturating_sub(10));
            let mut lines = vec![
                Line::from(vec![
                    chip(&th, "Done", th.warn),
                    Span::styled(format!("  {title}"), th.base().add_modifier(Modifier::BOLD)),
                ]),
                Line::raw(""),
            ];
            let rows = inner.height.saturating_sub(5) as usize;
            for (k, t, first) in wrapped.iter().skip(r.scroll).take(rows) {
                let c = if *k == ToastKind::Error {
                    th.error
                } else {
                    th.warn
                };
                lines.push(Line::from(vec![
                    Span::styled(if *first { "• " } else { "  " }, th.fg(c)),
                    Span::styled(t.clone(), th.fg(c)),
                ]));
            }
            lines.push(Line::raw(""));
            let btns = vec![
                button(&th, "u", "Undo what was done", Some(th.accent), true),
                Span::raw("  "),
                button(&th, "Esc", "Close", None, true),
            ];
            hit_spans(
                &mut app.hits,
                inner.x,
                inner.y + lines.len() as u16,
                &btns,
                &[
                    (0, Target::Key(KeyCode::Char('u'))),
                    (2, Target::Key(KeyCode::Esc)),
                ],
            );
            lines.push(Line::from(btns));
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::ConfirmQuit => {
            let r = centered(area, 66, 9);
            let inner = frame(f, &th, r, th.warn);
            let lines = vec![
                Line::from(vec![
                    chip(&th, "Running", th.warn),
                    Span::styled(
                        "  An operation is in progress",
                        th.base().add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::raw(""),
                Line::from(Span::styled(
                    "Quitting cancels it. Steps already finished stay in the",
                    th.dim(),
                )),
                Line::from(Span::styled("journal and can be undone.", th.dim())),
                Line::raw(""),
                Line::from(vec![
                    button(&th, "y", "Quit anyway", Some(th.warn), true),
                    Span::raw("  "),
                    button(&th, "Esc", "Stay", None, true),
                ]),
            ];
            let btns = vec![
                button(&th, "y", "Quit anyway", Some(th.warn), true),
                Span::raw("  "),
                button(&th, "Esc", "Stay", None, true),
            ];
            hit_spans(
                &mut app.hits,
                inner.x,
                inner.y + 5,
                &btns,
                &[
                    (0, Target::Key(KeyCode::Char('y'))),
                    (2, Target::Key(KeyCode::Esc)),
                ],
            );
            f.render_widget(Paragraph::new(lines), inner);
        }
        Modal::Help => draw_help(f, &th, area, &app.keymap, app.mouse, app.help_scroll),
        Modal::Menu(m) => draw_menu(f, &th, &app.keymap, m, area, &mut app.hits),
    }
}

// ---------------------------------------------------------------------------------- plan

fn verb(kind: OpKind) -> &'static str {
    match kind {
        OpKind::Copy => "Copy",
        OpKind::Move => "Move",
        OpKind::Rename | OpKind::BulkRename => "Rename",
        OpKind::MakeDir => "New folder",
        OpKind::Trash => "Trash",
        OpKind::Delete => "Delete",
        OpKind::Undo => "Undo",
    }
}

fn action_color(th: &Theme, a: ItemAction) -> Color {
    match a {
        ItemAction::Copy | ItemAction::Move | ItemAction::Create | ItemAction::Rename => th.accent,
        ItemAction::Merge | ItemAction::KeepBoth => th.kinds.vector,
        ItemAction::Skip | ItemAction::Overwrite => th.warn,
        ItemAction::Trash | ItemAction::Delete => th.error,
    }
}

fn draw_plan(
    f: &mut Frame,
    th: &Theme,
    pv: &PlanView,
    home: &std::path::Path,
    area: Rect,
    hits: &mut Hits,
) {
    let plan = &pv.plan;
    let blocked = !plan.is_executable() && plan.blocking().next().is_some();
    let delete = plan.kind == OpKind::Delete;
    let tone = if blocked || delete {
        th.error
    } else {
        th.accent
    };
    let w = 108.min(area.width.saturating_sub(6));
    let inner_w = (w as usize).saturating_sub(6);

    let mut body: Vec<Line> = Vec::new();
    let mut seg_line: Option<usize> = None;
    let mut seg_hits: Vec<(usize, Target)> = Vec::new();
    let mut seg_spans: Vec<Span> = Vec::new();

    // Heading: what kind of operation and how many items. Every path below is relative
    // to the bases named once here.
    let bases = Bases::of(plan, home);
    let (head, titled_path) = split_title(&plan.title);
    body.push(Line::from(vec![chip(th, verb(plan.kind), tone)]));
    body.push(Line::from(Span::styled(
        display::truncate(&head.replace(&display::path(home), "~"), inner_w),
        th.base().add_modifier(Modifier::BOLD),
    )));
    let base_line = |label: &str, p: &std::path::Path| {
        Line::from(vec![
            Span::styled(format!("{label:<6}"), th.dim()),
            Span::styled(
                tail(&fmt::short_path(p, home), inner_w.saturating_sub(6)),
                th.base(),
            ),
        ])
    };
    match (&bases.src, &bases.dst) {
        (Some(s), Some(d)) => {
            body.push(base_line("From", s));
            body.push(base_line("To", d));
        }
        (Some(s), None) => body.push(base_line("In", s)),
        (None, Some(d)) => body.push(base_line("In", d)),
        (None, None) => {
            if let Some(p) = &titled_path {
                body.push(base_line("In", std::path::Path::new(p)));
            }
        }
    }
    body.push(Line::raw(""));

    // Totals, big and calm: the number above, the label below.
    let t = &plan.totals;
    let mut stats: Vec<(String, &str, bool)> = Vec::new();
    match plan.kind {
        OpKind::Rename | OpKind::BulkRename | OpKind::MakeDir | OpKind::Undo => {
            stats.push((
                fmt::thousands(plan.steps.len() as u64),
                if plan.steps.len() == 1 {
                    "step"
                } else {
                    "steps"
                },
                true,
            ));
        }
        _ => {
            if t.files > 0 {
                stats.push((
                    fmt::thousands(t.files),
                    if t.files == 1 { "file" } else { "files" },
                    false,
                ));
            }
            if t.dirs > 0 {
                stats.push((
                    fmt::thousands(t.dirs),
                    if t.dirs == 1 { "folder" } else { "folders" },
                    false,
                ));
            }
            if t.symlinks > 0 {
                stats.push((
                    fmt::thousands(t.symlinks),
                    if t.symlinks == 1 { "link" } else { "links" },
                    false,
                ));
            }
            if t.bytes > 0 || stats.is_empty() {
                stats.push((
                    fmt::size(t.bytes),
                    if delete {
                        "freed"
                    } else if plan.kind == OpKind::Trash {
                        "to the trash"
                    } else {
                        "in total"
                    },
                    true,
                ));
            }
        }
    }
    let col_w = 14usize;
    let mut nums: Vec<Span> = Vec::new();
    let mut labels: Vec<Span> = Vec::new();
    for (n, l, primary) in &stats {
        let st = if *primary {
            Style::default().fg(tone).add_modifier(Modifier::BOLD)
        } else {
            th.base().add_modifier(Modifier::BOLD)
        };
        nums.push(Span::styled(pad(n, col_w), st));
        labels.push(Span::styled(pad(l, col_w), th.dim()));
    }
    body.push(Line::from(nums));
    body.push(Line::from(labels));
    if plan.total_bytes() > 0 && plan.total_bytes() != t.bytes {
        body.push(Line::from(Span::styled(
            format!("{} will actually be written", fmt::size(plan.total_bytes())),
            th.faint(),
        )));
    }
    body.push(Line::raw(""));

    // Warnings, strongest first, with a few examples each.
    for wn in &plan.warnings {
        let (g, c) = match wn.severity {
            Severity::Blocking => ("✖", th.error),
            Severity::Warning => ("▲", th.warn),
            Severity::Info => ("●", th.text_dim),
        };
        for (i, l) in fmt::wrap(&wn.message, inner_w.saturating_sub(3))
            .into_iter()
            .enumerate()
        {
            body.push(Line::from(vec![
                Span::styled(if i == 0 { format!("{g} ") } else { "  ".into() }, th.fg(c)),
                Span::styled(l, th.fg(c)),
            ]));
        }
        for ex in wn.examples.iter().take(3) {
            body.push(Line::from(Span::styled(
                format!("    {}", tail(&bases.rel(ex), inner_w.saturating_sub(4))),
                th.faint(),
            )));
        }
    }
    if !plan.warnings.is_empty() {
        body.push(Line::raw(""));
    }

    // How name clashes are settled: a segmented control.
    if matches!(pv.replan, Replan::Transfer { .. }) {
        seg_line = Some(body.len());
        let mut seg: Vec<Span> = vec![Span::styled("If a name already exists   ", th.dim())];
        for p in [
            ConflictPolicy::Skip,
            ConflictPolicy::KeepBoth,
            ConflictPolicy::Overwrite,
        ] {
            let label = match p {
                ConflictPolicy::Skip => "skip",
                ConflictPolicy::KeepBoth => "keep both",
                ConflictPolicy::Overwrite => "overwrite",
            };
            seg_hits.push((seg.len(), Target::Policy(p)));
            if plan.policy == p {
                seg.push(Span::styled(
                    format!(" {label} "),
                    Style::default()
                        .fg(th.on_accent)
                        .bg(th.accent)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                seg.push(Span::styled(format!(" {label} "), th.dim()));
            }
        }
        seg.push(Span::styled("   c to change", th.faint()));
        seg_spans = seg.clone();
        body.push(Line::from(seg));
        if plan.policy == ConflictPolicy::Overwrite {
            body.push(Line::from(Span::styled(
                "replaced items go to the trash first, so undo brings them back",
                th.faint(),
            )));
        }
        body.push(Line::raw(""));
    }

    // The overview: one line per selected item (or every step, on request).
    if !plan.renames.is_empty() {
        body.push(Line::from(Span::styled(
            "Renames",
            th.dim().add_modifier(Modifier::BOLD),
        )));
        for (from, to) in plan.renames.iter().take(400) {
            let a = from.file_name().map(display::name).unwrap_or_default();
            let b = to.file_name().map(display::name).unwrap_or_default();
            let half = inner_w.saturating_sub(5) / 2;
            body.push(Line::from(vec![
                Span::styled(display::truncate(&a, half), th.dim()),
                Span::styled("  →  ", th.accent_style()),
                Span::styled(display::truncate(&b, half), th.base()),
            ]));
        }
    } else if !pv.details && !plan.items.is_empty() {
        body.push(Line::from(vec![
            Span::styled("What will happen", th.dim().add_modifier(Modifier::BOLD)),
            Span::styled(
                if plan.steps.len() > plan.items.len() {
                    "   Tab: every step"
                } else {
                    ""
                },
                th.faint(),
            ),
        ]));
        for it in plan.items.iter().take(300) {
            let is_dir = it.kind == FileKind::Dir;
            let name = it
                .path
                .file_name()
                .map(display::name)
                .unwrap_or_else(|| display::path(&it.path));
            let name = if is_dir { format!("{name}/") } else { name };
            let tag = it.action.verb();
            let mut counts: Vec<String> = Vec::new();
            if it.files > 0 && (is_dir || it.files > 1) {
                counts.push(fmt::count(it.files, "file", "files"));
            }
            if it.dirs > 1 {
                counts.push(fmt::count(it.dirs - 1, "folder", "folders"));
            }
            if it.symlinks > 0 && is_dir {
                counts.push(fmt::count(it.symlinks, "link", "links"));
            }
            let counts = counts.join(" · ");
            let size = if it.bytes > 0 {
                fmt::size(it.bytes)
            } else {
                String::new()
            };
            let tag_w = 10;
            let size_w = 10;
            let counts_w = 34.min(inner_w / 3);
            let name_w = inner_w.saturating_sub(2 + tag_w + size_w + counts_w + 2);
            let (g, gc) = if is_dir {
                ("▸ ", th.kinds.folder)
            } else {
                ("· ", th.muted)
            };
            let mut spans = vec![
                Span::styled(g, th.fg(gc)),
                Span::styled(
                    pad(&display::truncate(&name, name_w), name_w),
                    if is_dir {
                        th.fg(th.kinds.folder)
                    } else {
                        th.base()
                    },
                ),
                Span::styled(pad(tag, tag_w), th.fg(action_color(th, it.action))),
                Span::styled(
                    pad_left(&display::truncate(&counts, counts_w), counts_w),
                    th.dim(),
                ),
                Span::styled(pad_left(&size, size_w), th.base()),
            ];
            if let Some(tg) = &it.target {
                if it.action == ItemAction::KeepBoth {
                    spans.push(Span::styled(
                        format!(
                            "  as {}",
                            tg.file_name().map(display::name).unwrap_or_default()
                        ),
                        th.faint(),
                    ));
                }
            }
            body.push(Line::from(spans));
        }
        if plan.items.len() > 300 {
            body.push(Line::from(Span::styled(
                format!("… and {} more", plan.items.len() - 300),
                th.faint(),
            )));
        }
    } else if !plan.steps.is_empty() {
        let n = plan.steps.len() as u64;
        body.push(Line::from(vec![
            Span::styled(
                format!("What will happen ({})", fmt::count(n, "step", "steps")),
                th.dim().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if !plan.items.is_empty() {
                    "   Tab: summary"
                } else {
                    ""
                },
                th.faint(),
            ),
        ]));
        let mut shown = 0;
        for s in &plan.steps {
            let Some(label) = step_label(s, &bases) else {
                continue;
            };
            if shown == 500 {
                body.push(Line::from(Span::styled(
                    "… more steps not listed",
                    th.faint(),
                )));
                break;
            }
            body.push(Line::from(Span::styled(tail(&label, inner_w), th.dim())));
            shown += 1;
        }
    }

    // Footer: the typed confirmation for deletions, then the buttons.
    let mut foot: Vec<Line> = Vec::new();
    if delete {
        foot.push(Line::from(vec![
            Span::styled(
                "This cannot be undone.  ",
                Style::default().fg(th.error).add_modifier(Modifier::BOLD),
            ),
            Span::styled("Type ", th.dim()),
            Span::styled("yes", th.key()),
            Span::styled(" to confirm: ", th.dim()),
            Span::styled(pv.typed.clone(), th.base()),
            Span::styled("▏", th.accent_style()),
        ]));
    }
    let can_run = pv.can_run();
    let mut buttons: Vec<Span> = Vec::new();
    let mut button_hits: Vec<(usize, Target)> = Vec::new();
    if pv.replanning.is_some() {
        buttons.push(Span::styled("re-planning…   ", th.fg(th.warn)));
    }
    if blocked {
        buttons.push(Span::styled(
            "Blocked: nothing will be done   ",
            th.fg(th.error),
        ));
    } else {
        if can_run {
            button_hits.push((buttons.len(), Target::Key(KeyCode::Enter)));
        }
        buttons.push(button(th, "Enter", "Run", Some(tone), can_run));
        buttons.push(Span::raw("  "));
    }
    button_hits.push((buttons.len(), Target::Key(KeyCode::Esc)));
    buttons.push(button(th, "Esc", "Cancel", None, true));
    if !delete {
        buttons.push(Span::styled("      ↑↓ scroll", th.faint()));
    }
    foot.push(Line::raw(""));
    foot.push(Line::from(buttons.clone()));

    let foot_h = foot.len() as u16;
    let want = body.len() as u16 + foot_h + 4;
    let h = want.clamp(14, area.height.saturating_sub(2));
    let r = centered(area, w, h);
    let inner = frame(f, th, r, tone);
    let content_h = inner.height.saturating_sub(foot_h) as usize;
    let max_scroll = body.len().saturating_sub(content_h);
    let scroll = pv.scroll.min(max_scroll);
    let shown: Vec<Line> = body.into_iter().skip(scroll).take(content_h).collect();
    f.render_widget(
        Paragraph::new(shown),
        Rect {
            height: content_h as u16,
            ..inner
        },
    );
    let foot_y = inner.y + content_h as u16;
    f.render_widget(
        Paragraph::new(foot),
        Rect {
            y: foot_y,
            height: foot_h,
            ..inner
        },
    );
    // Clickable: the buttons (last line of the footer) and the conflict control.
    hit_spans(hits, inner.x, foot_y + foot_h - 1, &buttons, &button_hits);
    if let Some(line) = seg_line {
        if line >= scroll && line < scroll + content_h {
            hit_spans(
                hits,
                inner.x,
                inner.y + (line - scroll) as u16,
                &seg_spans,
                &seg_hits,
            );
        }
    }
}

/// The two folders a plan is about, named once at the top; everything else is shown
/// relative to them (`→` marks something that lives in the destination).
struct Bases {
    src: Option<std::path::PathBuf>,
    dst: Option<std::path::PathBuf>,
    home: std::path::PathBuf,
}

fn common_parent<'a>(
    paths: impl Iterator<Item = &'a std::path::Path>,
) -> Option<std::path::PathBuf> {
    let mut common: Option<std::path::PathBuf> = None;
    for p in paths {
        let parent = p.parent()?.to_path_buf();
        common = Some(match common {
            None => parent,
            Some(c) => c
                .ancestors()
                .find(|a| parent.starts_with(a))
                .map(|a| a.to_path_buf())?,
        });
    }
    common
}

impl Bases {
    fn of(plan: &vela_core::ops::Plan, home: &std::path::Path) -> Bases {
        let from_items = common_parent(plan.items.iter().map(|i| i.path.as_path()));
        let from_steps = || {
            common_parent(
                plan.steps
                    .iter()
                    .filter(|s| !matches!(s, Step::FinishDir { .. }))
                    .map(|s| s.path()),
            )
        };
        let (src, dst) = match plan.kind {
            OpKind::Copy | OpKind::Move => {
                (from_items.or_else(from_steps), plan.destination.clone())
            }
            OpKind::MakeDir => (None, plan.destination.clone()),
            _ => (from_items.or_else(from_steps), None),
        };
        Bases {
            src,
            dst,
            home: home.to_path_buf(),
        }
    }

    /// Relative to the source base, or to the destination (with a `→`), or abbreviated.
    fn rel(&self, p: &std::path::Path) -> String {
        if let Some(Ok(r)) = self.dst.as_deref().map(|d| p.strip_prefix(d)) {
            return format!("→ {}", display::path(r));
        }
        self.rel_src(p)
    }

    fn rel_src(&self, p: &std::path::Path) -> String {
        match self.src.as_deref().map(|s| p.strip_prefix(s)) {
            Some(Ok(r)) if !r.as_os_str().is_empty() => display::path(r),
            _ => fmt::short_path(p, &self.home),
        }
    }

    /// Inside the destination, without the arrow (steps all live there).
    fn rel_dst(&self, p: &std::path::Path) -> String {
        match self.dst.as_deref().map(|d| p.strip_prefix(d)) {
            Some(Ok(r)) => display::path(r),
            _ => self.rel_src(p),
        }
    }
}

/// "Copy 5 items to /home/x/backup" -> ("Copy 5 items", Some("/home/x/backup")).
fn split_title(title: &str) -> (String, Option<String>) {
    for marker in [" to /", " to ~"] {
        if let Some(i) = title.rfind(marker) {
            return (title[..i].to_string(), Some(title[i + 4..].to_string()));
        }
    }
    (title.to_string(), None)
}

/// One line per step, paths relative to the plan's bases.
fn step_label(step: &Step, b: &Bases) -> Option<String> {
    Some(match step {
        Step::MakeDir { path, .. } => format!("folder   {}", b.rel_dst(path)),
        Step::FinishDir { .. } => return None,
        Step::CopyFile {
            dst, remove_source, ..
        } => format!(
            "{}   {}",
            if *remove_source { "move  " } else { "copy  " },
            b.rel_dst(dst)
        ),
        Step::CopySymlink {
            dst,
            target,
            remove_source,
            ..
        } => format!(
            "{}   {} → {}",
            if *remove_source { "link↪ " } else { "link  " },
            b.rel_dst(dst),
            display::path(target)
        ),
        Step::Rename { from, to } => format!("rename   {} → {}", b.rel_src(from), b.rel(to)),
        Step::TrashItem { path } => format!("trash    {}", b.rel_src(path)),
        Step::RemoveFile { path, .. } => format!("delete   {}", b.rel_src(path)),
        Step::RemoveDir { path } => format!("remove   {}/", b.rel_src(path)),
        Step::Restore { item } => format!("restore  {}", b.rel_src(&item.original)),
    })
}

// -------------------------------------------------------------------------------- palette

fn draw_palette(f: &mut Frame, th: &Theme, p: &PaletteView, area: Rect, hits: &mut Hits) {
    let w = 78.min(area.width.saturating_sub(6));
    let rows_wanted = p.hits.len().clamp(3, 12) as u16;
    let h = (rows_wanted + 7).min(area.height.saturating_sub(2));
    // A little above the middle, like a command palette.
    let mut r = centered(area, w, h);
    r.y = r.y.saturating_sub(area.height / 8);
    let inner = frame(f, th, r, th.accent);
    let iw = inner.width as usize;

    // Input line.
    let before: String = p.query.chars().take(p.cursor).collect();
    let at: String = p
        .query
        .chars()
        .nth(p.cursor)
        .map(String::from)
        .unwrap_or_else(|| " ".into());
    let after: String = p.query.chars().skip(p.cursor + 1).collect();
    let mut input = vec![Span::styled(
        "⌕  ",
        th.accent_style().add_modifier(Modifier::BOLD),
    )];
    if p.query.is_empty() {
        input.push(Span::styled(
            " ",
            Style::default().add_modifier(Modifier::REVERSED),
        ));
        input.push(Span::styled(
            "Jump to a folder, bookmark or disk…",
            th.faint(),
        ));
    } else {
        input.push(Span::styled(before, th.base()));
        input.push(Span::styled(
            at,
            Style::default().add_modifier(Modifier::REVERSED),
        ));
        input.push(Span::styled(after, th.base()));
    }
    f.render_widget(
        Paragraph::new(Line::from(input)),
        Rect { height: 1, ..inner },
    );
    f.render_widget(
        Paragraph::new(Span::styled("─".repeat(iw), th.faint())),
        Rect {
            y: inner.y + 1,
            height: 1,
            ..inner
        },
    );

    let list = Rect {
        y: inner.y + 2,
        height: inner.height.saturating_sub(4),
        ..inner
    };
    let visible = list.height as usize;
    let start = p.selected.saturating_sub(visible.saturating_sub(1));
    let mut lines: Vec<Line> = Vec::new();
    if p.hits.is_empty() {
        lines.push(Line::from(Span::styled(
            "No matches. Type a path starting with / or ~ to go anywhere.",
            th.dim(),
        )));
    }
    for (i, hit) in p.hits.iter().enumerate().skip(start).take(visible) {
        let it = &p.items[hit.item];
        let sel = i == p.selected;
        let row = if sel { th.selected() } else { Style::default() };
        let (glyph, gcolor) = match it.kind {
            PaletteKind::Bookmark => ("★", th.warn),
            PaletteKind::Recent => ("↻", th.text_dim),
            PaletteKind::Place => ("⌂", th.kinds.folder),
            PaletteKind::Disk => ("◉", th.kinds.data),
            PaletteKind::Parent => ("↑", th.text_dim),
            PaletteKind::Folder => ("▸", th.kinds.folder),
            PaletteKind::Typed => ("→", th.accent),
        };
        let tag = it.kind.tag();
        let tag_w = 9;
        let label_w = (iw / 3).clamp(14, 28);
        let detail_w = iw.saturating_sub(4 + label_w + 2 + tag_w + 1);
        let label = display::truncate(&it.label, label_w);
        let mut spans: Vec<Span> = vec![
            Span::styled(if sel { "▎" } else { " " }, th.accent_style().patch(row)),
            Span::styled(format!(" {glyph} "), Style::default().fg(gcolor).patch(row)),
        ];
        // The matched characters of the label are picked out in the accent colour.
        let mut used = 0;
        for (ci, c) in label.chars().enumerate() {
            let hl = hit.label_pos.contains(&ci);
            let st = if hl {
                th.accent_style().add_modifier(Modifier::BOLD)
            } else {
                th.base()
            };
            let st = if sel && !hl {
                st.add_modifier(Modifier::BOLD)
            } else {
                st
            };
            spans.push(Span::styled(c.to_string(), st.patch(row)));
            used += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        }
        spans.push(Span::styled(
            " ".repeat(label_w.saturating_sub(used) + 2),
            row,
        ));
        let detail = if it.detail.width() > detail_w {
            tail(&it.detail, detail_w)
        } else {
            it.detail.clone()
        };
        spans.push(Span::styled(pad(&detail, detail_w), th.dim().patch(row)));
        spans.push(Span::styled(
            pad_left(tag, tag_w + 1),
            th.faint().patch(row),
        ));
        let drawn: usize = spans.iter().map(|s| s.content.width()).sum();
        if drawn < iw {
            spans.push(Span::styled(" ".repeat(iw - drawn), row));
        }
        lines.push(Line::from(spans));
        hits.add(
            Rect {
                y: list.y + (i - start) as u16,
                height: 1,
                ..list
            },
            Target::PaletteRow(i),
        );
    }
    f.render_widget(Paragraph::new(lines), list);

    let foot = Line::from(vec![
        Span::styled("↑↓", th.key()),
        Span::styled(" move   ", th.dim()),
        Span::styled("Enter", th.key()),
        Span::styled(" go   ", th.dim()),
        Span::styled("Esc", th.key()),
        Span::styled(" close   ", th.dim()),
        Span::styled(format!("{} results", p.hits.len()), th.faint()),
    ]);
    f.render_widget(
        Paragraph::new(foot),
        Rect {
            y: inner.y + inner.height.saturating_sub(1),
            height: 1,
            ..inner
        },
    );
}

// -------------------------------------------------------------------------------- others

fn draw_input(f: &mut Frame, th: &Theme, iv: &InputView, area: Rect) {
    let (title, hint) = match &iv.kind {
        InputKind::Rename { .. } => ("Rename", "New name"),
        InputKind::NewDir => ("New folder", "Name"),
        InputKind::BulkRename { .. } => (
            "Bulk rename",
            "Pattern:  {name} {ext} {n} {n:3} {parent} {name:lower}   or   s/find/replace/",
        ),
    };
    let preview: Vec<(String, String)> = match &iv.kind {
        InputKind::BulkRename { items } => match vela_core::ops::Pattern::parse(&iv.text) {
            Ok(p) => p
                .preview(items)
                .into_iter()
                .take(12)
                .map(|pv| {
                    let from = pv.from.file_name().map(display::name).unwrap_or_default();
                    let to = match pv.to {
                        Ok(n) => display::name(&n),
                        Err(e) => format!("✖ {e}"),
                    };
                    (from, to)
                })
                .collect(),
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    };
    let h = 10 + preview.len() as u16;
    let r = centered(area, 88, h);
    let inner = frame(f, th, r, th.accent);
    let w = inner.width as usize;
    let before: String = iv.text.chars().take(iv.cursor).collect();
    let at: String = iv
        .text
        .chars()
        .nth(iv.cursor)
        .map(String::from)
        .unwrap_or_else(|| " ".to_string());
    let after: String = iv.text.chars().skip(iv.cursor + 1).collect();
    let mut lines = vec![
        Line::from(vec![chip(th, title, th.accent)]),
        Line::raw(""),
        Line::from(Span::styled(display::truncate(hint, w), th.dim())),
        Line::from(vec![
            Span::styled("› ", th.accent_style().add_modifier(Modifier::BOLD)),
            Span::styled(before, th.base()),
            Span::styled(at, Style::default().add_modifier(Modifier::REVERSED)),
            Span::styled(after, th.base()),
        ]),
    ];
    match &iv.error {
        Some(e) => lines.push(Line::from(Span::styled(format!("✖ {e}"), th.fg(th.error)))),
        None => {
            if let InputKind::BulkRename { .. } = &iv.kind {
                if let Err(e) = vela_core::ops::Pattern::parse(&iv.text) {
                    lines.push(Line::from(Span::styled(format!("✖ {e}"), th.fg(th.error))));
                } else {
                    lines.push(Line::raw(""));
                }
            } else {
                lines.push(Line::raw(""));
            }
        }
    }
    for (a, b) in preview {
        let half = w.saturating_sub(5) / 2;
        lines.push(Line::from(vec![
            Span::styled(display::truncate(&a, half), th.dim()),
            Span::styled("  →  ", th.accent_style()),
            Span::styled(display::truncate(&b, half), th.base()),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        button(th, "Enter", "Plan", Some(th.accent), true),
        Span::raw("  "),
        button(th, "Esc", "Cancel", None, true),
    ]));
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_history(
    f: &mut Frame,
    th: &Theme,
    h: &HistoryView,
    area: Rect,
    spinner: &str,
    hits: &mut Hits,
) {
    let rows = h.entries.len().clamp(3, 16) as u16;
    let r = centered(area, 104, rows + 9);
    let inner = frame(f, th, r, th.accent);
    let w = inner.width as usize;
    let mut lines: Vec<Line> = vec![
        Line::from(vec![chip(th, "History", th.accent)]),
        Line::raw(""),
    ];
    if h.loading {
        lines.push(Line::from(Span::styled(
            format!("{spinner} reading the journal…"),
            th.dim(),
        )));
    } else if h.entries.is_empty() {
        lines.push(Line::from(Span::styled("No operations yet.", th.dim())));
    }
    let visible = inner.height.saturating_sub(5) as usize;
    let start = h.selected.saturating_sub(visible.saturating_sub(1));
    for (i, e) in h.entries.iter().enumerate().skip(start).take(visible) {
        let sel = i == h.selected;
        let row = if sel { th.selected() } else { Style::default() };
        let (g, gc) = match e.status {
            EntryStatus::Finished(RunStatus::Completed) => ("✔", th.success),
            EntryStatus::Finished(RunStatus::CompletedWithProblems) => ("▲", th.warn),
            EntryStatus::Finished(RunStatus::Aborted)
            | EntryStatus::Finished(RunStatus::Cancelled) => ("■", th.warn),
            EntryStatus::Interrupted => ("?", th.error),
        };
        let tag = match (&e.undo_state, e.reversible) {
            (UndoState::Undone, _) => "undone".to_string(),
            (UndoState::Partial { remaining }, _) => format!("partly undone ({remaining} left)"),
            (_, false) => "permanent".to_string(),
            _ if e.undo_steps == 0 => "nothing to undo".to_string(),
            _ => "can undo".to_string(),
        };
        let tag_color = match tag.as_str() {
            "can undo" => th.accent,
            "permanent" => th.error,
            _ => th.muted,
        };
        let when = fmt::clock(e.time);
        let right = format!(" {tag}");
        let title_w = w.saturating_sub(4 + 13 + right.width());
        let mut spans = vec![
            Span::styled(if sel { "▎" } else { " " }, th.accent_style().patch(row)),
            Span::styled(format!("{g} "), Style::default().fg(gc).patch(row)),
            Span::styled(pad(&when, 13), th.dim().patch(row)),
            Span::styled(
                pad(&display::truncate(&e.title, title_w), title_w),
                th.base().patch(row),
            ),
            Span::styled(right, Style::default().fg(tag_color).patch(row)),
        ];
        let drawn: usize = spans.iter().map(|s| s.content.width()).sum();
        if drawn < w {
            spans.push(Span::styled(" ".repeat(w - drawn), row));
        }
        let row_y = inner.y + lines.len() as u16;
        hits.add(
            Rect {
                y: row_y,
                height: 1,
                ..inner
            },
            Target::HistoryRow(i),
        );
        lines.push(Line::from(spans));
    }
    while lines.len() < inner.height.saturating_sub(2) as usize {
        lines.push(Line::raw(""));
    }
    let btns = vec![
        button(th, "Enter", "Undo selected", Some(th.accent), true),
        Span::raw("  "),
        button(th, "Esc", "Close", None, true),
    ];
    hit_spans(
        hits,
        inner.x,
        inner.y + lines.len() as u16,
        &btns,
        &[
            (0, Target::Key(KeyCode::Enter)),
            (2, Target::Key(KeyCode::Esc)),
        ],
    );
    lines.push(Line::from(btns));
    f.render_widget(Paragraph::new(lines), inner);
}

/// Every action with its keys in both schemes side by side, grouped, scrollable.
fn draw_help(f: &mut Frame, th: &Theme, area: Rect, km: &Keymap, mouse: bool, scroll: usize) {
    use crate::keymap::Group;
    let w = 96.min(area.width.saturating_sub(4));
    let h = (area.height.saturating_sub(2)).min(46);
    let r = centered(area, w, h);
    let inner = frame(f, th, r, th.accent);
    let iw = inner.width as usize;
    let label_w = 30usize;
    let vim_w = ((iw.saturating_sub(label_w)) / 2).clamp(14, 30);

    let mut lines: Vec<Line> = Vec::new();
    let col = |on: bool, text: String| -> String {
        if !on {
            "—".to_string()
        } else if text.is_empty() {
            String::new()
        } else {
            text
        }
    };
    let vim_on = matches!(
        km.preset,
        crate::keymap::Preset::VimClassic | crate::keymap::Preset::Vim
    );
    let classic_on = matches!(
        km.preset,
        crate::keymap::Preset::VimClassic | crate::keymap::Preset::Classic
    );
    lines.push(Line::from(vec![
        Span::styled(pad("", label_w), th.dim()),
        Span::styled(
            pad("Vim", vim_w),
            th.accent_style().add_modifier(Modifier::BOLD),
        ),
        Span::styled("Classic", th.accent_style().add_modifier(Modifier::BOLD)),
    ]));
    for g in Group::ALL {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            g.title(),
            th.dim().add_modifier(Modifier::BOLD),
        )));
        for a in Action::ALL.iter().filter(|a| a.group() == g) {
            let vim = col(vim_on, km.keys_text(*a, Scheme::Vim));
            let classic = col(classic_on, km.keys_text(*a, Scheme::Classic));
            let style = |t: &str| {
                if t == "—" || t.is_empty() {
                    th.faint()
                } else {
                    th.key()
                }
            };
            lines.push(Line::from(vec![
                Span::styled(pad(a.label(), label_w), th.base()),
                Span::styled(pad(&display::truncate(&vim, vim_w - 1), vim_w), style(&vim)),
                Span::styled(
                    display::truncate(&classic, iw.saturating_sub(label_w + vim_w)),
                    style(&classic),
                ),
            ]));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Mouse",
        th.dim().add_modifier(Modifier::BOLD),
    )));
    let mouse_lines: Vec<&str> = if mouse {
        vec![
            "Click selects · double-click opens · wheel scrolls the list or the preview.",
            "Ctrl+click adds one item · Shift+click selects a range · right-click opens a menu.",
            "Click a folder in the path, a hint at the bottom, or a column title to sort.",
            "Shift+drag selects text in the terminal (the mouse belongs to vela while it runs).",
        ]
    } else {
        vec!["The mouse is off (mouse = false in the configuration)."]
    };
    for l in mouse_lines {
        lines.push(Line::from(Span::styled(l, th.base())));
    }
    lines.push(Line::raw(""));
    let quit = km
        .hint(Action::Quit)
        .unwrap_or_else(|| "Ctrl+Q".to_string());
    for l in [
        format!("Ctrl+C copies; it never quits. Quit with {quit}."),
        "Every operation shows a plan first. Every finished operation can be undone.".to_string(),
        "Rebind keys in the [keys] section of the configuration file.".to_string(),
    ] {
        lines.push(Line::from(Span::styled(l, th.dim())));
    }

    let rows = inner.height.saturating_sub(3) as usize;
    let max_scroll = lines.len().saturating_sub(rows);
    let scroll = scroll.min(max_scroll);
    let mut out: Vec<Line> = vec![
        Line::from(vec![
            chip(th, "Keys", th.accent),
            Span::styled(
                if max_scroll > 0 {
                    "   ↑↓ scroll · Esc close"
                } else {
                    "   Esc close"
                },
                th.faint(),
            ),
        ]),
        Line::raw(""),
    ];
    out.extend(lines.into_iter().skip(scroll).take(rows.saturating_sub(2)));
    f.render_widget(Paragraph::new(out), inner);
}

fn draw_menu(f: &mut Frame, th: &Theme, km: &Keymap, m: &MenuView, area: Rect, hits: &mut Hits) {
    let hint_of = |a: Action| km.hint(a).unwrap_or_default();
    let label_w = m
        .items
        .iter()
        .map(|i| i.action.label().width())
        .max()
        .unwrap_or(10);
    let hint_w = m
        .items
        .iter()
        .map(|i| hint_of(i.action).width())
        .max()
        .unwrap_or(0);
    let inner_w = label_w + 3 + hint_w;
    let w = (inner_w + 4) as u16;
    let seps = m.items.iter().filter(|i| i.gap_before).count();
    let h = (m.items.len() + seps + 2) as u16;
    let x = m.at.0.min(area.x + area.width.saturating_sub(w));
    let y = m.at.1.min(area.y + area.height.saturating_sub(h));
    let r = Rect {
        x,
        y,
        width: w.min(area.width),
        height: h.min(area.height),
    };
    f.render_widget(Clear, r);
    let blk = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(th.fg(th.accent));
    let inside = blk.inner(r);
    f.render_widget(blk, r);
    let mut lines: Vec<Line> = Vec::new();
    for (i, it) in m.items.iter().enumerate() {
        if it.gap_before && i > 0 {
            lines.push(Line::from(Span::styled(
                "─".repeat(inside.width as usize),
                th.faint(),
            )));
        }
        let sel = i == m.selected;
        let row = if sel { th.selected() } else { Style::default() };
        let text_style = if it.enabled { th.base() } else { th.faint() };
        let hint = hint_of(it.action);
        let gap = inner_w.saturating_sub(it.action.label().width() + hint.width());
        lines.push(Line::from(vec![
            Span::styled(" ", row),
            Span::styled(it.action.label().to_string(), text_style.patch(row)),
            Span::styled(" ".repeat(gap + 1), row),
            Span::styled(
                hint,
                if it.enabled { th.dim() } else { th.faint() }.patch(row),
            ),
            Span::styled(" ", row),
        ]));
        let ly = inside.y + (lines.len() - 1) as u16;
        if ly < inside.y + inside.height {
            hits.add(
                Rect {
                    y: ly,
                    height: 1,
                    ..inside
                },
                Target::MenuItem(i),
            );
        }
    }
    f.render_widget(Paragraph::new(lines), inside);
}
