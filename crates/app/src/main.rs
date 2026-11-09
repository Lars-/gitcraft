//! GitCraft: a fast graphical git client.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod details;
mod diff;
mod graph;
mod merge;
mod rebase;
mod sidebar;
mod start;
#[cfg(test)]
mod tests;
mod theme;
mod ui;

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use eframe::egui;
use gitcraft_core::askpass::{self, Question};

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
        for (key, value) in ui::KEYS.into_iter().zip(self.0.saved()) {
            storage.set_string(key, value);
        }
    }
}

/// Has git ask the user through this window from now on, rather than give up.
fn ask_here(ctx: egui::Context) -> std::io::Result<Receiver<Question>> {
    let (address, questions) = askpass::listen(move || ctx.request_repaint())?;
    askpass::install(&std::env::current_exe()?, &address);
    Ok(questions)
}

fn main() -> eframe::Result {
    // git runs GitCraft itself to ask something, such as a password, and takes the line it
    // prints as the answer; any other exit code than 0 makes it give up.
    if let Some(address) = std::env::var_os(askpass::VAR) {
        let prompt = std::env::args_os().nth(1).unwrap_or_default();
        match askpass::ask(&address.to_string_lossy(), &prompt.to_string_lossy()) {
            Ok(Some(answer)) => println!("{answer}"),
            Ok(None) => std::process::exit(1),
            Err(error) => {
                eprintln!("GitCraft could not ask: {error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
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
            theme::apply(&cc.egui_ctx);
            let mut app = ui::App::restore(|key| cc.storage.and_then(|s| s.get_string(key)), path);
            match ask_here(cc.egui_ctx.clone()) {
                Ok(questions) => app.questions = Some(questions),
                Err(error) => app.start.error = Some(format!("git cannot ask for passwords: {error}")),
            }
            Ok(Box::new(Window(app)))
        }),
    )
}
