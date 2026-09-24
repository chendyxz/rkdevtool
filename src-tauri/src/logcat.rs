use std::io::{BufRead, BufReader, Read};
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::platform::{adb_command, adb_path};
use crate::state::AppState;

const EVENT_LOGCAT_LINES: &str = "logcat-lines";
const EVENT_LOGCAT_ERROR: &str = "logcat-error";
const EVENT_LOGCAT_STOPPED: &str = "logcat-stopped";
const MAX_STORED_LINES: usize = 200_000;

#[derive(Clone, Serialize)]
struct LogcatLinesPayload {
    lines: Vec<String>,
}

fn stop_child(state: &AppState) -> Result<(), String> {
    let mut slot = state.logcat_child.lock().map_err(|e| e.to_string())?;
    if let Some(mut child) = slot.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

#[tauri::command]
pub fn start_logcat(
    app: AppHandle,
    state: State<'_, AppState>,
    serial: String,
) -> Result<(), String> {
    stop_child(state.inner())?;
    let generation = {
        let mut value = state.logcat_generation.lock().map_err(|e| e.to_string())?;
        *value = value.wrapping_add(1);
        *value
    };
    state
        .logcat_lines
        .lock()
        .map_err(|e| e.to_string())?
        .clear();

    let mut child = adb_command(&adb_path(&app)?)
        .args(["-s", &serial, "logcat", "-v", "threadtime"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start logcat: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Failed to capture logcat output")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Failed to capture logcat errors")?;
    *state.logcat_child.lock().map_err(|e| e.to_string())? = Some(child);

    let (line_tx, line_rx) = mpsc::channel::<String>();
    let reader_app = app.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(mut lines) = reader_app.state::<AppState>().logcat_lines.lock() {
                if lines.len() >= MAX_STORED_LINES {
                    let remove = MAX_STORED_LINES / 10;
                    lines.drain(..remove);
                }
                lines.push(line.clone());
            }
            if line_tx.send(line).is_err() {
                break;
            }
        }
    });

    let output_app = app.clone();
    thread::spawn(move || {
        while let Ok(first) = line_rx.recv() {
            thread::sleep(Duration::from_millis(50));
            let mut batch = Vec::with_capacity(200);
            batch.push(first);
            batch.extend(line_rx.try_iter().take(199));
            let _ = output_app.emit(EVENT_LOGCAT_LINES, LogcatLinesPayload { lines: batch });
        }

        let is_current = output_app
            .state::<AppState>()
            .logcat_generation
            .lock()
            .is_ok_and(|value| *value == generation);
        if is_current {
            let _ = output_app.emit(EVENT_LOGCAT_STOPPED, ());
        }
    });

    thread::spawn(move || {
        let mut error = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut error);
        let error = error.trim();
        if !error.is_empty() {
            let _ = app.emit(EVENT_LOGCAT_ERROR, error.to_string());
        }
    });
    Ok(())
}

#[tauri::command]
pub fn stop_logcat(state: State<'_, AppState>) -> Result<(), String> {
    let mut generation = state.logcat_generation.lock().map_err(|e| e.to_string())?;
    *generation = generation.wrapping_add(1);
    drop(generation);
    stop_child(state.inner())
}

/// 应用退出前回收常驻的 logcat 子进程。
///
/// 不清理的话 adb 会变成孤儿进程继续向设备拉流，反复重启应用后
/// 系统里会累积一串 `adb logcat`，设备端也白白多跑几路日志。
pub fn shutdown(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Ok(mut generation) = state.logcat_generation.lock() {
        // 递增代号，让输出线程知道这是主动关闭，不再补发 logcat-stopped
        *generation = generation.wrapping_add(1);
    }
    let _ = stop_child(state.inner());
}

#[tauri::command]
pub fn clear_logcat(state: State<'_, AppState>) -> Result<(), String> {
    state
        .logcat_lines
        .lock()
        .map_err(|e| e.to_string())?
        .clear();
    Ok(())
}

#[tauri::command]
pub fn export_logcat(state: State<'_, AppState>, path: String) -> Result<usize, String> {
    let lines = state
        .logcat_lines
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let content = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };
    std::fs::write(&path, content).map_err(|e| format!("Export logcat failed: {e}"))?;
    Ok(lines.len())
}
