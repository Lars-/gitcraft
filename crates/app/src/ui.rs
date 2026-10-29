//! The window: the start screen, or a repository with its toolbar and panels.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Button, Key, Ui};
use gitcraft_core::git::Op;
use gitcraft_core::repo::{RefKind, Repo};

use crate::app::{Action, Selection, Workspace};
use crate::start::Start;
use crate::{details, diff, graph, sidebar, theme};

const RECENT: usize = 10;

/// The whole app.
#[derive(Default)]
pub struct App {
    /// The open repository.
    pub workspace: Option<Workspace>,
    /// Recently opened repositories, newest first.
    pub recent: Vec<PathBuf>,
    /// The start screen, shown while no repository is open.
    pub start: Start,
    focused: bool,
    pending: Option<PathBuf>,
}

impl App {
    /// Starts the app, opening `path` or else the last repository on the first frame.
    pub fn new(recent: Vec<PathBuf>, path: Option<PathBuf>) -> Self {
        let pending = path.or_else(|| recent.first().cloned());
        Self { recent, pending, ..Self::default() }
    }

    fn open(&mut self, ctx: &egui::Context, path: &Path) {
        match Repo::open(path) {
            Ok(repo) => {
                let path = repo.git.dir().to_owned();
                self.recent.retain(|p| *p != path);
                self.recent.insert(0, path);
                self.recent.truncate(RECENT);
                self.workspace = Some(Workspace::new(ctx, repo));
                self.start.error = None;
            }
            Err(error) => self.start.error = Some(format!("{}: {error}", path.display())),
        }
    }

    /// Draws a frame.
    pub fn show(&mut self, ui: &mut Ui) {
        if let Some(path) = self.pending.take() {
            self.open(ui.ctx(), &path);
        }
        let mut actions = Vec::new();
        let focused = ui.input(|i| i.focused);
        match &mut self.workspace {
            None => self.start.show(ui, &self.recent, &mut actions),
            Some(ws) => {
                ws.poll();
                if focused && !self.focused {
                    actions.push(Action::Refresh);
                }
                workspace(ui, ws, &mut actions);
            }
        }
        self.focused = focused;
        for action in actions {
            match (action, &mut self.workspace) {
                (Action::OpenRepo(path), _) => self.open(ui.ctx(), &path),
                (action, Some(ws)) => ws.apply(ui.ctx(), action),
                (_, None) => {}
            }
        }
    }
}

/// Asks for a folder.
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

fn workspace(ui: &mut Ui, ws: &mut Workspace, actions: &mut Vec<Action>) {
    egui::Panel::top("toolbar").show(ui, |ui| toolbar(ui, ws, actions));
    if let Some(error) = ws.error.clone() {
        egui::Panel::bottom("error").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.small_button("Dismiss").clicked() {
                    ws.error = None;
                }
                ui.colored_label(theme::ERROR, error);
            });
        });
    }
    egui::Panel::left("sidebar").resizable(true).default_size(220.0).show(ui, |ui| sidebar::show(ui, ws, actions));
    egui::Panel::right("details").resizable(true).default_size(360.0).show(ui, |ui| details::show(ui, ws, actions));
    egui::CentralPanel::default().show(ui, |ui| match &ws.open_file {
        Some(file) => diff::show(ui, file, actions),
        None => graph::show(ui, ws, actions),
    });
    prompt(ui.ctx(), ws, actions);
}

fn toolbar(ui: &mut Ui, ws: &Workspace, actions: &mut Vec<Action>) {
    let status = &ws.snapshot.status;
    ui.horizontal(|ui| {
        if ui.button("Open…").clicked()
            && let Some(folder) = pick_folder("Open a repository")
        {
            actions.push(Action::OpenRepo(folder));
        }
        ui.strong(ws.repo.name());
        ui.weak(status.branch.as_deref().unwrap_or("detached HEAD"));
        ui.separator();
        let idle = ws.busy().is_none();
        let count = |label: &str, n: u32| if n > 0 { format!("{label}  {n}") } else { label.to_owned() };
        let remote = ws.snapshot.history.labels.values().flatten().find(|l| l.kind == RefKind::Remote);
        let remote = remote.and_then(|l| l.name.split_once('/')).map_or("origin", |(remote, _)| remote);
        let at = match ws.selection {
            Some(Selection::Commit(id)) => id.to_string(),
            _ => "HEAD".to_owned(),
        };
        let buttons = [
            ("Fetch", true, Action::Run(Op::Fetch)),
            (&count("Pull", status.behind), true, Action::Run(Op::Pull { rebase: false })),
            (
                &count("Push", status.ahead),
                status.branch.is_some(),
                Action::Run(Op::Push { upstream: status.upstream.is_none().then(|| remote.to_owned()), force: false }),
            ),
            ("Branch", true, Action::Prompt(crate::app::Prompt::Branch { at })),
            (
                "Stash",
                !status.is_clean(),
                Action::Run(Op::Stash(format!("WIP on {}", status.branch.as_deref().unwrap_or("HEAD")))),
            ),
            ("Pop", !ws.snapshot.stashes.is_empty(), Action::Run(Op::StashPop(0))),
            ("Refresh", true, Action::Refresh),
        ];
        for (label, enabled, action) in buttons {
            if ui.add_enabled(idle && enabled, Button::new(label)).clicked() {
                actions.push(action);
            }
        }
        if let Some(busy) = ws.busy() {
            ui.spinner();
            ui.weak(busy);
        }
    });
}

fn prompt(ctx: &egui::Context, ws: &mut Workspace, actions: &mut Vec<Action>) {
    let Some((prompt, name)) = &mut ws.prompt else { return };
    let mut done = false;
    let response = egui::Modal::new(egui::Id::unique("prompt")).show(ctx, |ui| {
        let what = match prompt {
            crate::app::Prompt::Branch { .. } => "New branch",
            crate::app::Prompt::Tag { .. } => "New tag",
        };
        ui.strong(what);
        let field = ui.add(egui::TextEdit::singleline(name).hint_text("name")).on_hover_text("Name");
        field.request_focus();
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        ui.horizontal(|ui| {
            if (ui.button("Create").clicked() || enter) && !name.trim().is_empty() {
                actions.push(Action::Run(prompt.op(name.trim().to_owned())));
                done = true;
            }
            done |= ui.button("Cancel").clicked();
        });
    });
    if done || response.should_close() {
        ws.prompt = None;
    }
}
