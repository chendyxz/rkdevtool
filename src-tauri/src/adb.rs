use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tauri::{AppHandle, Emitter, State};

use crate::platform::{adb_command, adb_path};
use crate::state::AppState;
use crate::upgrade_tool::LogPayload;

const EVENT_TOOL_LOG: &str = "tool-log";
const REMOTE_APK_PATH: &str = "/data/local/tmp/rkdevtool-install.apk";

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

fn adb_control_args(action: &str) -> Option<&'static [&'static str]> {
    match action {
        "volumeUp" => Some(&["shell", "input", "keyevent", "24"]),
        "volumeDown" => Some(&["shell", "input", "keyevent", "25"]),
        "mute" => Some(&["shell", "input", "keyevent", "164"]),
        "back" => Some(&["shell", "input", "keyevent", "4"]),
        "home" => Some(&["shell", "input", "keyevent", "3"]),
        "recents" => Some(&["shell", "input", "keyevent", "187"]),
        "up" => Some(&["shell", "input", "keyevent", "19"]),
        "down" => Some(&["shell", "input", "keyevent", "20"]),
        "left" => Some(&["shell", "input", "keyevent", "21"]),
        "right" => Some(&["shell", "input", "keyevent", "22"]),
        "enter" => Some(&["shell", "input", "keyevent", "66"]),
        "power" => Some(&["shell", "input", "keyevent", "26"]),
        "reboot" => Some(&["reboot"]),
        "settings" => Some(&["shell", "am", "start", "-a", "android.settings.SETTINGS"]),
        _ => None,
    }
}

fn foreground_package(output: &str) -> Option<String> {
    output
        .lines()
        .filter(|line| {
            line.contains("mResumedActivity")
                || line.contains("topResumedActivity")
                || line.contains("ResumedActivity")
        })
        .flat_map(str::split_whitespace)
        .find_map(|token| {
            let package = token
                .split_once('/')?
                .0
                .trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '.' && ch != '_');
            (!package.is_empty()
                && package.contains('.')
                && package
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_'))
            .then(|| package.to_string())
        })
}

