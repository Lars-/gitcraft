//! The settings, which hold for every repository, and the window to change them in.
//!
//! They are kept in egui's memory, which eframe saves between runs, like the zoom egui keeps there
//! itself.

use eframe::egui::gui_zoom::{
    self,
    kb_shortcuts::{ZOOM_IN, ZOOM_OUT, ZOOM_RESET},
};
use eframe::egui::{self, Button, Id, KeyboardShortcut, ThemePreference, Ui};

/// A setting that is on or off.
#[derive(Debug, Clone, Copy)]
pub struct Switch(&'static str);

/// Diffs show the old and the new version side by side.
pub const SIDE_BY_SIDE: Switch = Switch("side by side");
/// Dates read as a date and a time, rather than as how long ago.
pub const FULL_DATES: Switch = Switch("full dates");
/// The graph, the commit, a file's history and blame show each author without an avatar.
pub const HIDE_AVATARS: Switch = Switch("hide avatars");
/// Pull rebases onto the upstream, rather than merging it.
pub const REBASE: Switch = Switch("pull with rebase");

impl Switch {
    /// Whether it is on.
    pub fn on(self, ctx: &egui::Context) -> bool {
        ctx.data_mut(|d| d.get_persisted(Id::unique(self.0))).unwrap_or(false)
    }

    /// Turns it on or off.
    pub fn set(self, ctx: &egui::Context, on: bool) {
        ctx.data_mut(|d| d.insert_persisted(Id::unique(self.0), on));
    }

    /// A choice between what it does off and on.
    fn choice(self, ui: &mut Ui, what: &str, [off, on]: [&str; 2]) {
        if let Some(value) = choice(ui, what, self.on(ui.ctx()), &[(false, off), (true, on)]) {
            self.set(ui.ctx(), value);
        }
    }
}

/// Shows the settings; `false` once they are closed.
pub fn show(ctx: &egui::Context) -> bool {
    let mut close = false;
    let response = egui::Modal::new(Id::unique("settings")).show(ctx, |ui| {
        ui.strong("Settings");
        ui.horizontal(|ui| {
            label(ui, "Zoom");
            zoom(ui, "−", "Zoom out", ZOOM_OUT, gui_zoom::zoom_out);
            ui.add_sized([44.0, 20.0], egui::Label::new(format!("{:.0}%", ctx.zoom_factor() * 100.0)));
            zoom(ui, "+", "Zoom in", ZOOM_IN, gui_zoom::zoom_in);
            zoom(ui, "Reset", "Reset the zoom", ZOOM_RESET, |ctx| ctx.set_zoom_factor(1.0));
        });
        let themes =
            [(ThemePreference::Dark, "Dark"), (ThemePreference::Light, "Light"), (ThemePreference::System, "System")];
        if let Some(theme) = choice(ui, "Theme", ctx.options(|o| o.theme_preference), &themes) {
            ctx.set_theme(theme);
        }
        FULL_DATES.choice(ui, "Dates", ["How long ago", "Date and time"]);
        HIDE_AVATARS.choice(ui, "Avatars", ["Initials", "None"]);
        REBASE.choice(ui, "Pull", ["Merge", "Rebase"]);
        close = ui.button("Close").clicked();
    });
    !(close || response.should_close())
}

/// A row to pick one of `picks` for what it is about, with `value` picked; the new pick, once the
/// user makes one.
fn choice<T: PartialEq + Copy>(ui: &mut Ui, what: &str, mut value: T, picks: &[(T, &str)]) -> Option<T> {
    ui.horizontal(|ui| {
        label(ui, what);
        let mut changed = false;
        for &(pick, name) in picks {
            changed |= ui.radio_value(&mut value, pick, name).changed();
        }
        changed.then_some(value)
    })
    .inner
}

/// What a row of the settings is about, in a column of its own.
fn label(ui: &mut Ui, text: &str) {
    let width = ui.label(text).rect.width();
    ui.add_space((48.0 - width).max(0.0));
}

/// A button that zooms, with egui's own keys for it in its hint: they also zoom outside the settings.
fn zoom(ui: &mut Ui, text: &str, name: &str, keys: KeyboardShortcut, zoom: impl Fn(&egui::Context)) {
    let button = ui.add(Button::new(text).min_size(egui::vec2(24.0, 0.0))).accessible_name(name);
    if button.on_hover_text(format!("{name}  ({})", ui.ctx().format_shortcut(&keys))).clicked() {
        zoom(ui.ctx());
    }
}
