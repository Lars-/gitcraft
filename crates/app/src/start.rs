//! The start screen: open, clone or create a repository.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui::{self, Button, Key, ProgressBar, RichText, TextEdit, Ui};
use gitcraft_core::git::{self, Git};

use crate::app::Action;
use crate::theme;
use crate::ui::pick_folder;

/// The width of the dialog's fields.
const WIDTH: f32 = 380.0;

/// What the start screen keeps between frames.
#[derive(Default)]
pub struct Start {
    /// Why the last repository did not open.
    pub error: Option<String>,
    /// Cloning or creating a repository, while the dialog is open.
    pub dialog: Option<Dialog>,
    path: String,
    logo: Option<egui::TextureHandle>,
}

impl Start {
    /// Starts afresh once a repository is open, for the next new tab.
    pub fn clear(&mut self) {
        self.error = None;
        self.path.clear();
    }

    /// Draws the start screen.
    pub fn show(&mut self, ui: &mut Ui, recent: &[PathBuf], actions: &mut Vec<Action>) {
        let logo = self
            .logo
            .get_or_insert_with(|| {
                let icon = crate::logo();
                let size = [icon.width, icon.height].map(|n| usize::try_from(n).expect("a small image"));
                let image = egui::ColorImage::from_rgba_unmultiplied(size, &icon.rgba);
                ui.ctx().load_texture("logo", image, egui::TextureOptions::LINEAR)
            })
            .id();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 6.0);
                ui.image((logo, egui::Vec2::splat(112.0)));
                ui.heading(RichText::new("GitCraft").size(32.0).strong());
                ui.weak("A fast git client");
                ui.add_space(24.0);
                let size = egui::vec2(220.0, 32.0);
                if ui.add(Button::new("Open a repository…").min_size(size)).clicked()
                    && let Some(folder) = pick_folder("Open a repository")
                {
                    actions.push(Action::OpenRepo(folder));
                }
                ui.add_space(8.0);
                let path = &mut self.path;
                let field =
                    ui.add(TextEdit::singleline(path).hint_text("or type a path and press Enter").desired_width(320.0));
                if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !path.trim().is_empty() {
                    actions.push(Action::OpenRepo(PathBuf::from(path.trim())));
                }
                ui.add_space(8.0);
                for (label, clone) in [("Clone a repository…", true), ("Create a repository…", false)] {
                    if ui.add(Button::new(label).min_size(size)).clicked() {
                        self.dialog = Some(Dialog::new(clone, recent));
                    }
                }
                if let Some(error) = &self.error {
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
        if let Some(dialog) = &mut self.dialog
            && !dialog.show(ui.ctx(), actions)
        {
            self.dialog = None;
        }
    }
}

/// Cloning a repository, or creating an empty one.
#[derive(Debug, Default)]
pub struct Dialog {
    /// Clone [`Self::source`], or else create an empty repository.
    clone: bool,
    /// The URL to clone, or the name of the new repository.
    source: String,
    /// The folder the repository goes in.
    location: String,
    /// Why the last attempt failed.
    pub error: Option<String>,
    job: Option<Job>,
}

/// Git at work in the background.
#[derive(Debug)]
struct Job {
    /// The latest line of git's progress.
    line: String,
    events: Receiver<Event>,
}

#[derive(Debug)]
enum Event {
    Progress(String),
    Done(gitcraft_core::Result<Git>),
}

impl Dialog {
    /// Starts in the folder of the last repository.
    fn new(clone: bool, recent: &[PathBuf]) -> Self {
        let location = recent.first().and_then(|repo| repo.parent());
        Self { clone, location: location.map(|l| l.display().to_string()).unwrap_or_default(), ..Self::default() }
    }

    /// Whether git is at work.
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// Where the repository goes, once both fields are filled in.
    fn target(&self) -> Option<PathBuf> {
        let name = if self.clone { git::clone_name(&self.source) } else { self.source.trim() };
        let location = self.location.trim();
        (!name.is_empty() && !location.is_empty()).then(|| Path::new(location).join(name))
    }

