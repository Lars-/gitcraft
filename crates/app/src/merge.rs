//! A conflicted file, resolved conflict by conflict: our side and theirs next to each other with
//! the conflicts lined up, and the result below them.

use eframe::egui::{self, Align2, Button, Color32, FontId, Key, Rect, Response, Role, Sense, Ui, Vec2, WidgetInfo};
use gitcraft_core::conflict::{Part, Pick};
use gitcraft_core::git::Op;

use crate::app::{Action, Merge};
use crate::graph::points;
use crate::theme;

const LINE: f32 = 18.0;

/// A row of the sides, or of the result.
#[derive(Debug, Clone, Copy)]
enum Row<'a> {
    /// A line both sides agree on.
    Same(&'a str),
    /// The heading of a conflict; in the result, a conflict without a pick.
    Conflict(usize),
    /// A line of a conflict on our side and theirs; in the result, a line it keeps, on the left.
    Lines(usize, [Option<&'a str>; 2]),
}

impl Row<'_> {
    fn conflict(self) -> Option<usize> {
        match self {
            Self::Same(_) => None,
            Self::Conflict(n) | Self::Lines(n, _) => Some(n),
        }
    }
}

/// The rows of both sides, or with `result`, of the result.
fn rows(merge: &Merge, result: bool) -> Vec<Row<'_>> {
    let mut rows = Vec::new();
    let mut n = 0;
    for part in &merge.conflicts.parts {
        match (part, result) {
            (Part::Same(text), _) => rows.extend(text.lines().map(Row::Same)),
            (Part::Conflict(sides), true) => match merge.picks[n] {
                Some(pick) => {
                    let kept = sides[pick.sides()].iter().flat_map(|side| side.lines());
                    rows.extend(kept.map(|line| Row::Lines(n, [Some(line), None])));
                }
                None => rows.push(Row::Conflict(n)),
            },
            (Part::Conflict(sides), false) => {
                rows.push(Row::Conflict(n));
                let [ours, theirs] = sides.each_ref().map(|side| side.lines().collect::<Vec<_>>());
                let lines = (0..ours.len().max(theirs.len())).map(|i| [ours.get(i).copied(), theirs.get(i).copied()]);
                rows.extend(lines.map(|lines| Row::Lines(n, lines)));
            }
        }
        n += usize::from(matches!(part, Part::Conflict(_)));
    }
    rows
}

/// What was picked for a conflict, in words.
fn picked(pick: Option<Pick>) -> &'static str {
    match pick {
        None => "no pick yet",
        Some(Pick::Ours) => "ours",
        Some(Pick::Theirs) => "theirs",
        Some(Pick::Both) => "both, ours first",
    }
}

/// Fills `cell` with `background` and writes `text` in it.
fn paint(ui: &Ui, cell: Rect, text: &str, colour: Color32, background: Color32) {
    let painter = ui.painter_at(cell);
    painter.rect_filled(cell, 0.0, background);
    painter.text(cell.left_center() + Vec2::X * 8.0, Align2::LEFT_CENTER, text, FontId::monospace(12.5), colour);
}

/// `rows` in a scrolling list at most `height` high, scrolled to conflict `to` when given.
fn list(
    ui: &mut Ui,
    id: &str,
    rows: &[Row],
    height: f32,
    to: Option<usize>,
    mut row: impl FnMut(&Ui, Rect, Response, Row),
) {
    let mut area = egui::ScrollArea::vertical().id_salt(id).max_height(height).auto_shrink(false);
    if let Some(first) = to.and_then(|to| rows.iter().position(|row| row.conflict() == Some(to))) {
        area = area.vertical_scroll_offset(points(first.saturating_sub(3)) * LINE);
    }
    area.show_rows(ui, LINE, rows.len(), |ui, range| {
        for &r in &rows[range] {
            let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), LINE), Sense::click());
            row(ui, rect, response, r);
        }
    });
}

/// Shows the file being resolved.
pub fn show(ui: &mut Ui, merge: &mut Merge, actions: &mut Vec<Action>) {
    let (count, at) = (merge.picks.len(), merge.at);
    let resolved = merge.conflicts.resolve(&merge.picks);
    ui.horizontal(|ui| {
        ui.strong(&merge.path);
        ui.label(format!("Conflict {} of {count}", at + 1));
        for (verb, to) in [("Previous", at + count - 1), ("Next", at + 1)] {
            if ui.add_enabled(count > 1, Button::new(verb)).clicked() {
                actions.push(Action::GoTo(to % count));
            }
        }
        ui.separator();
        for (verb, pick) in [("Take ours", Pick::Ours), ("Take theirs", Pick::Theirs), ("Take both", Pick::Both)] {
            if ui.add(Button::selectable(merge.picks[at] == Some(pick), verb)).clicked() {
                actions.push(Action::Pick(pick));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                actions.push(Action::CloseFile);
            }
            let save = ui.add_enabled(resolved.is_some(), Button::new("Save and mark resolved"));
            if save.on_disabled_hover_text("Pick a side for every conflict first").clicked()
                && let Some(text) = resolved
            {
                actions.push(Action::Run(Op::Save { path: merge.path.clone(), text }));
            }
        });
    });
    ui.separator();
    let [ours, theirs] = &merge.conflicts.names;
    ui.columns(2, |columns| {
        columns[0].strong(format!("Ours: {ours}"));
        columns[1].strong(format!("Theirs: {theirs}"));
    });
    let to = std::mem::take(&mut merge.scroll).then_some(at);
    let visuals = ui.visuals();
    let (text, current, other) = (visuals.text_color(), visuals.selection.bg_fill, visuals.extreme_bg_color);
    ui.spacing_mut().item_spacing.y = 0.0;
    let half = (ui.available_height() - 2.0 * LINE) / 2.0;
    list(ui, "sides", &rows(merge, false), half, to, |ui, rect, response, row| {
        let cells =
            [Rect::from_min_max(rect.min, rect.center_bottom()), Rect::from_min_max(rect.center_top(), rect.max)];
        match row {
            Row::Same(line) => {
                for cell in cells {
                    paint(ui, cell, line, text, Color32::TRANSPARENT);
                }
            }
            Row::Conflict(n) => {
                let name = format!("Conflict {}", n + 1);
                let background = if n == at { current } else { other };
                paint(ui, rect, &format!("{name}: {}", picked(merge.picks[n])), text, background);
                response.widget_info(|| WidgetInfo::labeled(Role::Button, true, &name));
                if response.clicked() {
                    actions.push(Action::GoTo(n));
                }
            }
            Row::Lines(n, lines) => {
                for (side, (cell, line)) in cells.into_iter().zip(lines).enumerate() {
                    let background = match merge.picks[n] {
                        None => theme::CONFLICT,
                        Some(pick) if pick.sides().contains(&side) => theme::ADDED,
                        Some(_) => theme::REMOVED,
                    };
                    if let Some(line) = line {
                        paint(ui, cell, line, text, background);
                    }
                }
            }
        }
    });
    ui.add_space(4.0);
    ui.strong("Result");
    ui.add_space(4.0);
    list(ui, "result", &rows(merge, true), f32::INFINITY, to, |ui, rect, response, row| match row {
        Row::Same(line) => paint(ui, rect, line, text, Color32::TRANSPARENT),
        Row::Lines(_, [line, _]) => paint(ui, rect, line.unwrap_or_default(), text, theme::ADDED),
        Row::Conflict(n) => {
            paint(ui, rect, &format!("Conflict {}: pick a side", n + 1), theme::ERROR, theme::CONFLICT);
            if response.clicked() {
                actions.push(Action::GoTo(n));
            }
        }
    });
}
