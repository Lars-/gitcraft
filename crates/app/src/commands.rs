//! Commands: what the tabs, the toolbar and the selected commit offer, each by its name and maybe
//! with a shortcut, and the command palette that finds any of them by a few words of its name.

use eframe::egui::{self, Button, Key, KeyboardShortcut, Modifiers, Role, TextEdit, WidgetInfo};
use gitcraft_core::git::Op;

use crate::app::{Action, Confirm, Prompt, Selection, Workspace};
use crate::ui::App;
use crate::{diff, graph, settings, theme};

/// Opens the command palette.
pub const PALETTE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::P);

/// Opens the settings.
pub const SETTINGS: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Comma);

/// Commits, also while the message is being typed.
pub const COMMIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);

/// The shortcut of `key` with `modifiers`.
const fn keys(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(modifiers, key)
}

/// The keys that stage, unstage or discard what the diff in view shows.
pub fn staging(verb: &str) -> KeyboardShortcut {
    let key = match verb {
        "Stage" => Key::S,
        "Unstage" => Key::U,
        _ => Key::Backspace,
    };
    keys(Modifiers::COMMAND, key)
}

/// Something the user can do.
#[derive(Debug, Clone)]
pub struct Command {
    /// What it is called; a menu and its item are apart by ` › `.
    pub name: String,
    /// The keys that run it, if any.
    pub keys: Option<KeyboardShortcut>,
    /// What it does, or `None` while it cannot run.
    pub action: Option<Action>,
}

impl Command {
    /// A command by its name, with its keys if any.
    pub fn new(name: impl Into<String>, keys: impl Into<Option<KeyboardShortcut>>, action: Option<Action>) -> Self {
        Self { name: name.into(), keys: keys.into(), action }
    }

    /// Its name, with its shortcut if it has one.
    pub fn hint(&self, ctx: &egui::Context) -> String {
        match &self.keys {
            Some(keys) => format!("{}  ({})", self.name, ctx.format_shortcut(keys)),
            None => self.name.clone(),
        }
    }
}

