#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ble;
mod config;
mod i18n;
mod model;
mod protocol;
mod theme;
mod tray;
mod ui;

fn main() {
    if let Err(error) = ui::run() {
        ui::fatal(&i18n::tf("Couldn't start NearLink.\n{0}", &[&error]));
    }
}
