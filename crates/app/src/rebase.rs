//! An interactive rebase being put together: the commits it takes along, newest first like the
//! graph, each with what to do with it.

use eframe::egui::{self, Button, Key, RichText, Role, Ui, WidgetInfo};
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
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape)) {
                actions.push(Action::CloseFile);
            }
            let op = plan.op();
            let start = ui.add_enabled(idle && op.is_some(), Button::new("Start rebase"));
            if start.on_disabled_hover_text("The oldest commit it keeps has nothing below it to meld into").clicked()
                && let Some(op) = op
            {
                actions.push(Action::Run(op));
            }
        });
    });
    ui.weak("Squash and fixup meld a commit into the one below it. Squash keeps both messages, fixup that of the one below.");
    ui.separator();
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (row, &(step, id)) in plan.steps.iter().enumerate() {
            let summary = match commits.get(repo, id) {
                Ok(commit) => commit.summary().to_owned(),
                Err(error) => error.to_string(),
            };
            ui.horizontal(|ui| {
                for (arrow, way, to) in [("↑", "up", row.checked_sub(1)), ("↓", "down", Some(row + 1))] {
                    let to = to.filter(|&to| to < count);
                    let button = ui.add_enabled(to.is_some(), Button::new(RichText::new(arrow).monospace()).small());
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
                    button
                        .widget_info(|| WidgetInfo::selected(Role::Button, true, chosen, format!("{name} {summary}")));
                    if button.clicked() {
                        actions.push(Action::Step(row, choice));
                    }
                }
                ui.monospace(RichText::new(&id.to_string()[..7]).weak());
                let text = RichText::new(&summary);
                ui.label(match step {
                    Step::Pick => text,
                    Step::Squash | Step::Fixup => text.weak(),
                    Step::Drop => text.strikethrough().weak(),
                });
            });
        }
    });
}
