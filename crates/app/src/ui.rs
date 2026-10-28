//! The window: the start screen, or a repository with its toolbar and panels.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Button, Key, RichText, Ui};
use gitcraft_core::git::Op;
use gitcraft_core::repo::{RefKind, Repo};

use crate::app::{Action, Selection, Workspace};
use crate::{details, diff, graph, sidebar, theme};

const RECENT: usize = 10;

/// The whole app.
#[derive(Default)]
pub struct App {
    /// The open repository.
    pub workspace: Option<Workspace>,
    /// Recently opened repositories, newest first.
    pub recent: Vec<PathBuf>,
    path: String,
    error: Option<String>,
    focused: bool,
    pending: Option<PathBuf>,
    logo: Option<egui::TextureHandle>,
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
                self.error = None;
            }
            Err(error) => self.error = Some(format!("{}: {error}", path.display())),
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
            None => {
                let logo = self.logo.get_or_insert_with(|| {
                    let icon = crate::logo();
                    let size = [icon.width, icon.height].map(|n| usize::try_from(n).expect("a small image"));
                    let image = egui::ColorImage::from_rgba_unmultiplied(size, &icon.rgba);
                    ui.ctx().load_texture("logo", image, egui::TextureOptions::LINEAR)
                });
                start(ui, logo.id(), &mut self.path, &self.recent, self.error.as_deref(), &mut actions);
            }
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

fn pick_folder() -> Option<PathBuf> {
    rfd::FileDialog::new().set_title("Open a repository").pick_folder()
}

fn start(
    ui: &mut Ui,
    logo: egui::TextureId,
    path: &mut String,
    recent: &[PathBuf],
    error: Option<&str>,
    actions: &mut Vec<Action>,
) {
    egui::CentralPanel::default().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 6.0);
            ui.image((logo, egui::Vec2::splat(112.0)));
            ui.heading(RichText::new("GitCraft").size(32.0).strong());
            ui.weak("A fast git client");
            ui.add_space(24.0);
            if ui.add(Button::new("Open a repository…").min_size(egui::vec2(220.0, 32.0))).clicked()
                && let Some(folder) = pick_folder()
            {
                actions.push(Action::OpenRepo(folder));
            }
            ui.add_space(8.0);
            let field = ui
                .add(egui::TextEdit::singleline(path).hint_text("or type a path and press Enter").desired_width(320.0));
            if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !path.trim().is_empty() {
                actions.push(Action::OpenRepo(PathBuf::from(path.trim())));
            }
            if let Some(error) = error {
                ui.colored_label(theme::ERROR, error);
            }
            if !recent.is_empty() {
                ui.add_space(24.0);
                ui.weak("Recent");
                for repo in recent {
                    if ui.link(repo.display().to_string()).clicked() {
                        actions.push(Action::OpenRepo(repo.clone()));
                    }
                }
            }
        });
    });
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
            && let Some(folder) = pick_folder()
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
