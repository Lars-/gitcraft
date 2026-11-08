//! An interactive rebase being put together: the commits it takes along, newest first like the
//! graph, each with what to do with it, in an order to change with the arrows or by dragging.

use std::sync::Arc;

use eframe::egui::{self, Button, Id, Key, RichText, Role, TextEdit, Ui, WidgetInfo};
use gitcraft_core::ObjectId;
use gitcraft_core::git::Step;

use crate::app::{Action, Workspace};
use crate::theme;

/// Shows the plan of the workspace.
pub fn show(ui: &mut Ui, ws: &mut Workspace, actions: &mut Vec<Action>) {
    let idle = ws.busy().is_none();
    let Workspace { plan: Some(plan), commits, repo, .. } = ws else { return };
    let count = plan.steps.len();
    ui.horizontal(|ui| {
        ui.strong(format!("Interactive rebase onto {:.7}", plan.onto));
        ui.label(theme::count(count, "commit"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Cancel").clicked() {
                actions.push(Action::CloseFile);
            }
            let op = plan.op();
            let mut start = ui.add_enabled(idle && op.is_ok(), Button::new("Start rebase"));
            if let Err(why) = &op {
                start = start.on_disabled_hover_text(*why);
            }
            if start.clicked()
                && let Ok(op) = op
            {
                actions.push(Action::Run(op));
            }
        });
    });
    ui.weak("Reword gives a commit a new message. Squash and fixup meld a commit into the one below it. Squash keeps both messages, fixup that of the one below.");
    ui.separator();
    let typed = egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        // Escape in a message leaves the message, not the rebase.
        let mut left = false;
        for (row, &(step, id)) in plan.steps.iter().enumerate() {
            let summary = match commits.get(repo, id) {
                Ok(commit) => commit.summary().to_owned(),
                Err(error) => error.to_string(),
            };
            let response = ui
                .horizontal(|ui| {
                    for (arrow, way, to) in [("↑", "up", row.checked_sub(1)), ("↓", "down", Some(row + 1))] {
                        let to = to.filter(|&to| to < count);
                        let button =
                            ui.add_enabled(to.is_some(), Button::new(RichText::new(arrow).monospace()).small());
                        button.widget_info(|| {
                            WidgetInfo::labeled(Role::Button, to.is_some(), format!("Move {summary} {way}"))
                        });
                        if button.clicked()
                            && let Some(to) = to
                        {
                            actions.push(Action::Shift(row, to));
                        }
                    }
                    for choice in Step::ALL {
                        let (name, chosen) = (theme::capital(choice.word()), step == choice);
                        let button = ui.add(Button::selectable(chosen, &name));
                        button.widget_info(|| {
                            WidgetInfo::selected(Role::Button, true, chosen, format!("{name} {summary}"))
                        });
                        if button.clicked() {
                            actions.push(Action::Step(row, choice));
                        }
                    }
                    ui.dnd_drag_source(Id::unique(("rebase", id)), id, |ui| {
                        ui.monospace(RichText::new(&id.to_string()[..7]).weak());
                        let text = RichText::new(&summary);
                        ui.label(match step {
                            Step::Pick => text,
                            Step::Reword | Step::Squash | Step::Fixup => text.weak(),
                            Step::Drop => text.strikethrough().weak(),
                        });
                    });
                })
                .response;
            // A commit dragged onto another takes its place, like the arrows.
            let dragged =
                |id: Option<Arc<ObjectId>>| id.and_then(|id| plan.steps.iter().position(|step| step.1 == *id));
            if let Some(from) = dragged(response.dnd_hover_payload()).filter(|&from| from != row) {
                let y = if from < row { response.rect.bottom() } else { response.rect.top() };
                ui.painter().hline(response.rect.x_range(), y, ui.visuals().selection.stroke);
            }
            if let Some(from) = dragged(response.dnd_release_payload()).filter(|&from| from != row) {
                actions.push(Action::Shift(from, row));
            }
            if step == Step::Reword
                && let Some(message) = plan.messages.get_mut(&id)
            {
                let field = TextEdit::multiline(message).desired_rows(3).desired_width(f32::INFINITY);
                let field = ui.add(field).accessible_name(format!("New message of {summary}"));
                if plan.focus == Some(id) {
                    plan.focus = None;
                    field.request_focus();
                    field.scroll_to_me(Some(egui::Align::Center));
                }
                left |= field.lost_focus();
            }
        }
        left
    });
    if !typed.inner && ui.input(|i| i.key_pressed(Key::Escape)) {
        actions.push(Action::CloseFile);
    }
}
