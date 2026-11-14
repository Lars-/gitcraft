//! The history of a file: the commits that changed it, newest first, above the diff of the one
//! picked.

use eframe::egui::{self, Key, Rect, RichText, Role, Sense, Ui, Vec2, WidgetInfo};

use crate::app::{Action, Workspace};
use crate::graph::{ROW, put};
use crate::{diff, theme};

/// Shows the history of the workspace.
pub fn show(ui: &mut Ui, ws: &mut Workspace, actions: &mut Vec<Action>) {
    let Workspace { history: Some(history), commits, repo, open_file, .. } = ws else { return };
    egui::Panel::top("file history").resizable(true).default_size(240.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.strong(format!("History of {}", history.path));
            if history.reading.is_some() {
                ui.spinner();
            } else {
                ui.weak(theme::count(history.commits.len(), "commit"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                    actions.push(Action::CloseFile);
                }
            });
        });
        if history.reading.is_none() && history.commits.is_empty() {
            ui.weak("No commit changed this file yet.");
        }
        let now = theme::now();
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, ROW, history.commits.len(), |ui, rows| {
            for row in rows {
                let (id, file) = &history.commits[row];
                let open = open_file.as_ref().is_some_and(|open| open.source == history.source(row));
                let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::click());
                if open {
                    ui.painter().rect_filled(rect, 0.0, ui.visuals().selection.bg_fill);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                // Between a margin on either side: the SHA, the summary, the author and the date.
                let column = |left: f32, right: f32| {
                    Rect::from_x_y_ranges(rect.left() + left..=rect.right() - right, rect.y_range())
                };
                let width = rect.width();
                put(ui, column(8.0, width - 70.0), RichText::new(&id.to_string()[..7]).monospace().weak());
                let commit = match commits.get(repo, *id) {
                    Ok(commit) => commit,
                    Err(error) => {
                        put(ui, column(80.0, 8.0), RichText::new(error.to_string()).color(theme::ERROR));
                        continue;
                    }
                };
                let renamed = file.from.as_ref().map(|from| format!("  (from {from})")).unwrap_or_default();
                put(ui, column(80.0, 256.0), format!("{}{renamed}", commit.summary()));
                put(ui, column(width - 248.0, 108.0), RichText::new(&commit.author).weak());
                put(ui, column(width - 100.0, 8.0), RichText::new(theme::when(commit.time, now)).weak());
                response.widget_info(|| WidgetInfo::selected(Role::Button, true, open, commit.summary()));
                if response.clicked() {
                    actions.push(Action::Visit(row));
                }
            }
        });
    });
    if let Some(file) = open_file {
        diff::show(ui, file, false, actions);
    }
}
