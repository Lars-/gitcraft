//! The diff of one file, with the colours of its code and the words that changed within its
//! lines, and staging and discarding of hunks and single lines; or a picture's old and new version.

use std::ops::Range;

use eframe::egui::cache::{ComputerMut, FrameCache};
use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Align2, Color32, FontId, Key, Rect, RichText, Sense, Ui, Vec2};
use gitcraft_core::diff::{self, LineKind, Source};
use gitcraft_core::git::Op;

use crate::app::{Action, Confirm, OpenFile, Picture, Versions};
use crate::commands::{self, Command};
use crate::{details, theme};

const LINE: f32 = 18.0;
const NUMBER: f32 = 44.0;
const FONT: FontId = FontId::monospace(12.5);

/// What can be done with the whole file, or with `lines` of it: git stages no lines of a file it
/// does not track.
fn verbs(source: &Source, lines: bool) -> &'static [&'static str] {
    match source {
        Source::Unstaged(_) => &["Stage", "Discard"],
        Source::Untracked(_) if !lines => &["Stage", "Discard"],
        Source::Staged(_) => &["Unstage"],
        Source::Untracked(_) | Source::Commit { .. } => &[],
    }
}

/// What the keyboard and the palette do with the file: stage, unstage or discard the picked lines,
/// or the whole file while none are picked.
pub fn commands(file: &OpenFile) -> Vec<Command> {
    let picked = |h, l| file.lines.contains(&(h, l));
    let lines = theme::count(file.lines.len(), "line");
    let verbs = verbs(&file.source, !file.lines.is_empty());
    let commands = verbs.iter().map(|&verb| {
        let (what, action) = match verb {
            _ if !file.lines.is_empty() => (lines.as_str(), act(file, verb, &lines, &picked)),
            "Stage" => ("file", Action::Run(Op::Stage(vec![file.source.path().into()]))),
            "Unstage" => ("file", Action::Run(Op::Unstage(vec![file.source.path().into()]))),
            _ => ("file", Action::Confirm(details::discard(&file.source))),
        };
        // Discarding asks first.
        let name = format!("{verb} {what}{}", if verb == "Discard" { "…" } else { "" });
        Command::new(name, commands::staging(verb), Some(action))
    });
    commands.collect()
}

/// Doing `verb` with the `selected` lines, which `what` names.
fn act(file: &OpenFile, verb: &'static str, what: &str, selected: &dyn Fn(usize, usize) -> bool) -> Action {
    // Unstaging and discarding both start from the lines as they are now, and take them out.
    let reverse = verb != "Stage";
    let patch = file.diff.patch(selected, reverse);
    if verb == "Discard" {
        let question = format!("Discard {what} of {}?", file.source.path());
        return Action::Confirm(Confirm { question, verb, op: Op::DiscardLines(patch) });
    }
    Action::Run(Op::ApplyToIndex { patch, reverse })
}

/// Finds the words that changed between a removed and an added line, once while they are on screen.
#[derive(Debug, Default)]
struct Words;

impl ComputerMut<[&str; 2], [Vec<Range<usize>>; 2]> for Words {
    fn compute(&mut self, [old, new]: [&str; 2]) -> [Vec<Range<usize>>; 2] {
        diff::words(old, new)
    }
}

/// A line's text after its `marker`, in its `colours` (where each starts, `None` for `plain`), and
/// with its `words` that changed on the `background`.
fn line_text(
    marker: &str,
    text: &str,
    colours: &[(usize, Option<Color32>)],
    (words, background): (&[Range<usize>], Color32),
    font: &FontId,
    plain: Color32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut add = |text: &str, color, background| {
        job.append(text, 0.0, TextFormat { font_id: font.clone(), color, background, ..TextFormat::default() });
    };
    add(marker, plain, Color32::TRANSPARENT);
    // A piece goes up to where the next colour or word starts or a word ends; a word can end in
    // the carriage return that is not shown.
    let starts = words.iter().flat_map(|word| [word.start, word.end]).chain(colours.iter().map(|&(at, _)| at));
    let mut cuts: Vec<usize> = starts.filter(|&at| at < text.len()).chain([0, text.len()]).collect();
    cuts.sort_unstable();
    cuts.dedup();
    let (mut colour, mut next) = (plain, colours.iter().peekable());
    for piece in cuts.windows(2) {
        while let Some(&(_, start)) = next.next_if(|&&(at, _)| at <= piece[0]) {
            colour = start.unwrap_or(plain);
        }
        let changed = words.iter().any(|word| word.contains(&piece[0]));
        add(&text[piece[0]..piece[1]], colour, if changed { background } else { Color32::TRANSPARENT });
    }
    job
}

