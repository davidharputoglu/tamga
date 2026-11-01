//! TAMGA — pont entre l'interface (HTML/JS) et le moteur Rust `tamga-core`.
//! TAMGA — bridge between the web UI and the `tamga-core` engine.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tamga_core::convert::{self, Analysis, ConvertOptions, ConvertResult, MetaOverride};
use tamga_core::deps::{self, DepItem, TargetDistro};
use tamga_core::distro::SystemInfo;
use tamga_core::elf::ElfInfo;
use tamga_core::model::{Format, Package};
use tauri::State;

/// Paquet actuellement ouvert dans l'interface.
struct Session {
    pkg: Package,
    elf: ElfInfo,
    source: PathBuf,
}

#[derive(Default)]
struct AppState {
    session: Mutex<Option<Arc<Session>>>,
    system: Mutex<Option<SystemInfo>>,
}

fn err_string(e: anyhow::Error) -> String {
    format!("{e:#}")
}

fn system(state: &AppState) -> SystemInfo {
    let mut s = state.system.lock().unwrap();
    if s.is_none() {
        *s = Some(SystemInfo::detect());
    }
    s.clone().unwrap()
}

fn current(state: &AppState) -> Result<Arc<Session>, String> {
    state.session.lock().unwrap().clone().ok_or_else(|| "err.no_package".to_string())
}

/// Informations sur la machine (distribution, famille, glibc…).
#[tauri::command]
async fn system_info(state: State<'_, AppState>) -> Result<SystemInfo, String> {
    Ok(system(&state))
}

/// Ouvre et analyse un paquet.
#[tauri::command]
async fn analyze(path: String, state: State<'_, AppState>) -> Result<Analysis, String> {
    // Libère l'ancien paquet (et son dossier temporaire) avant d'en ouvrir un autre.
    *state.session.lock().unwrap() = None;
    let p = PathBuf::from(&path);
    let (session, analysis) = tauri::async_runtime::spawn_blocking(move || -> Result<(Session, Analysis), String> {
        let pkg = convert::load(&p).map_err(err_string)?;
        let (analysis, elf) = convert::analyze(&pkg, &p);
        Ok((Session { pkg, elf, source: p }, analysis))
    })
    .await
    .map_err(|e| e.to_string())??;
    *state.session.lock().unwrap() = Some(Arc::new(session));
    Ok(analysis)
}

/// Résout les dépendances pour la cible choisie.
#[tauri::command]
async fn resolve_deps(format: Format, distro: TargetDistro, online: bool, state: State<'_, AppState>) -> Result<Vec<DepItem>, String> {
    let s = current(&state)?;
    let sys = system(&state);
    tauri::async_runtime::spawn_blocking(move || convert::resolve_deps(&s.pkg, &s.elf, format, distro, online, &sys))
        .await
        .map_err(|e| e.to_string())
}

/// Formats que cette machine sait installer directement.
#[tauri::command]
fn installable_formats(state: State<'_, AppState>) -> Vec<Format> {
    tamga_core::install::installable_formats(&system(&state))
}

/// Résultat d'une installation.
#[derive(serde::Serialize)]
struct InstallResult {
    code: i32,
    command: String,
}

/// Installe un paquet converti avec le gestionnaire du système (mot de passe demandé par pkexec).
/// Chaque ligne affichée par l'installation est envoyée à l'interface (« install-log »).
#[tauri::command]
async fn install_package(path: String, format: Format, app: tauri::AppHandle, state: State<'_, AppState>) -> Result<InstallResult, String> {
    use tauri::Emitter;
    let sys = system(&state);
    tauri::async_runtime::spawn_blocking(move || {
        let plan = tamga_core::install::plan(format, Path::new(&path), &sys, tamga_core::install::Elevation::Pkexec).map_err(err_string)?;
        let _ = app.emit("install-log", format!("$ {}", plan.display));
        let a2 = app.clone();
        let code = tamga_core::install::run(&plan, move |l| {
            let _ = a2.emit("install-log", l);
        })
        .map_err(err_string)?;
        Ok(InstallResult { code, command: plan.display })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Chemin de sortie conseillé : même dossier que le paquet d'origine.
#[tauri::command]
fn default_output(format: Format, meta: MetaOverride, compression: Option<String>, state: State<'_, AppState>) -> Result<String, String> {
    let s = current(&state)?;
    let mut m = s.pkg.meta.clone();
    if let Some(n) = meta.name.filter(|n| !n.trim().is_empty()) {
        m.name = n;
    }
    if let Some(v) = meta.version.filter(|v| !v.trim().is_empty()) {
        m.version = v;
    }
    if let Some(r) = meta.release {
        m.release = r;
    }
    let name = convert::default_file_name_with(&m, format, compression.as_deref());
    let dir = s.source.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    Ok(dir.join(name).to_string_lossy().to_string())
}

/// Lance la conversion.
#[tauri::command]
async fn convert_package(options: ConvertOptions, state: State<'_, AppState>) -> Result<ConvertResult, String> {
    let s = current(&state)?;
    let sys = system(&state);
    tauri::async_runtime::spawn_blocking(move || convert::convert(&s.pkg, &s.elf, &options, &sys).map_err(err_string))
        .await
        .map_err(|e| e.to_string())?
}

/// Vide le cache local des recherches en ligne.
#[tauri::command]
fn clear_cache() -> Result<(), String> {
    deps::clear_cache().map_err(|e| e.to_string())
}

/// Ferme l'application.
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .setup(|app| {
            // Test automatique de bout en bout (versions de développement seulement).
            #[cfg(debug_assertions)]
            if let Ok(script) = std::env::var("TAMGA_E2E_JS") {
                use tauri::Manager;
                if let Some(w) = app.get_webview_window("main") {
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        let _ = w.eval(&script);
                    });
                }
            }
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            system_info,
            analyze,
            resolve_deps,
            default_output,
            convert_package,
            installable_formats,
            install_package,
            clear_cache,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}

/// Mode texte : `tamga convert paquet.deb --to rpm` (sans fenêtre).
pub fn run_cli_if_requested() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first()?;
    if tamga_core::cli::is_cli_command(first) {
        Some(tamga_core::cli::run(&args))
    } else {
        None
    }
}
