//! The panel on the right: the uncommitted changes with the commit form, or a commit.

use eframe::egui::{self, Button, Layout, Response, RichText, Role, TextEdit, Ui, WidgetInfo};
use gitcraft_core::ObjectId;
use gitcraft_core::diff::Source;
use gitcraft_core::git::Op;
use gitcraft_core::status::Change;

use crate::app::{Action, Confirm, Selection, Workspace};
use crate::theme;

/// Shows the panel.
pub fn show(ui: &mut Ui, ws: &mut Workspace, actions: &mut Vec<Action>) {
    match ws.selection {
        Some(Selection::Wip) => changes(ui, ws, actions),
        Some(Selection::Commit(id)) => commit(ui, ws, id, actions),
        None => {
            ui.weak("Select a commit to see its changes.");
        }
    }
}

/// One changed file: its kind of change, its path, and `buttons` on the right, right to left.
/// Returns the path.
fn file(ui: &mut Ui, change: Change, path: &str, open: bool, buttons: impl FnOnce(&mut Ui)) -> Response {
    let (letter, colour) = theme::change(change);
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            buttons(ui);
            ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(RichText::new(letter).monospace().strong().color(colour));
                ui.add(Button::selectable(open, path).truncate())
            })
            .inner
        })
        .inner
    })
    .inner
}

/// The right-click menu of a file: its history from the commit `at` back, if it has one.
fn file_menu(file: &Response, at: Option<&str>, path: &str, actions: &mut Vec<Action>) {
    let Some(at) = at else { return };
    file.context_menu(|ui| {
        if ui.button("File history").clicked() {
            actions.push(Action::History { at: at.to_owned(), path: path.to_owned() });
            ui.close();
        }
    });
}

/// Throwing away the unstaged changes to a file, or the whole file when git does not track it.
fn discard(change: Change, path: &str) -> Confirm {
    if change == Change::Untracked {
        let question = format!("Delete {path}? Git does not track it.");
        return Confirm { question, verb: "Delete", op: Op::DeleteUntracked(vec![path.into()]) };
    }
    let question = format!("Discard the unstaged changes to {path}?");
    Confirm { question, verb: "Discard", op: Op::Discard(vec![path.into()]) }
}

/// The buttons of a conflicted file: take the whole file from one of the `sides` (name, commit,
/// whether it has the file), when git says what they are, or mark it resolved as it is now.
fn resolve(ui: &mut Ui, path: &str, sides: Option<[(&str, &str, bool); 2]>, actions: &mut Vec<Action>) {
    for (side, from, has) in sides.into_iter().flatten() {
        let (op, hint) = if has {
            (Op::Restore { from: from.into(), paths: vec![path.into()] }, format!("Take the whole file from {from}"))
        } else {
            (Op::Remove(vec![path.into()]), format!("Delete the file, as {from} did"))
        };
        let button = ui.small_button(side).on_hover_text(hint);
        let name = format!("Take {} for {path}", side.to_lowercase());
        button.widget_info(|| WidgetInfo::labeled(Role::Button, true, &name));
        if button.clicked() {
            actions.push(Action::Run(op));
        }
    }
    if ui.small_button("Mark resolved").clicked() {
        actions.push(Action::Run(Op::Stage(vec![path.into()])));
    }
}

