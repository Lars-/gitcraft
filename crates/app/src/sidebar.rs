//! The branches, remotes, tags and stashes on the left.

use eframe::egui::{self, Button, Ui};
use gitcraft_core::git::Op;
use gitcraft_core::repo::{Label, RefKind};

use crate::app::{Action, Confirm, Prompt, Selection, Workspace};
use crate::graph::ref_items;

/// Shows the sidebar.
pub fn show(ui: &mut Ui, ws: &Workspace, actions: &mut Vec<Action>) {
    let history = &ws.snapshot.history;
    let mut refs: Vec<(&Label, usize)> =
        history.labels.iter().flat_map(|(&row, labels)| labels.iter().map(move |label| (label, row))).collect();
    refs.sort_by(|a, b| (a.0.kind, &a.0.name).cmp(&(b.0.kind, &b.0.name)));
    egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for (kind, title) in [(RefKind::Local, "Local"), (RefKind::Remote, "Remote"), (RefKind::Tag, "Tags")] {
            let of_kind: Vec<_> = refs.iter().copied().filter(|(label, _)| label.kind == kind).collect();
            egui::CollapsingHeader::new(format!("{title}  {}", of_kind.len())).default_open(kind != RefKind::Tag).show(
                ui,
                |ui| {
                    if kind == RefKind::Remote {
                        remotes(ui, ws, &of_kind, actions);
                    } else {
                        for reference in of_kind {
                            ref_row(ui, ws, reference, &reference.0.name, actions);
                        }
                    }
                },
            );
        }
        egui::CollapsingHeader::new(format!("Stashes  {}", ws.snapshot.stashes.len())).default_open(true).show(
            ui,
            |ui| {
                for (n, message) in ws.snapshot.stashes.iter().enumerate() {
                    ui.add(Button::selectable(false, message).truncate()).context_menu(|ui| {
                        let question = format!("Delete the stash \"{message}\"?");
                        items(
                            ui,
                            actions,
                            [
                                ("Apply stash", Action::Run(Op::StashApply(n))),
                                ("Pop stash", Action::Run(Op::StashPop(n))),
                                (
                                    "Delete stash",
                                    Action::Confirm(Confirm { question, verb: "Delete", op: Op::StashDrop(n) }),
                                ),
                            ],
                        );
                    });
                }
            },
        );
    });
}

/// Each remote with its remote branches and a menu to edit or remove it, then a way to add one.
fn remotes(ui: &mut Ui, ws: &Workspace, branches: &[(&Label, usize)], actions: &mut Vec<Action>) {
    let remotes = &ws.snapshot.remotes;
    // A remote branch is of the longest remote name it starts with, as git reads `a/b/c` when
    // there are remotes `a` and `a/b`.
    let remote_of = |branch: &str| {
        let of = |name: &str| branch.strip_prefix(name).is_some_and(|rest| rest.starts_with('/'));
        remotes.iter().map(|remote| remote.name.as_str()).filter(|name| of(name)).max_by_key(|name| name.len())
    };
    for remote in remotes {
        let name = remote.name.as_str();
        let header = egui::CollapsingHeader::new(name).default_open(true).show(ui, |ui| {
            for &branch in branches.iter().filter(|(label, _)| remote_of(&label.name) == Some(name)) {
                ref_row(ui, ws, branch, &branch.0.name[name.len() + 1..], actions);
            }
        });
        header.header_response.on_hover_text(&remote.url).context_menu(|ui| {
            let question = format!("Remove the remote {name} ({}) and its remote branches?", remote.url);
            let op = Op::RemoveRemote(name.to_owned());
            items(
                ui,
                actions,
                [
                    ("Edit remote…", Action::Prompt(Prompt::Remote(Some(remote.clone())))),
                    ("Remove remote…", Action::Confirm(Confirm { question, verb: "Remove", op })),
                ],
            );
        });
    }
    // A fetch with a refspec of its own can leave remote branches of no remote.
    for &branch in branches.iter().filter(|(label, _)| remote_of(&label.name).is_none()) {
        ref_row(ui, ws, branch, &branch.0.name, actions);
    }
    if ui.add(Button::selectable(false, egui::RichText::new("Add remote…").weak())).clicked() {
        actions.push(Action::Prompt(Prompt::Remote(None)));
    }
}

/// A branch or tag shown as `text`: a click shows its commit, a double click checks it out.
fn ref_row(ui: &mut Ui, ws: &Workspace, (label, row): (&Label, usize), text: &str, actions: &mut Vec<Action>) {
    let current = ws.snapshot.status.branch.as_deref();
    let kind = label.kind;
    let is_current = kind == RefKind::Local && Some(label.name.as_str()) == current;
    let text = egui::RichText::new(text);
    let response = ui.add(Button::selectable(false, if is_current { text.strong() } else { text }).truncate());
    if response.clicked() {
        actions.push(Action::Reveal(Selection::Commit(ws.snapshot.history.ids[row])));
    }
    if response.double_clicked() && kind != RefKind::Tag {
        let local =
            if kind == RefKind::Remote { label.name.split_once('/').map_or("", |(_, b)| b) } else { &label.name };
        actions.push(Action::Run(Op::Checkout(local.to_owned())));
    }
    response.context_menu(|ui| {
        ref_items(ui, label, current, &mut |ui: &mut Ui, text: &str, action| items(ui, actions, [(text, action)]));
    });
}

/// Menu items, each closing the menu with its action.
fn items<'a>(ui: &mut Ui, actions: &mut Vec<Action>, items: impl IntoIterator<Item = (&'a str, Action)>) {
    for (text, action) in items {
        if ui.button(text).clicked() {
            actions.push(action);
            ui.close();
        }
    }
}
