//! Who last changed each line of a file: beside each run of lines the commit that changed them
//! last, and a click on a line shows that commit.

use eframe::egui::{self, Align2, Color32, FontId, Key, Rect, RichText, Role, Sense, Stroke, Ui, Vec2, WidgetInfo};

use crate::app::{Action, Selection, Workspace};
use crate::graph::put;
use crate::theme;

const LINE: f32 = 18.0;
/// The width of the commit beside the lines, without its date.
const GUTTER: f32 = 300.0;

/// Shows the blame of the workspace.
pub fn show(ui: &mut Ui, ws: &mut Workspace, actions: &mut Vec<Action>) {
    let Workspace { blame: Some(blame), commits, repo, selection, .. } = ws else { return };
    ui.horizontal(|ui| {
        ui.strong(format!("Blame of {}", blame.path));
        ui.weak(blame.at.as_ref().map_or_else(|| "in the working tree".to_owned(), |at| format!("at {at:.7}")));
        if blame.reading.is_some() {
            ui.spinner();
        } else {
            ui.weak(theme::count(blame.lines.len(), "line"));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Close").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                actions.push(Action::CloseFile);
            }
        });
    });
    ui.separator();
    let (font, dates) = (FontId::monospace(12.5), theme::Dates::of(ui.ctx()));
    let (width, date) = (GUTTER + dates.width(), dates.width());
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, LINE, blame.lines.len(), |ui, rows| {
        let top = ui.clip_rect().top();
        for row in rows {
            let (id, text) = &blame.lines[row];
            let text = text.trim_end_matches('\r');
            let (summary, author, when) = if id.is_null() {
                ("Not committed yet".to_owned(), String::new(), String::new())
            } else {
                match commits.get(repo, *id) {
                    Ok(commit) => (commit.summary().to_owned(), commit.author.clone(), dates.when(commit.time)),
                    Err(error) => (error.to_string(), String::new(), String::new()),
                }
            };
            let line = if id.is_null() { Selection::Wip } else { Selection::Commit(*id) };
            let picked = *selection == Some(line);
            let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), LINE), Sense::click());
            let gutter = Rect::from_min_size(rect.min, Vec2::new(width, LINE));
            let painter = ui.painter_at(rect);
            if picked {
                painter.rect_filled(rect, 0.0, ui.visuals().selection.bg_fill);
            } else if response.hovered() {
                painter.rect_filled(rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
            } else {
                painter.rect_filled(gutter, 0.0, ui.visuals().extreme_bg_color);
            }
            // The commit goes beside the first line of each run of lines it changed, below a line,
            // and beside the top line wholly in sight, so that it stays in sight.
            let run = row == 0 || blame.lines[row - 1].0 != *id;
            if run {
                let edge = ui.visuals().widgets.noninteractive.bg_stroke.color;
                painter.hline(rect.x_range(), rect.top(), Stroke::new(1.0, edge));
            }
            if run || (top..top + LINE).contains(&rect.top()) {
                let column = |left: f32, right: f32| {
                    Rect::from_x_y_ranges(gutter.left() + left..=gutter.right() - right, rect.y_range())
                };
                if !id.is_null() {
                    put(ui, column(8.0, width - 64.0), RichText::new(&id.to_string()[..7]).monospace().weak());
                }
                put(ui, column(72.0, date + 108.0), summary.as_str());
                put(ui, column(width - date - 100.0, date + 8.0), RichText::new(author).weak());
                put(ui, column(width - date, 8.0), RichText::new(when).weak());
            }
            let at = |x: f32| gutter.right_center() + Vec2::X * x;
            painter.text(at(44.0), Align2::RIGHT_CENTER, (row + 1).to_string(), font.clone(), Color32::DARK_GRAY);
            painter.text(at(56.0), Align2::LEFT_CENTER, text, font.clone(), ui.visuals().text_color());
            let name = format!("Line {}: {text} ({summary})", row + 1);
            response.widget_info(|| WidgetInfo::selected(Role::Button, true, picked, &name));
            if response.clicked() {
                actions.push(Action::Reveal(line));
            }
        }
    });
}