fn run_adb(adb: &PathBuf, serial: &str, args: &[&str]) -> Result<String, String> {
    let output = adb_command(adb)
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

/// Fails loudly with the command that broke plus everything the device printed before -
/// a bare stderr line such as `error: closed` is impossible to diagnose from the log panel.
fn command_failure(serial: &str, args: &[&str], error: &str, log: &[String]) -> String {
    let mut message = format!("adb -s {serial} {}", args.join(" "));
    if !error.is_empty() {
        message.push('\n');
        message.push_str(error);
    }
    if !log.is_empty() {
        message.push_str("\n\nPrevious steps:\n");
        message.push_str(&log.join("\n"));
    }
    message
}

fn run_adb_step(
    log: &mut Vec<String>,
    adb: &PathBuf,
    serial: &str,
    args: &[&str],
) -> Result<(), String> {
    match run_adb(adb, serial, args) {
        Ok(output) => {
            if !output.is_empty() {
                log.push(output);
            }
            Ok(())
        }
        Err(error) => Err(command_failure(serial, args, &error, log)),
    }
}

/// True for transport errors that appear while adbd is still restarting, so the caller can
/// safely retry instead of giving up.
fn is_transient_adb_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    [
        "device offline",
        "device not found",
        "device unauthorized",
        "no devices/emulators found",
        "closed",
        "connection reset",
        "protocol fault",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

/// Step variant for commands issued right after `reboot`/`root`: adbd needs a moment to
/// come back, so transient transport errors are retried for up to ~30 s.
fn run_adb_step_retrying(
    log: &mut Vec<String>,
    adb: &PathBuf,
    serial: &str,
    args: &[&str],
) -> Result<(), String> {
    const ATTEMPTS: u32 = 30;
    let mut last_error = String::new();
    for attempt in 0..ATTEMPTS {
        match run_adb(adb, serial, args) {
            Ok(output) => {
                if !output.is_empty() {
                    log.push(output);
                }
                return Ok(());
            }
            Err(error) => {
                if !is_transient_adb_error(&error) {
                    return Err(command_failure(serial, args, &error, log));
                }
                last_error = error;
            }
        }
        if attempt + 1 < ATTEMPTS {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    Err(command_failure(serial, args, &last_error, log))
}

/// Best-effort step: the device output is kept for the log, but a failure does not abort
/// the flow because some devices legitimately reject the command.
fn run_adb_optional(log: &mut Vec<String>, adb: &PathBuf, serial: &str, args: &[&str]) -> String {
    match run_adb(adb, serial, args) {
        Ok(output) => {
            if !output.is_empty() {
                log.push(output.clone());
            }
            output
        }
        Err(error) => error,
    }
}

/// `adb disable-verity` only takes effect after a restart and says so in its output; user
/// builds and devices without AVB reject the command instead.
fn verity_needs_reboot(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    if lower.contains("already disabled") || lower.contains("only works for userdebug") {
        return false;
    }
    lower.contains("now reboot") || lower.contains("verity disabled")
}

/// `adb reboot` returns before the device leaves the bus, so an immediate `wait-for-device`
/// can still be answered by the dying session. Poll a real shell command until adbd is back.
fn wait_for_reboot(adb: &PathBuf, serial: &str) -> Result<(), String> {
    const ATTEMPTS: u32 = 90;
    std::thread::sleep(Duration::from_secs(3));
    let mut last_error = "the device shell is not ready yet".to_string();
    for attempt in 0..ATTEMPTS {
        match run_adb(adb, serial, &["shell", "echo", "ready"]) {
            Ok(output) if output.contains("ready") => return Ok(()),
            Ok(_) => {}
            Err(error) => last_error = error,
        }
        if attempt + 1 < ATTEMPTS {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    Err(format!(
        "Timed out waiting for device {serial} to restart: {last_error}"
    ))
}

fn emit_log(app: &AppHandle, text: String, level: &str, in_place: bool) {
    let _ = app.emit(
        EVENT_TOOL_LOG,
        LogPayload {
            text,
            level: level.to_string(),
            in_place,
        },
    );
}

fn valid_apk_path(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("apk"))
}

fn upload_apk(app: &AppHandle, adb: &PathBuf, serial: &str, apk_path: &Path) -> Result<(), String> {
    let mut apk = File::open(apk_path).map_err(|e| format!("Open APK failed: {e}"))?;
    let total = apk
        .metadata()
        .map_err(|e| format!("Read APK metadata failed: {e}"))?
        .len();
    if total == 0 {
        return Err("APK file is empty".to_string());
    }

    let mut child = adb_command(adb)
        .args(["-s", serial, "shell", &format!("cat > {REMOTE_APK_PATH}")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Start APK upload failed: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("Open ADB input failed")?;
    let mut buffer = vec![0_u8; 256 * 1024];
    let mut uploaded = 0_u64;
    let mut last_percent = u64::MAX;
    emit_log(app, "Push APK... (0%)".to_string(), "default", true);

    loop {
        let count = apk
            .read(&mut buffer)
            .map_err(|e| format!("Read APK failed: {e}"))?;
        if count == 0 {
            break;
        }
        stdin
            .write_all(&buffer[..count])
            .map_err(|e| format!("Upload APK failed: {e}"))?;
        uploaded += count as u64;
        let percent = uploaded.saturating_mul(100) / total;
        if percent != last_percent {
            emit_log(app, format!("Push APK... ({percent}%)"), "default", true);
            last_percent = percent;
        }
    }
    drop(stdin);

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Wait for APK upload failed: {e}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if error.is_empty() {
            "APK upload failed".to_string()
        } else {
            error
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn install_apk(app: AppHandle, serial: String, path: String) -> Result<String, String> {
    let apk_path = PathBuf::from(path);
    if !valid_apk_path(&apk_path) {
        return Err("Select a valid APK file".to_string());
    }
    let adb = adb_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = upload_apk(&app, &adb, &serial, &apk_path) {
            let _ = run_adb(&adb, &serial, &["shell", "rm", "-f", REMOTE_APK_PATH]);
            return Err(error);
        }
        emit_log(&app, "Installing APK...".to_string(), "default", false);

        let install_result = run_adb(
            &adb,
            &serial,
            &["shell", "pm", "install", "-r", REMOTE_APK_PATH],
        );
        let _ = run_adb(&adb, &serial, &["shell", "rm", "-f", REMOTE_APK_PATH]);

        let output = install_result?;
        if !output.lines().any(|line| line.trim() == "Success") {
            return Err(if output.is_empty() {
                "APK installation failed".to_string()
            } else {
                output
            });
        }
        Ok(output)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn run_adb_control(
    app: AppHandle,
    serial: String,
    action: String,
) -> Result<String, String> {
    if action == "restartApp" {
        let adb = adb_path(&app)?;
        return tauri::async_runtime::spawn_blocking(move || {
            let activities = run_adb(
                &adb,
                &serial,
                &["shell", "dumpsys", "activity", "activities"],
            )?;
            let package = foreground_package(&activities)
                .ok_or_else(|| "Unable to identify the current foreground app".to_string())?;
            run_adb(&adb, &serial, &["shell", "am", "force-stop", &package])?;
            Ok(package)
        })
        .await
        .map_err(|e| e.to_string())?;
    }

    let args = adb_control_args(&action).ok_or_else(|| "Unsupported ADB control".to_string())?;
    let adb = adb_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || run_adb(&adb, &serial, args))
        .await
        .map_err(|e| e.to_string())?
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
        run_adb_step(&mut log, &adb, &serial, &["root"])?;
        run_adb_step(&mut log, &adb, &serial, &["wait-for-device"])?;

        // Android 10+ keeps /system read-only behind dm-verity, and `remount` fails until
        // verity is disabled - which needs one extra restart. Best effort: user builds and
        // devices without AVB reject the command, and `remount` below still decides.
        let verity = run_adb_optional(&mut log, &adb, &serial, &["disable-verity"]);
        if verity_needs_reboot(&verity) {
            emit_log(
                &app,
                "Restarting the device once to apply the writable /system remount...".to_string(),
                "default",
                false,
            );
            run_adb_step(&mut log, &adb, &serial, &["reboot"])?;
            wait_for_reboot(&adb, &serial)?;
            run_adb_step_retrying(&mut log, &adb, &serial, &["root"])?;
            run_adb_step_retrying(&mut log, &adb, &serial, &["wait-for-device"])?;
        }

        run_adb_step(&mut log, &adb, &serial, &["remount"])?;
        let tool_path = tool.to_string_lossy();
        run_adb_step(
            &mut log,
            &adb,
            &serial,
            &["push", &tool_path, "/system/bin/read_write_id"],
        )?;
        run_adb_step(
            &mut log,
            &adb,
            &serial,
            &["shell", "chmod", "0777", "/system/bin/read_write_id"],
        )?;

        for parameter in parameters {
            let slot = parameter_slot(&parameter.kind).expect("parameter kind was validated");
            run_adb_step(
                &mut log,
                &adb,
                &serial,
                &[
                    "shell",
                    "read_write_id",
                    "write",
                    slot,
                    parameter.value.trim(),
                ],
            )?;
        }

        // `adb sync` is AOSP's host-side "push build output" command and aborts with
        // "product directory not specified; set $ANDROID_PRODUCT_OUT". Flush the
        // device write cache with the device-side `sync` instead.
        run_adb_step(&mut log, &adb, &serial, &["shell", "sync"])?;
        run_adb_step(&mut log, &adb, &serial, &["reboot"])?;
        Ok(log.join("\n"))
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

fn normalize_adb_address(address: &str) -> Result<String, String> {
    let address = address.trim();
    if address.is_empty()
        || address.len() > 255
        || !address
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | ':' | '[' | ']'))
    {
        return Err("Enter a valid device IP address".to_string());
    }
    if address.contains(':') {
        Ok(address.to_string())
    } else {
        Ok(format!("{address}:5555"))
    }
}

fn adb_devices_sync(adb: PathBuf) -> Result<Vec<String>, String> {
    let output = adb_command(&adb)
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
pub async fn connect_adb_device(app: AppHandle, address: String) -> Result<String, String> {
    let address = normalize_adb_address(&address)?;
    let adb = adb_path(&app)?;
    let target = address.clone();
    let output = tauri::async_runtime::spawn_blocking(move || {
        adb_command(&adb)
            .args(["connect", &target])
            .output()
            .map_err(|e| format!("Failed to run adb: {e}"))
    })
    .await
    .map_err(|e| e.to_string())??;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let message = [stdout, stderr]
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let lower = message.to_ascii_lowercase();
    if output.status.success()
        && (lower.contains("connected to") || lower.contains("already connected"))
    {
        Ok(address)
    } else if message.is_empty() {
        Err("Failed to connect to the wireless ADB device".to_string())
    } else {
        Err(message)
    }
}

#[tauri::command]
pub async fn reboot_to_loader(
    app: AppHandle,
    state: State<'_, AppState>,
    serial: String,
) -> Result<(), String> {
    let adb = adb_path(&app)?;
    let output = tauri::async_runtime::spawn_blocking(move || {
        adb_command(&adb)
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
    use super::{
        adb_control_args, command_failure, foreground_package, is_transient_adb_error,
        normalize_adb_address, parameter_slot, parse_adb_devices, valid_apk_path,
        verity_needs_reboot,
    };
    use std::path::Path;

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

    #[test]
    fn detects_when_disable_verity_requires_a_restart() {
        assert!(verity_needs_reboot(
            "Verity disabled on /system\nNow reboot your device for settings to take effect"
        ));
        assert!(!verity_needs_reboot("Verity already disabled on /system"));
        assert!(!verity_needs_reboot(
            "error: disable-verity only works for userdebug builds"
        ));
        assert!(!verity_needs_reboot(""));
    }

    #[test]
    fn only_retries_transport_errors() {
        assert!(is_transient_adb_error("error: device offline"));
        assert!(is_transient_adb_error("error: closed"));
        assert!(!is_transient_adb_error("remount failed"));
    }

    #[test]
    fn failure_message_keeps_the_failed_command_and_previous_output() {
        let log = vec!["adbd cannot run as root".to_string()];
        let message = command_failure("ABC", &["remount"], "remount failed", &log);
        assert!(message.starts_with("adb -s ABC remount"));
        assert!(message.contains("remount failed"));
        assert!(message.contains("adbd cannot run as root"));
    }

    #[test]
    fn rejects_non_apk_and_missing_install_files() {
        assert!(!valid_apk_path(Path::new("missing.apk")));
        assert!(!valid_apk_path(Path::new("package.zip")));

        let path = std::env::temp_dir().join(format!("rkdevtool-{}.apk", std::process::id()));
        std::fs::write(&path, b"apk").unwrap();
        assert!(valid_apk_path(&path));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn maps_only_supported_adb_controls() {
        assert_eq!(
            adb_control_args("volumeUp"),
            Some(&["shell", "input", "keyevent", "24"][..])
        );
        assert_eq!(adb_control_args("reboot"), Some(&["reboot"][..]));
        assert!(adb_control_args("arbitrary-command").is_none());
    }

    #[test]
    fn normalizes_wireless_adb_addresses() {
        assert_eq!(
            normalize_adb_address("192.168.1.10").unwrap(),
            "192.168.1.10:5555"
        );
        assert_eq!(
            normalize_adb_address("device.local:4321").unwrap(),
            "device.local:4321"
        );
        assert!(normalize_adb_address("192.168.1.10; reboot").is_err());
    }

    #[test]
    fn extracts_foreground_application_package() {
        let modern =
            "topResumedActivity=ActivityRecord{abc u0 com.example.player/.MainActivity t42}";
        let legacy = "mResumedActivity: ActivityRecord{def u0 style.f.app/MainActivity t7}";
        assert_eq!(
            foreground_package(modern).as_deref(),
            Some("com.example.player")
        );
        assert_eq!(foreground_package(legacy).as_deref(), Some("style.f.app"));
        assert_eq!(foreground_package("no resumed activity"), None);
    }
}
