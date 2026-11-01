// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // « tamga convert fichier.deb --to rpm » : mode texte, sans ouvrir de fenêtre.
    if let Some(code) = tamga_lib::run_cli_if_requested() {
        std::process::exit(code);
    }
    tamga_lib::run();
}
