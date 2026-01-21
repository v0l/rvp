use eframe::NativeOptions;
use egui::{CentralPanel, Id, TextEdit, ViewportBuilder, Widget};
use rvp::{DefaultOverlay, Player};
use std::time::Duration;

fn main() {
    env_logger::init();
    let mut opt = NativeOptions::default();
    opt.viewport = ViewportBuilder::default().with_inner_size([1270.0, 740.0]);

    let _ = eframe::run_native("app", opt, Box::new(|_| Ok(Box::new(App::default()))));
}

struct App {
    player: Option<Player>,
    media_path: String,
    did_load: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            media_path: "".to_string(),
            player: None,
            did_load: false,
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let path_id = Id::new("media_path");
        CentralPanel::default().show(ctx, |ui| {
            if !self.did_load {
                self.did_load = true;
                let persisted = ui.data_mut(|d| d.get_persisted::<String>(path_id));
                if let Some(path) = persisted {
                    self.media_path = path.clone();
                }
            }
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!self.media_path.is_empty(), |ui| {
                    if ui.button("load").clicked() {
                        if let Ok(mut p) = Player::new(ctx, &self.media_path.replace("\"", "")) {
                            p.enable_keybinds(true);
                            self.player = Some(p.with_overlay(DefaultOverlay));
                            ui.data_mut(|d| d.insert_persisted(path_id, self.media_path.clone()));
                        }
                    }
                });
                ui.add_enabled_ui(!self.media_path.is_empty(), |ui| {
                    if ui.button("clear").clicked() {
                        self.player = None;
                    }
                });

                ui.add_sized(
                    [ui.available_width(), ui.available_height()],
                    TextEdit::singleline(&mut self.media_path).hint_text("click to set path"),
                );
            });
            ui.separator();
            if let Some(player) = self.player.as_mut() {
                player.ui(ui);
            }
        });
    }

    fn auto_save_interval(&self) -> Duration {
        Duration::from_secs(1)
    }
}
