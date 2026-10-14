//! The window of a plan: what an operation will do, shown before anything is touched.
//!
//! A card with a round mark and a title that says what is asked ("Eliminare definitivamente 3
//! elementi?", "Copia 3 elementi in Documenti"), a quiet line under it with the size and where
//! from and to, then what will happen to each item (with a small label where the item meets a
//! file of the same name), the warnings as strips of their own colour, how clashes are settled
//! as a group of pills, a note on whether it can be undone, and the buttons.
//!
//! Permanent deletion keeps its typed confirmation: the button stays disabled until the word is
//! typed. Nothing in the picture of the plan changes that.

use rada_core::display;
use rada_core::fs::FileKind;
use rada_core::i18n::{Key, tf};
use rada_core::ops::{ConflictPolicy, ItemAction, OpKind, Severity, Step};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::popup::{self, Btn, Ctx, Tone};
use super::widgets::{hit_groups, key_name, pad, pad_left, tail};
use crate::app::{PlanView, Replan};
use crate::fmt;
use crate::hits::{Hits, Target};
use crate::icons::{self, Glyph};
use crate::keymap::{Action, Keymap};
use crate::theme::Theme;

/// The order the pills of the conflict control are in, left to right.
pub const POLICY_ORDER: [ConflictPolicy; 3] = [
    ConflictPolicy::KeepBoth,
    ConflictPolicy::Overwrite,
    ConflictPolicy::Skip,
];

/// The policy `dir` steps (1 or -1) from `p` in the order of the pills; it does not wrap.
pub fn policy_step(p: ConflictPolicy, dir: isize) -> ConflictPolicy {
    let at = POLICY_ORDER.iter().position(|x| *x == p).unwrap_or(0) as isize;
    let to = (at + dir).clamp(0, POLICY_ORDER.len() as isize - 1);
    POLICY_ORDER[to as usize]
}

/// Items listed before "… and N more".
const LISTED: usize = 6;

/// The mark in the round head of the window.
fn mark_of(ctx: &Ctx, kind: OpKind, redo: bool) -> &'static str {
    if redo {
        return icons::action_icon(ctx.set, Action::Redo);
    }
    let a = match kind {
        OpKind::Copy => Action::Copy,
        OpKind::Move => Action::Cut,
        OpKind::Rename | OpKind::BulkRename => Action::Rename,
        OpKind::MakeDir | OpKind::MakeFile => Action::NewFolder,
        OpKind::Trash | OpKind::Delete => Action::Trash,
        OpKind::Undo => Action::Undo,
        OpKind::Extract => Action::ExtractHere,
        OpKind::Compress => Action::Compress,
        OpKind::SetTags => return icons::ui(ctx.set, Glyph::Tag),
    };
    icons::action_icon(ctx.set, a)
}

/// "Copy 5 items to /home/x/Documents": the folder by its name, as the head of the window and
/// the card of an operation that runs say it.
pub fn shorten_title(title: &str, home: &std::path::Path) -> String {
    let (_, path) = split_title(title);
    match path {
        Some(p) => {
            let folder = std::path::Path::new(&p)
                .file_name()
                .map(display::name)
                .unwrap_or_else(|| fmt::short_path(std::path::Path::new(&p), home));
            title.replacen(&p, &folder, 1)
        }
        None => title.to_string(),
    }
}

/// The question or the command at the head of the window.
fn title_of(plan: &rada_core::ops::Plan, home: &std::path::Path) -> String {
    let items = || {
        fmt::count(
            plan.items.len().max(plan.totals.items as usize).max(1) as u64,
            Key::ItemOne,
            Key::ItemMany,
        )
    };
    // An undo, or a redo, asks what it takes back or does again.
    if let Some(u) = &plan.undoes {
        return tf(Key::UndoQ, &[&op_phrase(u.kind, u.items)]);
    }
    if plan.redo_of.is_some() {
        return tf(Key::RedoQ, &[&op_phrase(plan.kind, items_of(plan))]);
    }
    match plan.kind {
        OpKind::Delete => tf(Key::PlanQDelete, &[&items()]),
        OpKind::Trash => tf(Key::PlanQTrash, &[&items()]),
        _ => shorten_title(&plan.title, home),
    }
}

/// How many items a plan is about.
fn items_of(plan: &rada_core::ops::Plan) -> u64 {
    plan.items.len().max(plan.totals.items as usize).max(1) as u64
}

