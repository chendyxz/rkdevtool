use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use tauri::{AppHandle, Manager, State};

use crate::state::AppState;

fn adb_path(app: &AppHandle) -> Result<PathBuf, String> {
    let path = if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/platform-tools/macos-arm64/adb")
    } else {
        app.path()
            .resource_dir()
            .map_err(|e| e.to_string())?
            .join("resources/platform-tools/macos-arm64/adb")
    };
    path.is_file()
        .then_some(path)
        .ok_or_else(|| "Bundled ADB is missing from the application resources".to_string())
}

fn parse_adb_devices(output: &str) -> Vec<String> {
    output
        .lines()
        .skip_while(|line| !line.starts_with("List of devices attached"))
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?;
            (fields.next() == Some("device")).then(|| serial.to_string())
        })
        .collect()
}

fn adb_devices_sync(adb: PathBuf) -> Result<Vec<String>, String> {
    let output = Command::new(adb)
        .arg("devices")
        .output()
        .map_err(|e| format!("Failed to run adb: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(parse_adb_devices(&String::from_utf8_lossy(&output.stdout)))
}

#[tauri::command]
pub async fn list_adb_devices(app: AppHandle) -> Result<Vec<String>, String> {
    let adb = adb_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || adb_devices_sync(adb))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn reboot_to_loader(
    app: AppHandle,
    state: State<'_, AppState>,
    serial: String,
) -> Result<(), String> {
    let adb = adb_path(&app)?;
    let output = tauri::async_runtime::spawn_blocking(move || {
        Command::new(adb)
            .args(["-s", &serial, "reboot", "loader"])
            .output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("Failed to run adb: {e}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let devices = crate::devices::resync_devices(&app, state.inner()).await?;
        if devices
            .iter()
            .any(|device| device.mode.eq_ignore_ascii_case("loader"))
        {
            return Ok(());
        }
    }

    Err("Timed out waiting for the RockUSB Loader device".to_string())
}

#[cfg(test)]
mod tests {
    use super::parse_adb_devices;

    #[test]
    fn keeps_only_ready_devices() {
        let output = "List of devices attached\nABC\tdevice product:x\nDEF\tunauthorized\n\n";
        assert_eq!(parse_adb_devices(output), vec!["ABC"]);
    }
}
