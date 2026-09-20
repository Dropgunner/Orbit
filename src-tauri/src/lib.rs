mod agent;
mod commands;
mod events;
mod model;
mod state;
mod tools;
mod types;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::config::get_settings,
            commands::config::set_settings,
            commands::config::resolve_settings,
            commands::fs::list_dir,
            commands::fs::read_file,
            commands::fs::write_file,
            commands::fs::path_exists,
            commands::fs::search,
            commands::terminal::spawn_terminal,
            commands::terminal::write_terminal,
            commands::terminal::resize_terminal,
            commands::terminal::kill_terminal,
            commands::git::git_status,
            commands::git::git_diff,
            commands::git::git_stage,
            commands::git::git_unstage,
            commands::git::git_commit,
            commands::git::git_log,
            commands::git::git_current_branch,
            commands::agent::chat,
            commands::agent::stop_chat,
            commands::agent::list_pending_edits,
            commands::agent::apply_pending_edit,
            commands::agent::discard_pending_edit,
            commands::agent::apply_all_pending_edits
        ])
        .run(tauri::generate_context!())
        .expect("error while running Orbit");
}