/// Where egui keeps whether diffs show the old and the new version side by side: that is how the
/// view looks, like a folded section, so it stays with the view and lasts between runs.
const SIDE_BY_SIDE: &str = "side by side";

/// Shows the open file, with a way to close it when it fills the centre `alone`.
pub fn show(ui: &mut Ui, file: &OpenFile, alone: bool, actions: &mut Vec<Action>) {
    let verbs = verbs(&file.source, true);
    let picked = |h, l| file.lines.contains(&(h, l));
    let lines = theme::count(file.lines.len(), "line");
    let id = egui::Id::unique(SIDE_BY_SIDE);
    let mut side_by_side = ui.data_mut(|d| d.get_persisted(id)).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.strong(file.source.path());
        ui.weak(match file.source {
            Source::Unstaged(_) => "unstaged",
            Source::Staged(_) => "staged",
            Source::Untracked(_) => "untracked",
            Source::Commit { .. } => "",
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if alone && (ui.button("Close").clicked() || ui.input(|i| i.key_pressed(Key::Escape))) {
                actions.push(Action::CloseFile);
            }
            if ui.add(egui::Button::selectable(side_by_side, "Side by side")).clicked() {
                side_by_side = !side_by_side;
                ui.data_mut(|d| d.insert_persisted(id, side_by_side));
            }
            for verb in verbs {
                let keys = ui.ctx().format_shortcut(&commands::staging(verb));
                if !file.lines.is_empty() && ui.button(format!("{verb} {lines}")).on_hover_text(keys).clicked() {
                    actions.push(act(file, verb, &lines, &picked));
                }
            }
        });
    });
    ui.separator();
    if file.diff.binary {
        pictures(ui, file);
        return;
    }
    if file.diff.hunks.is_empty() {
        let renamed = file.diff.header.iter().find_map(|line| line.strip_prefix("rename from "));
        ui.weak(
            renamed
                .map_or_else(|| "No changes".to_owned(), |from| format!("Renamed from {from}, nothing else changed")),
        );
        return;
    }
    // A hunk's header, then its lines: one per row, or side by side the old and the new version's.
    let rows: Vec<(usize, Option<[Option<usize>; 2]>)> = file
        .diff
        .hunks
        .iter()
        .enumerate()
        .flat_map(|(h, hunk)| {
            let lines = if side_by_side {
                hunk.side_by_side()
            } else {
                (0..hunk.lines.len()).map(|l| [Some(l), None]).collect()
            };
            std::iter::once((h, None)).chain(lines.into_iter().map(move |cells| (h, Some(cells))))
        })
        .collect();
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::both().auto_shrink(false).show_rows(ui, LINE, rows.len(), |ui, range| {
        for &(h, cells) in &rows[range] {
            let hunk = &file.diff.hunks[h];
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), LINE), Sense::hover());
            let painter = ui.painter_at(rect);
            let Some(cells) = cells else {
                painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
                painter.text(
                    rect.left_center() + Vec2::X * 8.0,
                    Align2::LEFT_CENTER,
                    &hunk.header,
                    FONT,
                    Color32::GRAY,
                );
                for (verb, x) in verbs.iter().zip([110.0, 220.0]) {
                    let button = Rect::from_min_size(rect.right_top() - Vec2::X * x, Vec2::new(104.0, LINE));
                    if ui.put(button, egui::Button::new(RichText::new(format!("{verb} hunk")).small())).clicked() {
                        actions.push(act(file, verb, "this hunk", &|hh, _| hh == h));
                    }
                }
                continue;
            };
            let [old, new] = cells;
            if !side_by_side {
                line(ui, rect, file, (h, old.expect("a line on each row")), None, actions);
                continue;
            }
            let (left, right) = rect.split_left_right_at_fraction(0.5);
            for (side, (rect, l)) in [(left, old), (right, new)].into_iter().enumerate() {
                match l {
                    Some(l) => line(ui, rect, file, (h, l), Some(side), actions),
                    None => {
                        painter.rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
                    }
                }
            }
            painter.vline(rect.center().x, rect.y_range(), ui.visuals().widgets.noninteractive.bg_stroke);
        }
    });
}

