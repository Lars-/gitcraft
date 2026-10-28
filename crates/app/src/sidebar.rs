//! The branches, tags and stashes on the left.

use eframe::egui::{self, Button, Ui};
use gitcraft_core::git::Op;
use gitcraft_core::repo::{Label, RefKind};

use crate::app::{Action, Selection, Workspace};
use crate::graph::ref_items;

/// Shows the sidebar.
pub fn show(ui: &mut Ui, ws: &Workspace, actions: &mut Vec<Action>) {
    let history = &ws.snapshot.history;
    let mut refs: Vec<(&Label, usize)> =
        history.labels.iter().flat_map(|(&row, labels)| labels.iter().map(move |label| (label, row))).collect();
    refs.sort_by(|a, b| (a.0.kind, &a.0.name).cmp(&(b.0.kind, &b.0.name)));
    let current = ws.snapshot.status.branch.as_deref();
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (kind, title) in [(RefKind::Local, "Local"), (RefKind::Remote, "Remote"), (RefKind::Tag, "Tags")] {
            let of_kind: Vec<_> = refs.iter().filter(|(label, _)| label.kind == kind).collect();
            egui::CollapsingHeader::new(format!("{title}  {}", of_kind.len())).default_open(kind != RefKind::Tag).show(
                ui,
                |ui| {
                    for &&(label, row) in &of_kind {
                        let is_current = kind == RefKind::Local && Some(label.name.as_str()) == current;
                        let text = egui::RichText::new(&label.name);
                        let response =
                            ui.add(Button::selectable(false, if is_current { text.strong() } else { text }).truncate());
                        if response.clicked() {
                            actions.push(Action::Reveal(Selection::Commit(history.ids[row])));
                        }
                        if response.double_clicked() && kind != RefKind::Tag {
                            let local = if kind == RefKind::Remote {
                                label.name.split_once('/').map_or("", |(_, b)| b)
                            } else {
                                &label.name
                            };
                            actions.push(Action::Run(Op::Checkout(local.to_owned())));
                        }
                        response.context_menu(|ui| {
                            ref_items(ui, label, current, &mut |ui: &mut Ui, text: &str, action| {
                                if ui.button(text).clicked() {
                                    actions.push(action);
                                    ui.close();
                                }
                            });
                        });
                    }
                },
            );
        }
        egui::CollapsingHeader::new(format!("Stashes  {}", ws.snapshot.stashes.len())).default_open(true).show(
            ui,
            |ui| {
                for (n, message) in ws.snapshot.stashes.iter().enumerate() {
                    ui.add(Button::selectable(false, message).truncate()).context_menu(|ui| {
                        for (text, op) in [
                            ("Apply stash", Op::StashApply(n)),
                            ("Pop stash", Op::StashPop(n)),
                            ("Delete stash", Op::StashDrop(n)),
                        ] {
                            if ui.button(text).clicked() {
                                actions.push(Action::Run(op));
                                ui.close();
                            }
                        }
                    });
                }
            },
        );
    });
}
