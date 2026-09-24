use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde::Deserialize;
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BurnParameter {
    kind: String,
    value: String,
}

fn parameter_slot(kind: &str) -> Option<&'static str> {
    match kind {
        "voiceKey" => Some("7"),
        "dn" => Some("8"),
        "ds" => Some("9"),
        "pk" => Some("10"),
        _ => None,
    }
}

fn run_adb(adb: &PathBuf, serial: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(adb)
        .args(["-s", serial])
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run adb: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        Ok([stdout, stderr]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"))
    } else {
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

#[tauri::command]
pub async fn burn_parameters(
    app: AppHandle,
    serial: String,
    tool_path: String,
    parameters: Vec<BurnParameter>,
) -> Result<String, String> {
    if parameters.is_empty() {
        return Err("No parameters selected".to_string());
    }
    if parameters.iter().any(|item| item.value.trim().is_empty()) {
        return Err("Parameter values cannot be empty".to_string());
    }
    if parameters
        .iter()
        .any(|item| parameter_slot(&item.kind).is_none())
    {
        return Err("Unsupported burn parameter".to_string());
    }

    let adb = adb_path(&app)?;
    let tool = PathBuf::from(tool_path);
    if !tool.is_file() {
        return Err("The selected read_write_id file does not exist".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut log = Vec::new();
        log.push(run_adb(&adb, &serial, &["root"])?);
        log.push(run_adb(&adb, &serial, &["wait-for-device"])?);
        log.push(run_adb(&adb, &serial, &["remount"])?);
        let tool_path = tool.to_string_lossy();
        log.push(run_adb(
            &adb,
            &serial,
            &["push", &tool_path, "/system/bin/read_write_id"],
        )?);
        log.push(run_adb(
            &adb,
            &serial,
            &["shell", "chmod", "0777", "/system/bin/read_write_id"],
        )?);

        for parameter in parameters {
            let slot = parameter_slot(&parameter.kind).expect("parameter kind was validated");
            log.push(run_adb(
                &adb,
                &serial,
                &[
                    "shell",
                    "read_write_id",
                    "write",
                    slot,
                    parameter.value.trim(),
                ],
            )?);
        }

        log.push(run_adb(&adb, &serial, &["sync"])?);
        log.push(run_adb(&adb, &serial, &["reboot"])?);
        Ok(log
            .into_iter()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n"))
    })
    .await
    .map_err(|e| e.to_string())?
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
    use super::{parameter_slot, parse_adb_devices};

    #[test]
    fn keeps_only_ready_devices() {
        let output = "List of devices attached\nABC\tdevice product:x\nDEF\tunauthorized\n\n";
        assert_eq!(parse_adb_devices(output), vec!["ABC"]);
    }

    #[test]
    fn maps_burn_parameters_to_expected_slots() {
        assert_eq!(parameter_slot("voiceKey"), Some("7"));
        assert_eq!(parameter_slot("dn"), Some("8"));
        assert_eq!(parameter_slot("ds"), Some("9"));
        assert_eq!(parameter_slot("pk"), Some("10"));
        assert_eq!(parameter_slot("unknown"), None);
    }
}