/// "the copy" / "la copia": what an operation is called, without a count.
fn op_noun(kind: OpKind) -> Key {
    match kind {
        OpKind::Copy => Key::OpNounCopy,
        OpKind::Move => Key::OpNounMove,
        OpKind::Trash => Key::OpNounTrash,
        OpKind::Rename | OpKind::BulkRename => Key::OpNounRename,
        OpKind::Extract => Key::OpNounExtract,
        OpKind::Compress => Key::OpNounCompress,
        OpKind::MakeDir => Key::OpNounMkdir,
        OpKind::MakeFile => Key::OpNounMkfile,
        OpKind::SetTags => Key::OpNounTags,
        OpKind::Delete | OpKind::Undo => Key::OpNounDelete,
    }
}

/// "the copy of 3 items" / "la copia di 3 elementi" (a new folder has no count).
fn op_phrase(kind: OpKind, items: u64) -> String {
    let noun = op_noun(kind).tr();
    match kind {
        OpKind::MakeDir | OpKind::MakeFile => noun.to_string(),
        _ => tf(
            Key::OpOfItems,
            &[&noun, &fmt::count(items, Key::ItemOne, Key::ItemMany)],
        ),
    }
}

/// The quiet line under the title: the size, and where from and to.
fn summary_of(plan: &rada_core::ops::Plan, bases: &Bases, home: &std::path::Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    if plan.totals.bytes > 0 && !matches!(plan.kind, OpKind::Undo) {
        parts.push(fmt::size(plan.totals.bytes));
    }
    let name = |p: &std::path::Path| fmt::short_path(p, home);
    match (&bases.src, &bases.dst) {
        (Some(s), Some(d)) => parts.push(format!("{} → {}", name(s), name(d))),
        (Some(s), None) => parts.push(tf(Key::BaseIn, &[]).to_string() + " " + &name(s)),
        (None, Some(d)) => parts.push(tf(Key::BaseIn, &[]).to_string() + " " + &name(d)),
        (None, None) => {}
    }
    parts.join("  ·  ")
}

/// What the run button says.
fn run_label(plan: &rada_core::ops::Plan) -> String {
    if let Some(u) = &plan.undoes {
        return tf(Key::BtnUndoOp, &[&op_noun(u.kind).tr()]);
    }
    if plan.redo_of.is_some() {
        return tf(Key::BtnRedoOp, &[&op_noun(plan.kind).tr()]);
    }
    run_verb(plan.kind).to_string()
}

fn run_verb(kind: OpKind) -> &'static str {
    match kind {
        OpKind::Delete => Key::BtnDelete.tr(),
        OpKind::Trash => Action::Trash.label(),
        OpKind::Copy => Key::VCopy.tr(),
        OpKind::Move => Key::VMove.tr(),
        OpKind::Extract => Key::VExtract.tr(),
        OpKind::Compress => Key::VCompress.tr(),
        OpKind::Rename | OpKind::BulkRename => Key::VRename.tr(),
        _ => Key::BtnRun.tr(),
    }
}

/// What the note under the window says about taking it back, if it can be.
fn undo_note(kind: OpKind) -> Option<Key> {
    Some(match kind {
        OpKind::Copy => Key::NoteUndoCopy,
        OpKind::Move => Key::NoteUndoMove,
        OpKind::Trash => Key::NoteUndoTrash,
        OpKind::Extract => Key::NoteUndoExtract,
        OpKind::Compress => Key::NoteUndoCompress,
        OpKind::Rename | OpKind::BulkRename => Key::NoteUndoRename,
        OpKind::SetTags => Key::NoteUndoTags,
        OpKind::MakeDir | OpKind::MakeFile => Key::NoteUndoNew,
        OpKind::Delete | OpKind::Undo => return None,
    })
}

/// Whether an item meets something of the same name where it is going.
fn exists_already(a: ItemAction) -> bool {
    matches!(
        a,
        ItemAction::Skip | ItemAction::Overwrite | ItemAction::KeepBoth | ItemAction::Merge
    )
}

