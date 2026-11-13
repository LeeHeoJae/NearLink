#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ble;
mod config;
mod model;
mod protocol;
mod ui;

fn main() {
    if let Err(error) = ui::run() {
        ui::fatal(&format!("NearLink를 시작하지 못했습니다.\n{error}"));
    }
}