fn changes(ui: &mut Ui, ws: &mut Workspace, actions: &mut Vec<Action>) {
    let underway = ws.snapshot.underway.as_ref();
    let open = ws.open_file.as_ref().map(|f| &f.source);
    let merging = ws.merge.as_ref().map(|m| m.path.as_str());
    let entries = &ws.snapshot.status.entries;
    let unstaged: Vec<_> = entries.iter().filter_map(|e| Some((e.unstaged?, e.path.as_str()))).collect();
    let staged: Vec<_> = entries.iter().filter_map(|e| Some((e.staged?, e.path.as_str()))).collect();
    let all = |list: &[(Change, &str)]| list.iter().map(|(_, path)| (*path).to_owned()).collect::<Vec<_>>();
    let half = (ui.available_height() - 210.0).max(120.0) / 2.0;
    // Before the first commit, no file has a history; one git does not track never has.
    let head = ws.snapshot.history.head.map(|_| "HEAD");

    ui.horizontal(|ui| {
        ui.strong(format!("Unstaged files  {}", unstaged.len()));
        if !unstaged.is_empty() && ui.small_button("Stage all").clicked() {
            actions.push(Action::Run(Op::Stage(all(&unstaged))));
        }
    });
    egui::ScrollArea::vertical().id_salt("unstaged").max_height(half).auto_shrink([false, true]).show(ui, |ui| {
        for &(change, path) in &unstaged {
            let source = if change == Change::Untracked {
                Source::Untracked(path.into())
            } else {
                Source::Unstaged(path.into())
            };
            let conflicted = matches!(change, Change::Conflicted { .. });
            let shown = if conflicted { merging == Some(path) } else { open == Some(&source) };
            let response = file(ui, change, path, shown, |ui| {
                // A conflicted file has its buttons on a line of their own, below.
                if conflicted {
                    return;
                }
                if ui.small_button("Stage").clicked() {
                    actions.push(Action::Run(Op::Stage(vec![path.into()])));
                }
                let button = ui.small_button("Discard");
                button.widget_info(|| WidgetInfo::labeled(Role::Button, true, format!("Discard {path}")));
                if button.clicked() {
                    actions.push(Action::Confirm(discard(change, path)));
                }
            });
            file_menu(&response, head.filter(|_| change != Change::Untracked), path, actions);
            if let Change::Conflicted { ours, theirs } = change {
                let sides = underway.map(|u| [("Ours", "HEAD", ours), ("Theirs", u.theirs, theirs)]);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    resolve(ui, path, sides, actions);
                });
            }
            if response.clicked() {
                actions.push(match change {
                    Change::Conflicted { ours: true, theirs: true } => Action::Resolve(path.into()),
                    // A file one side deleted has no lines to line up.
                    Change::Conflicted { .. } => continue,
                    _ => Action::Open(source),
                });
            }
        }
    });
    ui.separator();
    ui.horizontal(|ui| {
        ui.strong(format!("Staged files  {}", staged.len()));
        if !staged.is_empty() && ui.small_button("Unstage all").clicked() {
            actions.push(Action::Run(Op::Unstage(all(&staged))));
        }
    });
    egui::ScrollArea::vertical().id_salt("staged").max_height(half).auto_shrink([false, true]).show(ui, |ui| {
        for &(change, path) in &staged {
            let source = Source::Staged(path.into());
            let response = file(ui, change, path, open == Some(&source), |ui| {
                if ui.small_button("Unstage").clicked() {
                    actions.push(Action::Run(Op::Unstage(vec![path.into()])));
                }
            });
            file_menu(&response, head, path, actions);
            if response.clicked() {
                actions.push(Action::Open(source));
            }
        }
    });
    ui.separator();
    let staged = staged.len();
    ui.add(TextEdit::singleline(&mut ws.summary).hint_text("Summary").desired_width(f32::INFINITY));
    ui.add(
        TextEdit::multiline(&mut ws.description).hint_text("Description").desired_rows(4).desired_width(f32::INFINITY),
    );
    if ui.checkbox(&mut ws.amend, "Amend previous commit").changed() && ws.amend && ws.summary.is_empty() {
        let head = ws.snapshot.history.head.map(|row| ws.snapshot.history.ids[row]);
        if let Some(Ok(commit)) = head.map(|id| ws.commits.get(&ws.repo, id)) {
            let (summary, description) = commit.message.split_once('\n').unwrap_or((&commit.message, ""));
            (ws.summary, ws.description) = (summary.to_owned(), description.trim().to_owned());
        }
    }
    let ready = !ws.summary.trim().is_empty() && (staged > 0 || ws.amend) && ws.busy().is_none();
    let label = if ws.amend {
        "Amend previous commit".to_owned()
    } else {
        format!("Commit changes to {}", theme::count(staged, "file"))
    };
    if ui.add_enabled(ready, Button::new(label).min_size(egui::vec2(ui.available_width(), 30.0))).clicked() {
        let message = [ws.summary.trim(), ws.description.trim()].join("\n\n");
        actions.push(Action::Run(Op::Commit { message, amend: ws.amend }));
    }
}

fn commit(ui: &mut Ui, ws: &mut Workspace, id: ObjectId, actions: &mut Vec<Action>) {
    let commit = match ws.commits.get(&ws.repo, id) {
        Ok(commit) => commit.clone(),
        Err(error) => {
            ui.colored_label(theme::ERROR, error.to_string());
            return;
        }
    };
    let sha = id.to_string();
    let (summary, body) = commit.message.split_once('\n').unwrap_or((&commit.message, ""));
    ui.label(RichText::new(summary).strong().size(15.0));
    if !body.trim().is_empty() {
        ui.label(body.trim());
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.weak("commit");
        if ui.add(Button::new(RichText::new(&sha[..10]).monospace()).frame(false)).on_hover_text("Copy").clicked() {
            ui.ctx().copy_text(sha.clone());
        }
    });
    if let Some(row) = ws.snapshot.history.row(id) {
        ui.horizontal(|ui| {
            ui.weak("parents");
            for parent in ws.snapshot.history.graph.parents(row) {
                let parent = ws.snapshot.history.ids[parent];
                if ui.add(Button::new(RichText::new(&parent.to_string()[..7]).monospace()).frame(false)).clicked() {
                    actions.push(Action::Reveal(Selection::Commit(parent)));
                }
            }
        });
    }
    ui.label(format!("{} <{}>", commit.author, commit.email));
    ui.weak(format!("authored {}", commit.time.format_or_unix(gitcraft_core::date::time::format::ISO8601)));
    ui.separator();
    let Some((_, files)) = ws.files.as_ref().filter(|(of, _)| *of == id) else { return };
    ui.strong(theme::count(files.len(), "changed file"));
    let open = ws.open_file.as_ref().map(|f| &f.source);
    egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
        for change in files {
            let source = Source::Commit { commit: sha.clone(), path: change.path.clone(), from: change.from.clone() };
            let response = file(ui, change.change, &change.path, open == Some(&source), |_| {});
            file_menu(&response, Some(&sha), &change.path, actions);
            if response.clicked() {
                actions.push(Action::Open(source));
            }
        }
    });
}
