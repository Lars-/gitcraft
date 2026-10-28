//! The panel on the right: the uncommitted changes with the commit form, or a commit.

use eframe::egui::{self, Button, Layout, RichText, TextEdit, Ui};
use gitcraft_core::ObjectId;
use gitcraft_core::diff::Source;
use gitcraft_core::git::Op;
use gitcraft_core::status::Change;

use crate::app::{Action, Selection, Workspace};
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

/// One changed file: its kind of change, its path, and a button.
fn file(ui: &mut Ui, change: Change, path: &str, open: bool, button: Option<&str>) -> (bool, bool) {
    let (letter, colour) = theme::change(change);
    ui.horizontal(|ui| {
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            let pressed = button.is_some_and(|text| ui.small_button(text).clicked());
            ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(RichText::new(letter).monospace().strong().color(colour));
                let clicked = ui.add(Button::selectable(open, path).truncate()).clicked();
                (clicked, pressed)
            })
            .inner
        })
        .inner
    })
    .inner
}

fn changes(ui: &mut Ui, ws: &mut Workspace, actions: &mut Vec<Action>) {
    let open = ws.open_file.as_ref().map(|f| &f.source);
    let entries = &ws.snapshot.status.entries;
    let unstaged: Vec<_> = entries.iter().filter_map(|e| Some((e.unstaged?, e.path.as_str()))).collect();
    let staged: Vec<_> = entries.iter().filter_map(|e| Some((e.staged?, e.path.as_str()))).collect();
    let all = |list: &[(Change, &str)]| list.iter().map(|(_, path)| (*path).to_owned()).collect::<Vec<_>>();
    let half = (ui.available_height() - 210.0).max(120.0) / 2.0;

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
            let (clicked, stage) = file(ui, change, path, open == Some(&source), Some("Stage"));
            if stage {
                actions.push(Action::Run(Op::Stage(vec![path.into()])));
            } else if clicked {
                actions.push(Action::Open(source));
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
            let (clicked, unstage) = file(ui, change, path, open == Some(&source), Some("Unstage"));
            if unstage {
                actions.push(Action::Run(Op::Unstage(vec![path.into()])));
            } else if clicked {
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
            let source = Source::Commit { commit: sha.clone(), path: change.path.clone() };
            if file(ui, change.change, &change.path, open == Some(&source), None).0 {
                actions.push(Action::Open(source));
            }
        }
    });
}
