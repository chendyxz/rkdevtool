use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use tauri::{AppHandle, State};

use crate::state::AppState;

fn adb_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("adb"))
            .find(|path| path.is_file())
    }) {
        return Ok(path);
    }

    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin/adb"),
        PathBuf::from("/usr/local/bin/adb"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join("Library/Android/sdk/platform-tools/adb"));
    }
    for name in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(root) = std::env::var_os(name) {
            candidates.push(PathBuf::from(root).join("platform-tools/adb"));
        }
    }

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "ADB not found. Install Android Platform Tools or add adb to PATH".to_string()
        })
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

fn adb_devices_sync() -> Result<Vec<String>, String> {
    let output = Command::new(adb_path()?)
        .arg("devices")
        .output()
        .map_err(|e| format!("Failed to run adb: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(parse_adb_devices(&String::from_utf8_lossy(&output.stdout)))
}

#[tauri::command]
pub async fn list_adb_devices() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(adb_devices_sync)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn reboot_to_loader(
    app: AppHandle,
    state: State<'_, AppState>,
    serial: String,
) -> Result<(), String> {
    let adb = adb_path()?;
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