/// The toolbar's buttons, left to right: each with what it shows, its command, and the commands of
/// its right-click menu; Pull rebases if the settings say so. While git is busy, what they run
/// waits its turn, apart from undo and redo, which depend on where it leaves `HEAD`.
pub fn toolbar(ctx: &egui::Context, ws: &Workspace) -> Vec<(String, Command, Vec<Command>)> {
    let (cmd, shift) = (Modifiers::COMMAND, Modifiers::COMMAND.plus(Modifiers::SHIFT));
    let status = &ws.snapshot.status;
    let idle = ws.busy().is_none();
    let mut tools = Vec::new();
    let (undo, redo) = (&ws.snapshot.undo.last, &ws.snapshot.undo.undone);
    for (verb, step, shortcut) in [("Undo", undo, keys(cmd, Key::Z)), ("Redo", redo, keys(shift, Key::Z))] {
        let name = step.as_ref().map_or_else(|| verb.to_owned(), |step| format!("{verb} {}", step.what));
        let action = step.as_ref().filter(|_| idle).map(|step| Action::Run(step.op.clone()));
        tools.push((verb.to_owned(), Command::new(name, shortcut, action), Vec::new()));
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
    let pull = Action::Run(Op::Pull { rebase: settings::REBASE.on(ctx) });
    let pull_menu = [("Pull with merge", false), ("Pull with rebase", true)]
        .map(|(name, rebase)| Command::new(name, None, Some(Action::Run(Op::Pull { rebase }))));
    let mut push_menu = Vec::new();
    for remote in remotes.iter().map(|remote| &remote.name) {
        let push = |force| Op::Push { remote: Some(remote.clone()), track, force };
        let question =
            format!("Force push {branch} to {remote}, throwing away what it has there that {branch} does not?");
        let force = Action::Confirm(Confirm { question, verb: "Force push", op: push(true) });
        push_menu.extend([
            Command::new(format!("Push to {remote}"), None, Some(Action::Run(push(false)))),
            Command::new(format!("Force push to {remote}…"), None, Some(force)),
            Command::new(format!("Push tags to {remote}"), None, Some(Action::Run(Op::PushTags(remote.clone())))),
        ]);
    }
    tools.extend([
        ("Fetch".to_owned(), Command::new("Fetch", keys(shift, Key::F), Some(Action::Run(Op::Fetch))), Vec::new()),
        (count("Pull", status.behind), Command::new("Pull", keys(shift, Key::ArrowDown), Some(pull)), pull_menu.into()),
        (
            count("Push", status.ahead),
            Command::new("Push", keys(shift, Key::ArrowUp), status.branch.is_some().then_some(push)),
            push_menu,
        ),
        (
            "Branch".to_owned(),
            Command::new("New branch…", keys(cmd, Key::B), Some(Action::Prompt(Prompt::Branch { at }))),
            Vec::new(),
        ),
        (
            "Stash".to_owned(),
            Command::new(
                "Stash",
                keys(shift, Key::S),
                (!status.is_clean()).then(|| Action::Run(Op::Stash(format!("WIP on {branch}")))),
            ),
            Vec::new(),
        ),
        (
            "Pop".to_owned(),
            Command::new("Pop stash", None, (!ws.snapshot.stashes.is_empty()).then_some(Action::Run(Op::StashPop(0)))),
            Vec::new(),
        ),
        (
            "Refresh".to_owned(),
            Command::new("Refresh", keys(Modifiers::NONE, Key::F5), Some(Action::Refresh)),
            Vec::new(),
        ),
    ]);
    tools
}

/// Every command there is now: of the tabs, the start screen and the settings, and of the
/// repository on screen with its toolbar, the file in view and the selected commit.
pub fn all(ctx: &egui::Context, app: &App) -> Vec<Command> {
    let cmd = Modifiers::COMMAND;
    let (ctrl, back) = (Modifiers::CTRL, Modifiers::CTRL.plus(Modifiers::SHIFT));
    // The start screen is the last tab, the one with the +.
    let tabs = app.tabs.len() + 1;
    let at = app.active.unwrap_or(tabs - 1);
    let after = |by: usize| {
        let to = (at + by) % tabs;
        Some(Action::ShowTab((to < tabs - 1).then_some(to)))
    };
    let mut commands = vec![
        Command::new("Open a repository…", keys(cmd, Key::O), Some(Action::PickRepo)),
        Command::new("New tab", keys(cmd, Key::T), Some(Action::ShowTab(None))),
        Command::new("Close tab", keys(cmd, Key::W), app.active.map(Action::CloseTab)),
        Command::new("Next tab", keys(ctrl, Key::Tab), after(1)),
        Command::new("Previous tab", keys(back, Key::Tab), after(tabs - 1)),
        Command::new("Clone a repository…", None, Some(Action::NewRepo { clone: true })),
        Command::new("Create a repository…", None, Some(Action::NewRepo { clone: false })),
        Command::new("Settings…", SETTINGS, Some(Action::Settings)),
    ];
    let Some(ws) = app.workspace() else { return commands };
    for (_, command, menu) in toolbar(ctx, ws) {
        commands.push(command);
        commands.extend(menu);
    }
    commands.push(Command::new("Commit", COMMIT, ws.commit().map(Action::Run)));
    commands.extend(ws.open_file.as_ref().map(diff::commands).unwrap_or_default());
    // The files of the details panel, one after the other; with none in view, from either end.
    // Not with Alt: some window managers, such as labwc, move the window with Alt and an arrow.
    let files = ws.listed();
    let at = ws.open_file.as_ref().and_then(|file| files.iter().position(|source| *source == file.source));
    let (next, previous) = (at.map_or(0, |at| at + 1), at.unwrap_or(files.len()).checked_sub(1));
    for (name, key, to) in [("Next file", Key::ArrowDown, Some(next)), ("Previous file", Key::ArrowUp, previous)] {
        let to = to.and_then(|to| files.get(to)).cloned();
        commands.push(Command::new(name, keys(cmd, key), to.map(Action::Open)));
    }
    let away = ws.open_file.is_some() || ws.merge.is_some() || ws.plan.is_some();
    let back = (away || ws.history.is_some() || ws.blame.is_some()).then_some(Action::CloseFile);
    commands.push(Command::new("Back to the graph", keys(Modifiers::NONE, Key::Escape), back));
    if let Some(row) = ws.selected_commit() {
        let history = &ws.snapshot.history;
        let (id, current) = (history.ids[row], ws.snapshot.status.branch.as_deref());
        let labels = history.labels.get(&row).into_iter().flatten();
        let items = labels.flat_map(|label| graph::ref_items(label, current)).chain(graph::commit_items(id, current));
        let commit = id.to_hex_with_len(7);
        commands
            .extend(items.map(|(name, action)| Command::new(format!("Commit {commit} › {name}"), None, Some(action))));
    }
    commands
}

/// The actions of the shortcuts pressed in this frame. None while a dialog is open; while a text
/// field has the keyboard only committing, since the field uses the other keys itself. Escape is
/// for the views: it leaves a text field, or else the view.
pub fn shortcuts(ctx: &egui::Context, commands: &[Command]) -> Vec<Action> {
    if ctx.memory(|m| m.top_modal_layer().is_some()) {
        return Vec::new();
    }
    let typing = ctx.text_edit_focused();
    let mut keyed: Vec<_> = commands.iter().filter_map(|command| Some((command.keys?, command))).collect();
    keyed.retain(|(keys, _)| keys.logical_key != Key::Escape && (!typing || *keys == COMMIT));
    // egui takes Ctrl+Shift+Z for Ctrl+Z too, so the one with more keys goes first.
    keyed.sort_by_key(|(keys, _)| std::cmp::Reverse(keys.modifiers.shift));
    // A shortcut of a command that cannot run now does nothing, rather than fall through.
    ctx.input_mut(|i| {
        keyed.into_iter().filter(|(keys, _)| i.consume_shortcut(keys)).filter_map(|(_, c)| c.action.clone()).collect()
    })
}

/// The command palette while it is open: what was typed, and which of the commands that match is
/// picked.
#[derive(Debug, Default)]
pub struct Palette {
    text: String,
    at: usize,
}

/// The commands whose names have every word of `text` in them.
fn matching<'a>(commands: &'a [Command], text: &str) -> Vec<&'a Command> {
    let text = text.to_lowercase();
    let words: Vec<_> = text.split_whitespace().collect();
    commands.iter().filter(|command| words.iter().all(|word| command.name.to_lowercase().contains(word))).collect()
}

