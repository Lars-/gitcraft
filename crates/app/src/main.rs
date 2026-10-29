//! GitCraft: a fast graphical git client.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod details;
mod diff;
mod graph;
mod sidebar;
mod start;
#[cfg(test)]
mod tests;
mod theme;
mod ui;

use std::path::PathBuf;

use eframe::egui;

const RECENT: &str = "recent";

/// The logo, which is also the window icon.
const LOGO: &[u8] = include_bytes!("../../../assets/icon/256.png");

/// The logo, decoded.
fn logo() -> egui::IconData {
    eframe::icon_data::from_png_bytes(LOGO).expect("the logo is a PNG")
}

struct Window(ui::App);

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.show(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let recent: Vec<_> = self.0.recent.iter().map(|path| path.display().to_string()).collect();
        storage.set_string(RECENT, recent.join("\n"));
    }
}

fn main() -> eframe::Result {
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    let viewport = egui::ViewportBuilder::default()
        .with_title("GitCraft")
        .with_app_id("gitcraft")
        .with_inner_size([1360.0, 820.0])
        .with_min_inner_size([760.0, 420.0])
        .with_icon(logo());
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "GitCraft",
        options,
        Box::new(|cc| {
            let recent = cc.storage.and_then(|s| s.get_string(RECENT)).unwrap_or_default();
            let recent = recent.lines().map(PathBuf::from).collect();
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(Window(ui::App::new(recent, path))))
        }),
    )
}