    fn run(&mut self, ctx: &egui::Context, target: PathBuf) {
        let (send, events) = mpsc::channel();
        let (url, ctx) = (self.clone.then(|| self.source.trim().to_owned()), ctx.clone());
        std::thread::spawn(move || {
            // Sending fails only when the window was closed in the meantime.
            let report = |event| {
                let _ = send.send(event);
                ctx.request_repaint();
            };
            let done = match url {
                Some(url) => git::clone(&url, &target, |line| report(Event::Progress(line.to_owned()))),
                None => git::init(&target),
            };
            report(Event::Done(done));
        });
        self.error = None;
        self.job = Some(Job { line: "Starting…".to_owned(), events });
    }

    /// Takes in what git reported; returns the new repository once it is there.
    fn poll(&mut self) -> Option<Git> {
        let job = self.job.as_mut()?;
        let done = loop {
            match job.events.try_recv() {
                Ok(Event::Progress(line)) => job.line = line,
                Ok(Event::Done(done)) => break done.map_err(|error| error.to_string()),
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => break Err("The background job stopped unexpectedly".to_owned()),
            }
        };
        self.job = None;
        self.error = done.as_ref().err().cloned();
        done.ok()
    }

    /// Draws the dialog; returns whether it stays open.
    fn show(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) -> bool {
        if let Some(git) = self.poll() {
            actions.push(Action::OpenRepo(git.dir().to_owned()));
            return false;
        }
        let (verb, what) = if self.clone { ("Clone", "URL") } else { ("Create", "Name") };
        let mut open = true;
        let response = egui::Modal::new(egui::Id::unique("new repository")).show(ctx, |ui| {
            ui.strong(format!("{verb} a repository"));
            let label = ui.label(what);
            let field = ui.add(TextEdit::singleline(&mut self.source).desired_width(WIDTH)).labelled_by(label.id);
            // Ready to type the URL or name.
            if ui.memory(|m| m.focused().is_none()) {
                field.request_focus();
            }
            let label = ui.label("Folder");
            ui.horizontal(|ui| {
                ui.add(TextEdit::singleline(&mut self.location).desired_width(WIDTH - 80.0)).labelled_by(label.id);
                if ui.button("Browse…").clicked()
                    && let Some(folder) = pick_folder("Where to put the repository")
                {
                    self.location = folder.display().to_string();
                }
            });
            let target = self.target();
            match (&self.job, &target) {
                (Some(job), _) => {
                    let fraction = percent(&job.line);
                    // Up to the first comma fits: `Receiving objects:  45% (450/1000), 1.20 MiB | …`
                    let text = job.line.split(", ").next().unwrap_or_default();
                    let bar = ProgressBar::new(fraction.unwrap_or(0.0)).desired_width(WIDTH);
                    ui.add(bar.text(text).animate(fraction.is_none()));
                }
                (None, Some(target)) => {
                    ui.weak(format!("{} {}", if self.clone { "Clones into" } else { "Creates" }, target.display()));
                }
                (None, None) => {}
            }
            if let Some(error) = &self.error {
                ui.colored_label(theme::ERROR, error);
            }
            let idle = !self.busy();
            let enter = ui.input(|i| i.key_pressed(Key::Enter));
            ui.horizontal(|ui| {
                let go = ui.add_enabled(idle && target.is_some(), Button::new(verb)).clicked();
                if let Some(target) = target.filter(|_| idle && (go || enter)) {
                    self.run(ui.ctx(), target);
                }
                open &= !ui.add_enabled(idle, Button::new("Cancel")).clicked();
            });
        });
        self.busy() || (open && !response.should_close())
    }
}

/// How far a line of git's progress is, like `Receiving objects:  45% (450/1000)`.
pub fn percent(line: &str) -> Option<f32> {
    let (before, _) = line.split_once('%')?;
    before.rsplit(' ').next()?.parse::<f32>().ok().map(|percent| percent / 100.0)
}