/// Shows the command palette, if it is open: a field to type a few words of a command's name in,
/// and the commands that have them. Up and down pick one, Enter or a click runs it.
pub fn palette(ctx: &egui::Context, open: &mut Option<Palette>, commands: &[Command], actions: &mut Vec<Action>) {
    let Some(palette) = open else { return };
    let mut run = None;
    let id = egui::Id::unique("command palette");
    let area = egui::Modal::default_area(id).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0));
    let response = egui::Modal::new(id).area(area).show(ctx, |ui| {
        let [up, down, enter] =
            ui.input_mut(|i| [Key::ArrowUp, Key::ArrowDown, Key::Enter].map(|key| i.consume_key(Modifiers::NONE, key)));
        let field = TextEdit::singleline(&mut palette.text).hint_text("Type a command").desired_width(480.0);
        let field = ui.add(field).accessible_name("Command");
        field.request_focus();
        if field.changed() {
            palette.at = 0;
        }
        let found = matching(commands, &palette.text);
        let n = found.len().max(1);
        palette.at = (palette.at.min(n - 1) + n + usize::from(down) - usize::from(up)) % n;
        if found.is_empty() {
            ui.weak("No command has those words");
        }
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (i, command) in found.into_iter().enumerate() {
                let keys = command.keys.map(|keys| ui.ctx().format_shortcut(&keys)).unwrap_or_default();
                let button = Button::selectable(i == palette.at, &command.name).shortcut_text(keys);
                let (enabled, picked) = (command.action.is_some(), i == palette.at);
                let row = ui.add_enabled(enabled, button.min_size(egui::vec2(480.0, 0.0)));
                row.widget_info(|| WidgetInfo::selected(Role::ListBoxOption, enabled, picked, &command.name));
                if picked && (up || down) {
                    row.scroll_to_me(None);
                }
                if row.clicked() || enter && picked {
                    run.clone_from(&command.action);
                }
            }
        });
    });
    if run.is_some() || response.should_close() {
        *open = None;
    }
    actions.extend(run);
}
