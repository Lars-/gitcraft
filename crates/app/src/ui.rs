//! The window: the start screen, or a repository with its toolbar and panels.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Button, Key, Role, Ui, WidgetInfo};
use gitcraft_core::git::Op;
use gitcraft_core::repo::{RefKind, Repo};
use gitcraft_core::status::Change;

use crate::app::{Action, Confirm, Selection, Workspace};
use crate::start::Start;
use crate::{details, diff, graph, merge, sidebar, theme};

const RECENT: usize = 10;

/// Where [`App::saved`] keeps the recent repositories, the open tabs and the tab on screen.
pub const KEYS: [&str; 3] = ["recent", "tabs", "active tab"];

/// The whole app.
#[derive(Default)]
pub struct App {
    /// The open repositories, one per tab.
    pub tabs: Vec<Workspace>,
    /// The tab on screen, or `None` for the start screen.
    pub active: Option<usize>,
    /// Recently opened repositories, newest first.
    pub recent: Vec<PathBuf>,
    /// The start screen, shown while no tab is.
    pub start: Start,
    focused: bool,
    pending: Vec<PathBuf>,
}

impl App {
    /// Starts the app with what [`Self::saved`] stored, opening those tabs and `path` on the first frame.
    pub fn restore(stored: impl Fn(&str) -> Option<String>, path: Option<PathBuf>) -> Self {
        let [recent, tabs, active] =
            KEYS.map(|key| stored(key).map(|v| v.lines().map(PathBuf::from).collect::<Vec<_>>()));
        let recent = recent.unwrap_or_default();
        // Versions without tabs opened the last repository again.
        let tabs = tabs.unwrap_or_else(|| recent.iter().take(1).cloned().collect());
        let pending = tabs.into_iter().chain(active.into_iter().flatten()).chain(path).collect();
        Self { recent, pending, ..Self::default() }
    }

    /// What to remember until the next run, one value for each of [`KEYS`].
    pub fn saved(&self) -> [String; 3] {
        let tabs = self.tabs.iter().map(|ws| ws.repo.git.dir());
        [
            lines(self.recent.iter().map(PathBuf::as_path)),
            lines(tabs),
            lines(self.workspace().map(|ws| ws.repo.git.dir())),
        ]
    }

    /// The repository on screen.
    pub fn workspace(&self) -> Option<&Workspace> {
        self.tabs.get(self.active?)
    }

    /// The repository on screen.
    pub fn workspace_mut(&mut self) -> Option<&mut Workspace> {
        self.tabs.get_mut(self.active?)
    }

    fn open(&mut self, ctx: &egui::Context, path: &Path) {
        match Repo::open(path) {
            Ok(repo) => {
                let path = repo.git.dir().to_owned();
                self.recent.retain(|p| *p != path);
                self.recent.insert(0, path.clone());
                self.recent.truncate(RECENT);
                let tab = self.tabs.iter().position(|ws| ws.repo.git.dir() == path).unwrap_or_else(|| {
                    self.tabs.push(Workspace::new(ctx, repo));
                    self.tabs.len() - 1
                });
                self.active = Some(tab);
                self.start.clear();
            }
            Err(error) => {
                self.start.error = Some(format!("{}: {error}", path.display()));
                self.active = None;
            }
        }
    }

    /// Closes a tab and shows the one that takes its place, or the start screen after the last.
    fn close(&mut self, tab: usize) {
        self.tabs.remove(tab);
        let last = self.tabs.len().checked_sub(1);
        // The tabs after the closed one move one to the left.
        self.active = self.active.zip(last).map(|(active, last)| (active - usize::from(active > tab)).min(last));
    }

    /// Draws a frame.
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        for path in std::mem::take(&mut self.pending) {
            self.open(&ctx, &path);
        }
        self.tabs.iter_mut().for_each(|ws| ws.poll(&ctx));
        // A tab out of sight reloads when it is shown.
        if let Some(ws) = self.workspace_mut() {
            ws.reload_if_changed(&ctx);
        }
        let mut actions = Vec::new();
        if !self.tabs.is_empty() {
            egui::Panel::top("tabs").show(ui, |ui| tabs(ui, &self.tabs, self.active, &mut actions));
        }
        let focused = ui.input(|i| i.focused);
        match self.active {
            None => self.start.show(ui, &self.recent, &mut actions),
            Some(tab) => {
                if focused && !self.focused {
                    actions.push(Action::Refresh);
                }
                workspace(ui, &mut self.tabs[tab], &mut actions);
            }
        }
        self.focused = focused;
        for action in actions {
            match action {
                Action::OpenRepo(path) => self.open(&ctx, &path),
                Action::CloseTab(tab) => self.close(tab),
                Action::ShowTab(tab) => {
                    self.active = tab;
                    // It may have changed while it was out of sight.
                    if let Some(ws) = self.workspace_mut() {
                        ws.apply(&ctx, Action::Refresh);
                    }
                }
                action => {
                    if let Some(ws) = self.workspace_mut() {
                        ws.apply(&ctx, action);
                    }
                }
            }
        }
    }
}

/// Paths one per line, the way [`App::saved`] keeps them.
fn lines<'a>(paths: impl IntoIterator<Item = &'a Path>) -> String {
    paths.into_iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join("\n")
}

