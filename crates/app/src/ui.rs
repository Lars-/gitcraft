//! The window: the start screen, or a repository with its toolbar and panels.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use eframe::egui::{self, Button, Key, Role, TextEdit, Ui, WidgetInfo};
use gitcraft_core::askpass::Question;
use gitcraft_core::git::Op;
use gitcraft_core::repo::Repo;
use gitcraft_core::status::Change;

use crate::app::{Action, Confirm, Prompt, Selection, Workspace};
use crate::start::Start;
use crate::{details, diff, graph, merge, rebase, sidebar, theme};

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
    /// What git asks while it runs, such as a password.
    pub questions: Option<Receiver<Question>>,
    /// The question on screen, with the answer so far.
    question: Option<(Question, String)>,
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
        // A question comes first, whichever tab or clone it is for: git waits for it.
        if self.question.is_none() {
            self.question = self.questions.as_ref().and_then(|q| q.try_recv().ok()).map(|q| (q, String::new()));
        }
        question(&ctx, &mut self.question);
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
        } else if ws.plan.is_some() {
            rebase::show(ui, ws, actions);
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
        let at = match ws.selection {
            Some(Selection::Commit(id)) => id.to_string(),
            _ => "HEAD".to_owned(),
        };
        let remotes = &ws.snapshot.remotes;
        let (branch, track) = (status.branch.as_deref().unwrap_or("HEAD"), status.upstream.is_none());
        let push = match &status.upstream {
            // A branch that went its own way since the last push only gets there by force.
            Some(upstream) if status.ahead > 0 && status.behind > 0 => {
                let lost = theme::count(status.behind as usize, "commit");
                let question =
                    format!("Force push {branch}, throwing away the {lost} on {upstream} that it does not have?");
                let op = Op::Push { remote: None, track: false, force: true };
                Action::Confirm(Confirm { question, verb: "Force push", op })
            }
            _ => {
                // Like git, a branch without an upstream goes to the only remote, or else to origin.
                let remote = match &remotes[..] {
                    [only] => only.name.as_str(),
                    _ => "origin",
                };
                Action::Run(Op::Push { remote: track.then(|| remote.to_owned()), track, force: false })
            }
        };
        let pull_menu = [("Pull with merge", false), ("Pull with rebase", true)]
            .map(|(text, rebase)| (text.to_owned(), Action::Run(Op::Pull { rebase })));
        let mut push_menu = Vec::new();
        for remote in remotes.iter().map(|remote| &remote.name) {
            let push = |force| Op::Push { remote: Some(remote.clone()), track, force };
            let question =
                format!("Force push {branch} to {remote}, throwing away what it has there that {branch} does not?");
            push_menu.extend([
                (format!("Push to {remote}"), Action::Run(push(false))),
                (
                    format!("Force push to {remote}…"),
                    Action::Confirm(Confirm { question, verb: "Force push", op: push(true) }),
                ),
                (format!("Push tags to {remote}"), Action::Run(Op::PushTags(remote.clone()))),
            ]);
        }
        let buttons = [
            ("Fetch", true, Action::Run(Op::Fetch), Vec::new()),
            (&count("Pull", status.behind), true, Action::Run(Op::Pull { rebase: false }), pull_menu.into()),
            (&count("Push", status.ahead), status.branch.is_some(), push, push_menu),
            ("Branch", true, Action::Prompt(Prompt::Branch { at }), Vec::new()),
            ("Stash", !status.is_clean(), Action::Run(Op::Stash(format!("WIP on {branch}"))), Vec::new()),
            ("Pop", !ws.snapshot.stashes.is_empty(), Action::Run(Op::StashPop(0)), Vec::new()),
            ("Refresh", true, Action::Refresh, Vec::new()),
        ];
        for (label, enabled, action, menu) in buttons {
            let button = ui.add_enabled(idle && enabled, Button::new(label));
            if button.clicked() {
                actions.push(action);
            }
            if !menu.is_empty() {
                button.on_hover_text("Right-click for more").context_menu(|ui| {
                    for (text, action) in menu {
                        if ui.button(text).clicked() {
                            actions.push(action);
                            ui.close();
                        }
                    }
                });
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
        ui.strong(format!("{} in progress", theme::capital(command)));
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

/// Asks the user what git asks, and answers git.
fn question(ctx: &egui::Context, asked: &mut Option<(Question, String)>) {
    let Some((question, answer)) = asked else { return };
    let mut answered = None;
    let response = egui::Modal::new(egui::Id::unique("question")).show(ctx, |ui| {
        let label = ui.strong(question.prompt.trim());
        let field = TextEdit::singleline(answer).password(question.secret()).hint_text("answer").desired_width(380.0);
        ui.add(field).labelled_by(label.id).request_focus();
        // The Enter that answers is not for a dialog behind this one.
        let enter = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() || enter {
                answered = Some(true);
            }
            if ui.button("Cancel").clicked() {
                answered = Some(false);
            }
        });
    });
    if let Some(ok) = answered.or(response.should_close().then_some(false)) {
        let (question, answer) = asked.take().expect("a question is on screen");
        question.answer(ok.then_some(answer.as_str()));
    }
}

fn prompt(ctx: &egui::Context, ws: &mut Workspace, actions: &mut Vec<Action>) {
    let Some((prompt, fields)) = &mut ws.prompt else { return };
    let mut done = false;
    let response = egui::Modal::new(egui::Id::unique("prompt")).show(ctx, |ui| {
        let (what, verb) = match prompt {
            Prompt::Branch { .. } => ("New branch".to_owned(), "Create"),
            Prompt::Tag { .. } => ("New tag".to_owned(), "Create"),
            Prompt::Remote(None) => ("New remote".to_owned(), "Add"),
            Prompt::Remote(Some(remote)) => (format!("Edit remote {}", remote.name), "Save"),
        };
        ui.strong(what);
        let [name, url] = fields;
        let field = ui.add(TextEdit::singleline(name).hint_text("name").desired_width(380.0)).accessible_name("Name");
        // Ready to type the name.
        if ui.memory(|m| m.focused().is_none()) {
            field.request_focus();
        }
        // Only a remote has a URL.
        let remote = matches!(prompt, Prompt::Remote(_));
        if remote {
            ui.add(TextEdit::singleline(url).hint_text("URL").desired_width(380.0)).accessible_name("URL");
        }
        let enter = ui.input(|i| i.key_pressed(Key::Enter));
        let filled = !name.trim().is_empty() && (!remote || !url.trim().is_empty());
        ui.horizontal(|ui| {
            if (ui.button(verb).clicked() || enter) && filled {
                let ops = prompt.ops([name, url].map(|field| field.trim().to_owned()));
                actions.extend(ops.into_iter().map(Action::Run));
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
