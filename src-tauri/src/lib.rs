mod adb;
pub mod apk_update;
mod device_ops;
mod devices;
pub mod firmware;
mod logcat;
mod state;
mod upgrade_tool;

use adb::{
    burn_parameters, connect_adb_device, install_apk, list_adb_devices, reboot_to_loader,
    run_adb_control,
};
use apk_update::update_firmware_apk;
use device_ops::{download_boot, get_current_storage, read_chip_info, upgrade_firmware};
use firmware::{extract_firmware_file, parse_firmware_info, FirmwareInfo};
use logcat::{clear_logcat, export_logcat, start_logcat, stop_logcat};
use state::AppState;
use upgrade_tool::{
    download_execute, get_tool_info, is_tool_busy, list_devices, partition_list, run_action,
    select_device,
};

#[tauri::command]
fn parse_firmware(path: String) -> Result<FirmwareInfo, String> {
    parse_firmware_info(&path)
}

#[tauri::command]
async fn extract_firmware(path: String, output_dir: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || extract_firmware_file(&path, &output_dir))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .manage(AppState::default())
        .setup(|app| {
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
            devices::start_hotplug_watcher(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_tool_info,
            list_devices,
            select_device,
            partition_list,
            upgrade_firmware,
            download_boot,
            download_execute,
            parse_firmware,
            extract_firmware,
            read_chip_info,
            get_current_storage,
            run_action,
            is_tool_busy,
            list_adb_devices,
            connect_adb_device,
            reboot_to_loader,
            install_apk,
            run_adb_control,
            burn_parameters,
            start_logcat,
            stop_logcat,
            clear_logcat,
            export_logcat,
            update_firmware_apk,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
