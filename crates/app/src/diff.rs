//! The diff of one file, with staging of hunks and single lines.

use eframe::egui::{self, Align2, Color32, FontId, Key, Rect, RichText, Sense, Ui, Vec2};
use gitcraft_core::diff::{LineKind, Source};
use gitcraft_core::git::Op;

use crate::app::{Action, OpenFile};
use crate::theme;

const LINE: f32 = 18.0;
const NUMBER: f32 = 44.0;

/// Shows the open file.
pub fn show(ui: &mut Ui, file: &OpenFile, actions: &mut Vec<Action>) {
    let reverse = matches!(file.source, Source::Staged(_));
    let stageable = matches!(file.source, Source::Unstaged(_) | Source::Staged(_));
    let verb = if reverse { "Unstage" } else { "Stage" };
    let run = |selected: &dyn Fn(usize, usize) -> bool| {
        Action::Run(Op::ApplyToIndex { patch: file.diff.patch(selected, reverse), reverse })
    };
    ui.horizontal(|ui| {
        ui.strong(file.source.path());
        ui.weak(match file.source {
            Source::Unstaged(_) => "unstaged",
            Source::Staged(_) => "staged",
            Source::Untracked(_) => "untracked",
            Source::Commit { .. } => "",
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                actions.push(Action::CloseFile);
            }
            if stageable && !file.lines.is_empty() && ui.button(format!("{verb} {} lines", file.lines.len())).clicked()
            {
                actions.push(run(&|h, l| file.lines.contains(&(h, l))));
            }
        });
    });
    ui.separator();
    if file.diff.binary {
        ui.weak("Binary file");
        return;
    }
    if file.diff.hunks.is_empty() {
        ui.weak("No changes");
        return;
    }
    let rows: Vec<(usize, Option<usize>)> = file
        .diff
        .hunks
        .iter()
        .enumerate()
        .flat_map(|(h, hunk)| std::iter::once((h, None)).chain((0..hunk.lines.len()).map(move |l| (h, Some(l)))))
        .collect();
    let font = FontId::monospace(12.5);
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::both().auto_shrink(false).show_rows(ui, LINE, rows.len(), |ui, range| {
        for &(h, line) in &rows[range] {
            let hunk = &file.diff.hunks[h];
            let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), LINE), Sense::click());
            let painter = ui.painter_at(rect);
            let Some(l) = line else {
                painter.rect_filled(rect, 0.0, ui.visuals().extreme_bg_color);
                painter.text(
                    rect.left_center() + Vec2::X * 8.0,
                    Align2::LEFT_CENTER,
                    &hunk.header,
                    font.clone(),
                    Color32::GRAY,
                );
                if stageable {
                    let button = Rect::from_min_size(rect.right_top() - Vec2::X * 110.0, Vec2::new(104.0, LINE));
                    if ui.put(button, egui::Button::new(RichText::new(format!("{verb} hunk")).small())).clicked() {
                        actions.push(run(&|hh, _| hh == h));
                    }
                }
                continue;
            };
            let line = &hunk.lines[l];
            let picked = file.lines.contains(&(h, l));
            let (background, marker) = match line.kind {
                LineKind::Added => (theme::ADDED, "+"),
                LineKind::Removed => (theme::REMOVED, "-"),
                LineKind::Context | LineKind::NoNewline => (Color32::TRANSPARENT, " "),
            };
            painter.rect_filled(rect, 0.0, if picked { theme::PICKED } else { background });
            let number = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
            let at = |x: f32| rect.left_center() + Vec2::X * x;
            painter.text(at(NUMBER - 6.0), Align2::RIGHT_CENTER, number(line.old), font.clone(), Color32::DARK_GRAY);
            painter.text(
                at(2.0 * NUMBER - 6.0),
                Align2::RIGHT_CENTER,
                number(line.new),
                font.clone(),
                Color32::DARK_GRAY,
            );
            let text = format!("{marker} {}", line.text.trim_end_matches('\r'));
            painter.text(at(2.0 * NUMBER + 6.0), Align2::LEFT_CENTER, text, font.clone(), ui.visuals().text_color());
            let changed = matches!(line.kind, LineKind::Added | LineKind::Removed);
            if stageable && changed && response.clicked() {
                actions.push(Action::ToggleLine(h, l));
            }
        }
    });
}