pub fn draw(
    f: &mut Frame,
    ctx: &Ctx,
    pv: &PlanView,
    home: &std::path::Path,
    km: &Keymap,
    area: Rect,
    hits: &mut Hits,
) {
    let th = ctx.th;
    let plan = &pv.plan;
    let blocked = !plan.is_executable() && plan.blocking().next().is_some();
    let delete = plan.kind == OpKind::Delete;
    let typed = pv.needs_typed_confirmation();
    let tone = if blocked || typed {
        Tone::Danger
    } else if plan.kind == OpKind::Trash {
        Tone::Warn
    } else {
        Tone::Accent
    };
    let w = (if pv.details { 104 } else { 76 }).min(area.width.saturating_sub(4));
    let iw = (w as usize).saturating_sub(6);
    let bases = Bases::of(plan, home);

    let mut body: Vec<Line> = Vec::new();
    let mut seg_line: Option<usize> = None;
    let mut seg_hits: Vec<(usize, usize, Target)> = Vec::new();
    let mut seg_spans: Vec<Span> = Vec::new();

    // The head: the round mark, what is asked, and the line under it.
    body.extend(popup::headline(
        ctx,
        mark_of(ctx, plan.kind, plan.redo_of.is_some()),
        tone,
        &display::truncate(&title_of(plan, home), iw.saturating_sub(6)),
        &summary_of(plan, &bases, home),
        iw,
    ));
    body.push(Line::from(popup::rule(ctx, iw)));

    // What will happen.
    if !plan.renames.is_empty() {
        for (from, to) in plan.renames.iter().take(LISTED) {
            let a = from.file_name().map(display::name).unwrap_or_default();
            let b = to.file_name().map(display::name).unwrap_or_default();
            let half = iw.saturating_sub(6) / 2;
            body.push(Line::from(vec![
                Span::styled(" ", ctx.surface()),
                Span::styled(display::truncate(&a, half), ctx.dim()),
                Span::styled("  →  ", ctx.accent()),
                Span::styled(display::truncate(&b, half), ctx.base()),
            ]));
        }
        if plan.renames.len() > LISTED {
            body.push(Line::from(Span::styled(
                format!(" {}", tf(Key::AndNMore, &[&(plan.renames.len() - LISTED)])),
                ctx.faint(),
            )));
        }
    } else if !pv.details && !plan.items.is_empty() {
        let right_w = 10usize;
        let show_badge = matches!(
            plan.kind,
            OpKind::Copy | OpKind::Move | OpKind::Extract | OpKind::Compress | OpKind::Undo
        );
        let badge_w = if show_badge {
            [
                Key::BadgeNew,
                Key::BadgeExists,
                Key::BadgeRemoved,
                Key::BadgeBack,
                Key::BadgeTagsBack,
            ]
            .iter()
            .map(|k| k.tr().width())
            .max()
            .unwrap_or(0)
                + 4
        } else {
            0
        };
        let name_w = iw.saturating_sub(4 + right_w + badge_w);
        for it in plan.items.iter().take(LISTED) {
            let is_dir = it.kind == FileKind::Dir;
            let name = it
                .path
                .file_name()
                .map(display::name)
                .unwrap_or_else(|| display::path(&it.path));
            let name = if is_dir { format!("{name}/") } else { name };
            let (glyph, gcolor) = icons::icon_for(
                &it.path.file_name().map(display::name).unwrap_or_default(),
                it.kind,
                ctx.set,
                th,
            );
            let mut spans = vec![
                Span::styled(" ", ctx.surface()),
                Span::styled(
                    format!("{} ", if glyph.is_empty() { " " } else { glyph }),
                    Style::default().fg(gcolor).bg(th.popup),
                ),
                Span::styled(
                    pad(&display::truncate(&name, name_w), name_w),
                    if is_dir {
                        th.fg(th.kinds.folder).bg(th.popup)
                    } else {
                        ctx.base()
                    },
                ),
            ];
            if show_badge {
                let b = if plan.kind == OpKind::Undo {
                    match it.action {
                        ItemAction::Delete | ItemAction::Trash => {
                            popup::badge(ctx, Key::BadgeRemoved.tr(), Tone::Danger)
                        }
                        ItemAction::Retag => popup::badge(ctx, Key::BadgeTagsBack.tr(), Tone::Info),
                        _ => popup::badge(ctx, Key::BadgeBack.tr(), Tone::Info),
                    }
                } else if exists_already(it.action) {
                    popup::badge(ctx, Key::BadgeExists.tr(), Tone::Warn)
                } else {
                    popup::badge(ctx, Key::BadgeNew.tr(), Tone::Ok)
                };
                let bw: usize = b.iter().map(|s| s.content.width()).sum();
                spans.push(Span::styled(
                    " ".repeat(badge_w.saturating_sub(bw)),
                    ctx.surface(),
                ));
                spans.extend(b);
            }
            let size = if it.bytes > 0 {
                fmt::size(it.bytes)
            } else {
                String::new()
            };
            spans.push(Span::styled(pad_left(&size, right_w), ctx.dim()));
            body.push(Line::from(spans));
        }
        if plan.items.len() > LISTED {
            body.push(Line::from(Span::styled(
                format!("   {}", tf(Key::AndNMore, &[&(plan.items.len() - LISTED)])),
                ctx.faint(),
            )));
        }
    } else if !plan.steps.is_empty() {
        let n = plan.steps.len() as u64;
        body.push(Line::from(vec![Span::styled(
            tf(
                Key::PlanWhatN,
                &[&fmt::count(n, Key::StepOne, Key::StepMany)],
            ),
            ctx.dim().add_modifier(Modifier::BOLD),
        )]));
        let mut shown = 0;
        for s in &plan.steps {
            let Some(label) = step_label(s, &bases) else {
                continue;
            };
            if shown == 500 {
                body.push(Line::from(Span::styled(
                    Key::MoreStepsNotListed.tr(),
                    ctx.faint(),
                )));
                break;
            }
            body.push(Line::from(Span::styled(tail(&label, iw), ctx.dim())));
            shown += 1;
        }
    }
    body.push(Line::raw(""));

    // Warnings: a strip of the colour of each, with a few examples under it.
    for wn in plan
        .warnings
        .iter()
        .filter(|w| !(delete && w.kind == rada_core::ops::WarningKind::Irreversible))
    {
        let (mark, t) = match wn.severity {
            Severity::Blocking => ("✖", Tone::Danger),
            Severity::Warning => ("▲", Tone::Warn),
            Severity::Info => ("●", Tone::Info),
        };
        let mark = if ctx.set == icons::IconSet::None {
            "!"
        } else {
            mark
        };
        for (i, l) in fmt::wrap(&wn.message, iw.saturating_sub(6))
            .into_iter()
            .enumerate()
        {
            body.push(Line::from(popup::strip(
                ctx,
                t,
                if i == 0 { mark } else { " " },
                &l,
                iw,
            )));
        }
        for ex in wn.examples.iter().take(3) {
            body.push(Line::from(Span::styled(
                format!("    {}", tail(&bases.rel(ex), iw.saturating_sub(4))),
                ctx.faint(),
            )));
        }
    }
    // Nothing goes to the trash: said once, in the colour of what cannot be taken back.
    if delete {
        body.push(Line::from(popup::strip(
            ctx,
            Tone::Danger,
            if ctx.set == icons::IconSet::None {
                "!"
            } else {
                "▲"
            },
            Key::CannotBeUndone.tr(),
            iw,
        )));
    }
    if !plan.warnings.is_empty() || delete || typed {
        body.push(Line::raw(""));
    }

    // How name clashes are settled: a group of pills, the one in use filled.
    if !matches!(pv.replan, Replan::None) {
        seg_line = Some(body.len());
        let mut seg: Vec<Span> = vec![Span::styled(
            format!("{}   ", Key::PolicyAsk.tr()),
            ctx.dim(),
        )];
        for p in POLICY_ORDER {
            let label = match p {
                ConflictPolicy::Skip => Key::PolSkip,
                ConflictPolicy::KeepBoth => Key::PolKeep,
                ConflictPolicy::Overwrite => Key::PolOver,
            }
            .tr();
            let label = capitalised(label);
            let pill = if plan.policy == p {
                popup::pill(
                    ctx,
                    &label,
                    th.accent,
                    Style::default()
                        .fg(th.on_accent)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                popup::pill(ctx, &label, th.button, Style::default().fg(th.text))
            };
            let start = seg.len();
            seg.extend(pill);
            seg_hits.push((start, seg.len(), Target::Policy(p)));
            seg.push(Span::styled(" ", ctx.surface()));
        }
        seg.push(Span::styled(
            format!("  {}", Key::PolicyChange.tr()),
            ctx.faint(),
        ));
        seg_spans = seg.clone();
        body.push(Line::from(seg));
        if plan.policy == ConflictPolicy::Overwrite {
            body.push(Line::from(Span::styled(
                Key::PolicyTrashNote.tr(),
                ctx.faint(),
            )));
        }
        body.push(Line::raw(""));
    }

    // Whether it can be taken back.
    if !blocked
        && !delete
        && let Some(key) = undo_note(plan.kind)
        && plan.reversible
    {
        let undo_key = km.hint(Action::Undo).unwrap_or_default();
        body.push(Line::from(popup::strip(
            ctx,
            Tone::Info,
            if ctx.set == icons::IconSet::None {
                "i"
            } else {
                "●"
            },
            &tf(key, &[&undo_key]),
            iw,
        )));
        body.push(Line::raw(""));
    }

    // Footer: the typed confirmation, then the keys and the buttons.
    let mut foot: Vec<Line> = Vec::new();
    if typed {
        let field_fill = th.key_bg;
        let mut spans = vec![
            Span::styled(Key::TypePrefix.tr().to_string(), ctx.dim()),
            Span::styled(
                Key::ConfirmWord.tr().to_string(),
                ctx.accent().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                Key::ToConfirm.tr().trim_end_matches([':', ' ']).to_string(),
                ctx.dim(),
            ),
            Span::styled("  ", ctx.surface()),
        ];
        let text = format!("{}▏", pv.typed);
        spans.extend(popup::pill(
            ctx,
            &pad(&text, 12),
            field_fill,
            Style::default().fg(th.text),
        ));
        foot.push(Line::from(spans));
        foot.push(Line::raw(""));
    }
    let can_run = pv.can_run();
    // Left: the keys that are not buttons. Right: the buttons.
    let mut left: Vec<Span> = Vec::new();
    if pv.replanning.is_some() {
        left.push(Span::styled(
            Key::Replanning.tr().to_string(),
            th.fg(th.warn).bg(th.popup),
        ));
    } else if blocked {
        left.push(Span::styled(
            Key::BlockedNothing.tr().to_string(),
            th.fg(th.error).bg(th.popup),
        ));
    } else {
        let mut parts = vec![Key::HintTabButtons.tr().to_string()];
        if !plan.items.is_empty() && plan.steps.len() > plan.items.len() && !typed {
            parts.push(
                if pv.details {
                    Key::PlanTabSummary
                } else {
                    Key::PlanTabSteps
                }
                .tr()
                .to_string(),
            );
        }
        if !typed {
            parts.push(Key::ArrowsScroll.tr().to_string());
        }
        left.push(Span::styled(parts.join("  ·  "), ctx.faint()));
    }
    let mut buttons: Vec<Span> = Vec::new();
    let mut button_hits: Vec<(usize, usize, Target)> = Vec::new();
    let cancel = popup::button(
        ctx,
        Key::BtnCancel.tr(),
        Some(key_name("Esc")),
        Btn::Quiet,
        pv.cancel_focus,
    );
    buttons.extend(cancel);
    button_hits.push((0, buttons.len(), Target::PlanCancel));
    if !blocked {
        buttons.push(Span::styled("  ", ctx.surface()));
        let kind = if can_run {
            Btn::Primary(if delete { Tone::Danger } else { Tone::Accent })
        } else {
            Btn::Disabled
        };
        let at = buttons.len();
        buttons.extend(popup::button(
            ctx,
            &run_label(plan),
            Some(key_name("Enter")),
            kind,
            !pv.cancel_focus && can_run,
        ));
        if can_run {
            button_hits.push((at, buttons.len(), Target::PlanRun));
        }
    }
    let left_w: usize = left.iter().map(|s| s.content.width()).sum();
    let btn_w: usize = buttons.iter().map(|s| s.content.width()).sum();
    let gap = iw.saturating_sub(left_w + btn_w);
    let mut foot_spans = Vec::new();
    let mut hit_index: Vec<(usize, usize, Target)> = Vec::new();
    if gap >= 2 {
        foot_spans.extend(left);
        foot_spans.push(Span::styled(" ".repeat(gap), ctx.surface()));
    } else {
        foot_spans.push(Span::styled(
            " ".repeat(iw.saturating_sub(btn_w)),
            ctx.surface(),
        ));
    }
    let offset = foot_spans.len();
    for (a, b, t) in button_hits {
        hit_index.push((offset + a, offset + b, t));
    }
    foot_spans.extend(buttons);
    foot.push(Line::from(foot_spans.clone()));

    // The card: as tall as it needs, within the screen.
    let foot_h = foot.len() as u16;
    let want = body.len() as u16 + foot_h + 4;
    let h = want.clamp(12, area.height.saturating_sub(2));
    let r = super::widgets::centered(area, w, h);
    let inner_card = popup::card(f.buffer_mut(), r, th);
    let inner = Rect {
        x: inner_card.x + 2,
        y: inner_card.y + 1,
        width: inner_card.width.saturating_sub(4),
        height: inner_card.height.saturating_sub(2),
    };
    let content_h = inner.height.saturating_sub(foot_h) as usize;
    let max_scroll = body.len().saturating_sub(content_h);
    let scroll = pv.scroll.min(max_scroll);
    let shown: Vec<Line> = body.into_iter().skip(scroll).take(content_h).collect();
    let buf = f.buffer_mut();
    for (i, line) in shown.into_iter().enumerate() {
        popup::put(
            buf,
            Rect {
                y: inner.y + i as u16,
                height: 1,
                ..inner
            },
            line,
        );
    }
    let foot_y = inner.y + content_h as u16;
    for (i, line) in foot.into_iter().enumerate() {
        popup::put(
            buf,
            Rect {
                y: foot_y + i as u16,
                height: 1,
                ..inner
            },
            line,
        );
    }
    // Clickable: the buttons (last line of the footer) and the pills of the conflict control.
    hit_groups(hits, inner.x, foot_y + foot_h - 1, &foot_spans, &hit_index);
    if let Some(line) = seg_line
        && line >= scroll
        && line < scroll + content_h
    {
        hit_groups(
            hits,
            inner.x,
            inner.y + (line - scroll) as u16,
            &seg_spans,
            &seg_hits,
        );
    }
    let _ = Color::Reset;
    let _ = Theme::NAMES;
}

/// The first letter in capitals, for a label that stands alone ("tieni entrambi" → "Tieni
/// entrambi").
fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[allow(dead_code)]
fn verb(kind: OpKind) -> &'static str {
    match kind {
        OpKind::Copy => Key::VCopy,
        OpKind::Move => Key::VMove,
        OpKind::Rename | OpKind::BulkRename => Key::VRename,
        OpKind::MakeDir => Key::VNewFolder,
        OpKind::MakeFile => Key::VNewFile,
        OpKind::Trash => Key::VTrash,
        OpKind::Delete => Key::VDelete,
        OpKind::Undo => Key::VUndo,
        OpKind::Extract => Key::VExtract,
        OpKind::Compress => Key::VCompress,
        OpKind::SetTags => Key::VTags,
    }
    .tr()
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
    fn of(plan: &rada_core::ops::Plan, home: &std::path::Path) -> Bases {
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
            OpKind::Copy | OpKind::Move | OpKind::Extract => {
                (from_items.or_else(from_steps), plan.destination.clone())
            }
            // The destination is the new archive; its steps live in the folder it is in.
            OpKind::Compress => (
                from_items.or_else(from_steps),
                plan.destination
                    .as_deref()
                    .and_then(|d| d.parent())
                    .map(std::path::Path::to_path_buf),
            ),
            OpKind::MakeDir | OpKind::MakeFile => (None, plan.destination.clone()),
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

/// "Copy 5 items to /home/x/backup" -> ("Copy 5 items", Some("/home/x/backup")). The path may
/// begin with `/`, `~`, a drive (`D:\`) or `\\` (a network share).
fn split_title(title: &str) -> (String, Option<String>) {
    let looks_like_path = |rest: &str| {
        let b = rest.as_bytes();
        rest.starts_with(['/', '~', '\\'])
            || (b.len() > 2
                && b[0].is_ascii_alphabetic()
                && b[1] == b':'
                && matches!(b[2], b'\\' | b'/'))
    };
    let mut best: Option<usize> = None;
    for marker in [" to ", " in "] {
        for (i, _) in title.rmatch_indices(marker) {
            if looks_like_path(&title[i + marker.len()..]) {
                best = Some(best.map_or(i, |b| b.max(i)));
                break;
            }
        }
    }
    match best {
        // " to " and " in " are both four bytes long.
        Some(i) => (title[..i].to_string(), Some(title[i + 4..].to_string())),
        None => (title.to_string(), None),
    }
}

/// A word of the first column of the list of steps, padded to nine cells (and kept apart
/// from what follows when it is longer).
fn step_word(w: Key) -> String {
    let w = w.tr();
    if w.width() >= 9 {
        format!("{w} ")
    } else {
        pad(w, 9)
    }
}

/// One line per step, paths relative to the plan's bases.
fn step_label(step: &Step, b: &Bases) -> Option<String> {
    Some(match step {
        Step::MakeDir { path, .. } => format!("{}{}", step_word(Key::StepWFolder), b.rel_dst(path)),
        Step::MakeFile { path, .. } => format!("{}{}", step_word(Key::StepWFile), b.rel_dst(path)),
        Step::FinishDir { .. } => return None,
        Step::CopyFile {
            dst, remove_source, ..
        } => format!(
            "{}{}",
            step_word(if *remove_source {
                Key::StepWMove
            } else {
                Key::StepWCopy
            }),
            b.rel_dst(dst)
        ),
        Step::CopySymlink {
            dst,
            target,
            remove_source,
            ..
        } => format!(
            "{}{} → {}",
            pad(
                &format!(
                    "{}{}",
                    Key::StepWLink.tr(),
                    if *remove_source { "↪" } else { "" }
                ),
                9
            ),
            b.rel_dst(dst),
            display::path(target)
        ),
        Step::HardLink {
            existing,
            link,
            src_link,
            ..
        } => format!(
            "{}{} = {}",
            pad(
                &if src_link.is_some() {
                    format!("{}⇄", Key::StepWLink.tr())
                } else {
                    Key::StepWHardLink.tr().to_string()
                },
                9
            ),
            b.rel_dst(link),
            b.rel_dst(existing)
        ),
        Step::ExtractFile { dst, .. } => {
            format!("{}{}", step_word(Key::StepWExtract), b.rel_dst(dst))
        }
        Step::ExtractSymlink { dst, target, .. } => format!(
            "{}{} → {}",
            step_word(Key::StepWLink),
            b.rel_dst(dst),
            display::path(target)
        ),
        Step::Compress { dst, items, .. } => format!(
            "{}{} ({})",
            step_word(Key::StepWArchive),
            b.rel_dst(dst),
            fmt::count(items.len() as u64, Key::ItemOne, Key::ItemMany)
        ),
        Step::Rename { from, to } => format!(
            "{}{} → {}",
            step_word(Key::StepWRename),
            b.rel_src(from),
            b.rel(to)
        ),
        Step::TrashItem { path } => format!("{}{}", step_word(Key::StepWTrash), b.rel_src(path)),
        Step::RemoveFile { path, .. } => {
            format!("{}{}", step_word(Key::StepWDelete), b.rel_src(path))
        }
        Step::RemoveDir { path } => format!("{}{}/", step_word(Key::StepWRemove), b.rel_src(path)),
        Step::Restore { item } => format!(
            "{}{}",
            step_word(Key::StepWRestore),
            b.rel_src(&item.original)
        ),
        Step::SetTags {
            path,
            before,
            after,
        } => format!(
            "{}{}: {} → {}",
            step_word(Key::StepWTags),
            b.rel_src(path),
            rada_core::ops::tag_names(before),
            rada_core::ops::tag_names(after)
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_is_split_before_a_path_of_any_system() {
        for path in [
            "/home/x/backup",
            "~/backup",
            r"D:\a\b\backup",
            "C:/Users/x/backup",
            r"\\server\share\backup",
        ] {
            let t = format!("Copy 5 items to {path}");
            assert_eq!(
                split_title(&t),
                ("Copy 5 items".to_string(), Some(path.to_string())),
                "{t}"
            );
        }
        // No path after the word: nothing to shorten.
        assert_eq!(
            split_title("Copy to the end"),
            ("Copy to the end".to_string(), None)
        );
        assert_eq!(
            split_title("Rename 3 items in the list"),
            ("Rename 3 items in the list".to_string(), None)
        );
    }

    #[test]
    fn the_head_of_a_plan_names_the_folder_of_a_windows_path() {
        let t = shorten_title(
            r"Copy 1 item to D:\a\rada-fm\target\proc-500\elsewhere.d",
            std::path::Path::new(r"C:\Users\x"),
        );
        // A backslash separates folders only on Windows.
        if cfg!(windows) {
            assert_eq!(t, "Copy 1 item to elsewhere.d");
        } else {
            assert!(t.starts_with("Copy 1 item to "), "{t}");
        }
    }
}