/// The tab bar: a tab per repository, each with a close button, and a new tab.
fn tabs(ui: &mut Ui, tabs: &[Workspace], active: Option<usize>, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        for (tab, ws) in tabs.iter().enumerate() {
            let name = ws.repo.name();
            let path = ws.repo.git.dir().display().to_string();
            if ui.add(Button::selectable(active == Some(tab), &name)).on_hover_text(path).clicked() {
                actions.push(Action::ShowTab(Some(tab)));
            }
            let close = ui.add(Button::new("×").small().frame(false)).on_hover_text("Close");
            close.widget_info(|| WidgetInfo::labeled(Role::Button, true, format!("Close {name}")));
            if close.clicked() {
                actions.push(Action::CloseTab(tab));
            }
            ui.separator();
        }
        let new = ui.add(Button::selectable(active.is_none(), "+")).on_hover_text("New tab");
        new.widget_info(|| WidgetInfo::labeled(Role::Button, true, "New tab"));
        if new.clicked() {
            actions.push(Action::ShowTab(None));
        }
    });
}

/// Asks for a folder.
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

fn workspace(ui: &mut Ui, ws: &mut Workspace, actions: &mut Vec<Action>) {
    // Dialogs come first, so that Escape closes them and nothing behind them.
    prompt(ui.ctx(), ws, actions);
    confirm(ui.ctx(), ws, actions);
    egui::Panel::top("toolbar").show(ui, |ui| toolbar(ui, ws, actions));
    if ws.snapshot.underway.is_some() {
        egui::Panel::top("underway").show(ui, |ui| underway(ui, ws, actions));
    }
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
    egui::CentralPanel::default().show(ui, |ui| {
        if let Some(merge) = &mut ws.merge {
            merge::show(ui, merge, actions);
        } else if let Some(file) = &ws.open_file {
            diff::show(ui, file, actions);
        } else {
            graph::show(ui, ws, actions);
        }
    });
}

fn toolbar(ui: &mut Ui, ws: &Workspace, actions: &mut Vec<Action>) {
    let status = &ws.snapshot.status;
    ui.horizontal(|ui| {
        ui.strong(status.branch.as_deref().unwrap_or("detached HEAD"));
        ui.separator();
        let idle = ws.busy().is_none();
        for (verb, step) in [("Undo", &ws.snapshot.undo.last), ("Redo", &ws.snapshot.undo.undone)] {
            let button = ui.add_enabled(idle && step.is_some(), Button::new(verb));
            if let Some(step) = step
                && button.on_hover_text(format!("{verb} {}", step.what)).clicked()
            {
                actions.push(Action::Run(step.op.clone()));
            }
        }
        let count = |label: &str, n: u32| if n > 0 { format!("{label}  {n}") } else { label.to_owned() };
        let remote = ws.snapshot.history.labels.values().flatten().find(|l| l.kind == RefKind::Remote);
        let remote = remote.and_then(|l| l.name.split_once('/')).map_or("origin", |(remote, _)| remote);
        let at = match ws.selection {
            Some(Selection::Commit(id)) => id.to_string(),
            _ => "HEAD".to_owned(),
        };
        let push = match (&status.branch, &status.upstream) {
            // A branch that went its own way since the last push only gets there by force.
            (Some(branch), Some(upstream)) if status.ahead > 0 && status.behind > 0 => {
                let lost = theme::count(status.behind as usize, "commit");
                let question =
                    format!("Force push {branch}, throwing away the {lost} on {upstream} that it does not have?");
                Action::Confirm(Confirm { question, verb: "Force push", op: Op::Push { upstream: None, force: true } })
            }
            _ => Action::Run(Op::Push { upstream: status.upstream.is_none().then(|| remote.to_owned()), force: false }),
        };
        let buttons = [
            ("Fetch", true, Action::Run(Op::Fetch)),
            (&count("Pull", status.behind), true, Action::Run(Op::Pull { rebase: false })),
            (&count("Push", status.ahead), status.branch.is_some(), push),
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

/// What stopped half way, the conflicts left, and the ways on from there.
fn underway(ui: &mut Ui, ws: &Workspace, actions: &mut Vec<Action>) {
    let Some(underway) = &ws.snapshot.underway else { return };
    let entries = &ws.snapshot.status.entries;
    let conflicts = entries.iter().filter(|e| matches!(e.unstaged, Some(Change::Conflicted { .. }))).count();
    let (command, idle) = (underway.command, ws.busy().is_none());
    ui.horizontal(|ui| {
        let (first, rest) = command.split_at(1);
        ui.strong(format!("{}{rest} in progress", first.to_uppercase()));
        ui.weak(&underway.message);
        if conflicts > 0 {
            ui.colored_label(theme::ERROR, theme::count(conflicts, "conflicted file"));
        }
        let go_on = ui.add_enabled(idle && conflicts == 0, Button::new("Continue"));
        if go_on.on_disabled_hover_text("Resolve every conflict first").clicked() {
            actions.push(Action::Run(Op::Continue(command)));
        }
        // A merge has no commit of its own to leave out.
        let skip = (command != "merge").then_some(("Skip", Op::Skip(command)));
        for (verb, op) in skip.into_iter().chain([("Abort", Op::Abort(command))]) {
            if ui.add_enabled(idle, Button::new(verb)).clicked() {
                actions.push(Action::Run(op));
            }
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

fn confirm(ctx: &egui::Context, ws: &mut Workspace, actions: &mut Vec<Action>) {
    let Some(confirm) = &ws.confirm else { return };
    let mut done = false;
    let response = egui::Modal::new(egui::Id::unique("confirm")).show(ctx, |ui| {
        ui.strong(&confirm.question);
        ui.label("This cannot be undone.");
        ui.horizontal(|ui| {
            if ui.button(egui::RichText::new(confirm.verb).color(theme::ERROR)).clicked() {
                actions.push(Action::Run(confirm.op.clone()));
                done = true;
            }
            done |= ui.button("Cancel").clicked();
        });
    });
    if done || response.should_close() {
        ws.confirm = None;
    }
}