/// The old and the new version of a picture side by side, each as large as it is or as fits.
fn pictures(ui: &mut Ui, file: &OpenFile) {
    let pictures = match &file.versions {
        Versions::Pictures(pictures) if pictures.iter().any(Option::is_some) => pictures,
        _ => {
            if file.reading.is_some() {
                ui.spinner();
            } else {
                ui.weak("Binary file");
            }
            return;
        }
    };
    ui.columns(2, |columns| {
        for ((ui, version), name) in columns.iter_mut().zip(pictures).zip(["Old", "New"]) {
            ui.weak(match version {
                None => format!("{name} · none"),
                Some(Err(error)) => format!("{name} · cannot be shown: {error}"),
                Some(Ok(Picture { size: [width, height], bytes: n, .. })) => {
                    format!("{name} · {width} × {height} · {}", bytes(*n))
                }
            });
            if let Some(Ok(picture)) = version {
                let image = egui::Image::new(&picture.texture).fit_to_original_size(1.0);
                egui::Frame::canvas(ui.style()).show(ui, |ui| ui.add(image.max_size(ui.available_size())));
            }
        }
    });
}

/// A number of bytes, as people read it.
pub fn bytes(n: usize) -> String {
    let tenths = |unit: usize| format!("{}.{}", n / unit, n % unit * 10 / unit);
    match n {
        ..1024 => format!("{n} bytes"),
        1024..1_048_576 => format!("{} KB", tenths(1024)),
        _ => format!("{} MB", tenths(1_048_576)),
    }
}

/// Draws line `l` of hunk `h` in `rect`, after its numbers in the old and the new version, or
/// side by side only the one of its `side` (0 is old, 1 is new). A click picks or unpicks a
/// changed line for staging.
fn line(
    ui: &mut Ui,
    rect: Rect,
    file: &OpenFile,
    (h, l): (usize, usize),
    side: Option<usize>,
    actions: &mut Vec<Action>,
) {
    let hunk = &file.diff.hunks[h];
    let line = &hunk.lines[l];
    let numbers = [line.old, line.new];
    let numbers = side.map_or(&numbers[..], |side| &numbers[side..=side]);
    let (background, word, marker) = match line.kind {
        LineKind::Added => (theme::ADDED, theme::ADDED_WORD, "+ "),
        LineKind::Removed => (theme::REMOVED, theme::REMOVED_WORD, "- "),
        LineKind::Context | LineKind::NoNewline => (Color32::TRANSPARENT, Color32::TRANSPARENT, "  "),
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, if file.lines.contains(&(h, l)) { theme::PICKED } else { background });
    let at = |x: f32| rect.left_center() + Vec2::X * x;
    let mut x = 0.0;
    for n in numbers {
        x += NUMBER;
        let n = n.map(|n| n.to_string()).unwrap_or_default();
        painter.text(at(x - 6.0), Align2::RIGHT_CENTER, n, FONT, Color32::DARK_GRAY);
    }
    let color = ui.visuals().text_color();
    // The version the line is in, the old one for a line in both.
    let version = usize::from(line.kind == LineKind::Added);
    let words = hunk.pair(l).map_or_else(Vec::new, |pair| {
        ui.memory_mut(|m| m.caches.cache::<FrameCache<_, Words>>().get(pair)[version].clone())
    });
    let text = line.text.trim_end_matches('\r');
    // The line's colours, as long as the version read has the same line there.
    let colours = match &file.versions {
        Versions::Code(code) => [line.old, line.new][version]
            .and_then(|n| code[version].get((n as usize).checked_sub(1)?))
            .filter(|coloured| coloured.text == text)
            .map(|coloured| &coloured.colours[..]),
        _ => None,
    };
    let job = line_text(marker, text, colours.unwrap_or_default(), (&words, word), &FONT, color);
    let galley = painter.layout_job(job);
    painter.galley(at(x + 6.0) - Vec2::Y * galley.size().y / 2.0, galley, color);
    let changed = matches!(line.kind, LineKind::Added | LineKind::Removed);
    if changed
        && !verbs(&file.source, true).is_empty()
        && ui.interact(rect, egui::Id::unique(("diff line", h, l)), Sense::click()).clicked()
    {
        actions.push(Action::ToggleLine(h, l));
    }
}
