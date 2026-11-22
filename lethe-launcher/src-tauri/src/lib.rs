mod auth;
mod commands;
mod config;
mod downloader;
mod http;
mod launcher;
mod logger;
mod manifest;
mod news;
mod profiles;
mod steam;
mod updater;
mod verifier;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize logger
    logger::init_logger();

    // Create default config if missing, then initialise the profile layout
    // (and migrate a legacy root-level token into the default profile).
    config::create_default_config();
    if let Err(e) = profiles::init() {
        lethe_log!("Profile initialisation failed: {}", e);
    }

    // Log working directory for debugging
    if let Ok(cwd) = std::env::current_dir() {
        lethe_log!("Working directory: {}", cwd.display());
    }

    // Note: DisableAutoUpdate is now a frontend-controlled setting.
    // The UI always loads so the user can login, change settings, and launch.
    tauri::Builder::default()
        .setup(|app| {
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
            commands::start_sync,
            commands::launch,
            commands::get_auto_update_status,
            commands::start_oauth,
            commands::get_saved_auth,
            commands::logout,
            commands::check_for_updates,
            commands::download_update,
            commands::get_settings,
            commands::set_setting,
            commands::get_mods,
            commands::toggle_mod,
            commands::profile_list,
            commands::profile_get,
            commands::profile_create,
            commands::profile_update,
            commands::profile_delete,
            commands::profile_switch,
            commands::profile_test_connection,
            commands::server_url_test,
            commands::profile_login_status,
            commands::profile_addresses,
            commands::profile_apply_same_machine,
            commands::profile_plugin_files,
            commands::get_diagnostics,
            commands::get_news,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
