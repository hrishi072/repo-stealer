#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use repo_stealer_app::{Action, Engine, ItemPage, PlanPage, Status};
use std::sync::Arc;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
#[cfg(target_os = "linux")]
mod profile;

#[tauri::command]
fn get_status(engine: State<'_, Arc<Engine>>) -> Result<Status, String> {
    engine.status()
}
#[tauri::command]
fn get_items(
    engine: State<'_, Arc<Engine>>,
    query: String,
    offset: usize,
    limit: usize,
) -> Result<ItemPage, String> {
    engine.items(&query, offset, limit)
}
#[tauri::command]
fn get_plan_rows(
    engine: State<'_, Arc<Engine>>,
    query: String,
    offset: usize,
    limit: usize,
) -> Result<PlanPage, String> {
    engine.plan_rows(&query, offset, limit)
}
#[tauri::command]
fn start_job(engine: State<'_, Arc<Engine>>, action: Action) -> Result<u64, String> {
    engine.inner().start(action)
}
#[tauri::command]
fn lock_vault(engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    engine.lock()
}
#[tauri::command]
fn cancel_job(engine: State<'_, Arc<Engine>>) -> Result<(), String> {
    engine.cancel()
}

#[tauri::command]
async fn choose_path(app: tauri::AppHandle, purpose: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = app.dialog().file();
        let selected = match purpose.as_str() {
            "directory" => dialog.blocking_pick_folder(),
            "backup_open" => dialog
                .add_filter("Repo Stealer backup", &["rsbackup"])
                .blocking_pick_file(),
            "backup_save" => dialog
                .add_filter("Repo Stealer backup", &["rsbackup"])
                .set_file_name("repo-stealer.rsbackup")
                .blocking_save_file(),
            "key_open" => dialog.blocking_pick_file(),
            _ => return Err("Unsupported path dialog.".to_owned()),
        };
        selected
            .map(|file| {
                file.into_path()
                    .map_err(|_| "Choose a local filesystem path.".to_owned())
                    .and_then(|path| {
                        path.to_str()
                            .map(str::to_owned)
                            .ok_or_else(|| "This path is not valid UTF-8.".to_owned())
                    })
            })
            .transpose()
    })
    .await
    .map_err(|_| "The path dialog stopped unexpectedly.".to_owned())?
}

fn main() {
    #[allow(unused_mut)]
    let mut context = tauri::generate_context!();
    #[cfg(target_os = "linux")]
    let _profile = {
        let profile = profile::create().expect("cannot create private tmpfs browser storage");
        for window in &mut context.config_mut().app.windows {
            window.incognito = false;
            window.data_directory = Some(profile.path().to_owned());
        }
        profile // Retain the private directory until the event loop exits.
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(Engine::new()))
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                // A forced close has the same encrypted crash boundary as the
                // CLI. Request cancellation where supported before exit.
                let _ = window.state::<Arc<Engine>>().cancel();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_items,
            get_plan_rows,
            start_job,
            lock_vault,
            cancel_job,
            choose_path
        ])
        .run(context)
        .expect("cannot start Repo Stealer desktop runtime");
}
