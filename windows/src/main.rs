#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ble;
mod config;
mod model;
mod protocol;
mod theme;
mod tray;
mod ui;

fn main() {
    if let Err(error) = ui::run() {
        ui::fatal(&format!("NearLink를 시작하지 못했어요.\n{error}"));
    }
}
