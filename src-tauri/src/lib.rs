mod agent;
mod assets;
mod chain;
mod commands;
mod dce;
mod discord;
mod discord_api;
mod dsh;
mod events;
mod javtools;
mod model;
mod patterns;
mod profiler;
mod selfhost;
mod state;
mod tools;
mod types;
mod web;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            // Tauri installs its default menu on macOS, which already carries the
            // Edit roles (⌘C/⌘V/⌘X/⌘A) plus Services, View and Help. Setting a
            // custom menu replaces those, so leave it alone.
            // ORBIT_WEB starts the local web server on launch and prints its URL,
            // for headless use and for verifying the browser front-end.
            if std::env::var("ORBIT_WEB").is_ok() {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    match crate::web::start(&handle).await {
                        Ok(info) => println!("[orbit] local web UI: {}", info.url()),
                        Err(error) => eprintln!("[orbit] local web UI failed: {error}"),
                    }
                });
            }
            // ORBIT_DISCORD_WEB opens the in-app Discord Web browser at launch.
            if std::env::var("ORBIT_DISCORD_WEB").is_ok() {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    match crate::commands::browser::open_discord_web(handle).await {
                        Ok(()) => println!("[orbit] opened the in-app Discord Web browser"),
                        Err(error) => eprintln!("[orbit] Discord Web window failed: {error}"),
                    }
                });
            }
            Ok(())
        })
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
            commands::agent::workspace_mode,
            commands::agent::list_pending_edits,
            commands::agent::apply_pending_edit,
            commands::agent::discard_pending_edit,
            commands::agent::apply_all_pending_edits,
            commands::chain::start_chain,
            commands::chain::stop_chain,
            commands::chain::get_chain,
            commands::dsh::dsh_status,
            commands::dsh::dsh_run,
            commands::dsh::stop_dsh,
            commands::discord::import_discord,
            commands::discord::import_discord_text,
            commands::discord::discord_summary,
            commands::discord::discord_messages,
            commands::discord::analyze_profile,
            commands::discord::analyze_bot,
            commands::discord::analyze_guard,
            commands::discord::screen_message_cmd,
            commands::discord::guard_policies,
            commands::discord::list_patterns,
            commands::discord::flag_pattern,
            commands::discord::remove_pattern,
            commands::discord::match_patterns,
            commands::discord::list_flags,
            commands::discord::set_flag,
            commands::discord::clear_flag,
            commands::discord::discuss,
            commands::discord_live::discord_whoami,
            commands::discord_live::discord_guilds,
            commands::discord_live::discord_channels,
            commands::discord_live::discord_members,
            commands::discord_live::discord_fetch_channel,
            commands::discord_live::discord_dce_status,
            commands::discord_live::discord_dce_export,
            commands::discord_live::discord_export_json,
            commands::discord_live::discord_export_json_text,
            commands::discord::discuss_once,
            commands::javtools::classify_cmd,
            commands::javtools::score_cmd,
            commands::javtools::psych_profile_cmd,
            commands::javtools::noul_cmd,
            commands::javtools::prune_cmd,
            commands::javtools::compact_cmd,
            commands::javtools::generate_cmd,
            commands::web::start_web_ui,
            commands::web::stop_web_ui,
            commands::web::web_ui_url,
            commands::browser::open_discord_web,
            commands::browser::discord_web_open
        ])
        .run(tauri::generate_context!())
        .expect("error while running Orbit");
}
