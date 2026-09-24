//! OTA 升级包打包：把 payload（APK 或 ROM OTA 包）与 `version.json` 打成一个
//! zip。条目必须使用存储模式（等价于 `zip -0`），设备端按偏移直接读取，
//! 因此这里不能使用默认的 Deflate 压缩。

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use tauri::{AppHandle, Emitter, State};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::device_ops::format_byte_count;
use crate::devices;
use crate::state::AppState;
use crate::upgrade_tool::LogPayload;

const EVENT_TOOL_LOG: &str = "tool-log";
const VERSION_ENTRY_NAME: &str = "version.json";
/// 与 apk_update 保持一致的拷贝块大小，减少 GB 级 payload 的读写次数。
const COPY_BUFFER: usize = 16 * 1024 * 1024;
/// 进度日志按 5% 一档上报，避免刷屏。
const PROGRESS_STEP: u64 = 5;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OtaZipRequest {
    /// `apk` 或 `rom`。
    kind: String,
    /// APK OTA 为 apkVersion，ROM OTA 为 romVersion。
    version: String,
    pk: String,
    /// APK 文件路径，或 ROM OTA zip 路径。
    source: String,
    output: String,
}

fn emit_log(app: &AppHandle, text: &str, level: &str) {
    let _ = app.emit(
        EVENT_TOOL_LOG,
        LogPayload {
            text: text.to_string(),
            level: level.to_string(),
            in_place: false,
        },
    );
}

fn emit_progress(app: &AppHandle, text: String) {
    let _ = app.emit(
        EVENT_TOOL_LOG,
        LogPayload {
            text,
            level: "default".to_string(),
            in_place: true,
        },
    );
}

fn validate_field(name: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{name} 不能为空"));
    }
    if value.contains(['"', '\\']) {
        return Err(format!("{name} 不能包含引号或反斜杠"));
    }
    Ok(())
}

/// 与设备端已校验通过的包保持完全一致的字节格式：`"pk":` 后有一个空格。
fn version_json(kind: &str, version: &str, pk: &str) -> Result<String, String> {
    let key = match kind {
        "apk" => "apkVersion",
        "rom" => "romVersion",
        other => return Err(format!("不支持的 OTA 类型：{other}")),
    };
    Ok(format!("{{\"{key}\":\"{version}\",\"pk\": \"{pk}\"}}"))
}

fn expected_extension(kind: &str) -> Result<&'static str, String> {
    match kind {
        "apk" => Ok("apk"),
        "rom" => Ok("zip"),
        other => Err(format!("不支持的 OTA 类型：{other}")),
    }
}

fn validate_source(kind: &str, source: &Path) -> Result<PathBuf, String> {
    let expected = expected_extension(kind)?;
    if !source.is_file() {
        return Err(format!("文件不存在：{}", source.display()));
    }
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if !extension.eq_ignore_ascii_case(expected) {
        return Err(format!("请选择 .{expected} 文件：{}", source.display()));
    }
    Ok(source.to_path_buf())
}

/// 版本号里带小数点（如 `apk-ota-1.1.7`），不能用 `with_extension` 直接替换。
fn ensure_zip_extension(path: &Path) -> PathBuf {
    let has_zip = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("zip"));
    if has_zip {
        return path.to_path_buf();
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(".zip");
    PathBuf::from(name)
}

fn validate_output(source: &Path, output: &Path) -> Result<PathBuf, String> {
    let output = ensure_zip_extension(output);
    let canonical_source = source.canonicalize().map_err(|e| e.to_string())?;
    if output
        .canonicalize()
        .is_ok_and(|canonical| canonical == canonical_source)
    {
        return Err("输出文件不能与源文件相同".into());
    }
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("创建输出目录失败：{e}"))?;
        }
    }
    Ok(output)
}

fn entry_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .ok_or_else(|| format!("无法解析文件名：{}", path.display()))
}

fn part_path(output: &Path) -> PathBuf {
    let mut name = output.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}

fn write_entries(
    source: &Path,
    output: &Path,
    version: &str,
    progress: &mut impl FnMut(String),
) -> Result<(), String> {
    let total = fs::metadata(source).map_err(|e| e.to_string())?.len();
    let part = part_path(output);

    let result = (|| -> Result<(), String> {
        let file = File::create(&part).map_err(|e| format!("创建输出文件失败：{e}"))?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .unix_permissions(0o644);

        zip.start_file(VERSION_ENTRY_NAME, options)
            .map_err(|e| format!("写入 {VERSION_ENTRY_NAME} 失败：{e}"))?;
        zip.write_all(version.as_bytes())
            .map_err(|e| format!("写入 {VERSION_ENTRY_NAME} 失败：{e}"))?;

        zip.start_file(entry_name(source)?, options)
            .map_err(|e| format!("写入 payload 失败：{e}"))?;

        let mut reader = File::open(source).map_err(|e| e.to_string())?;
        let mut buffer = vec![0u8; COPY_BUFFER];
        let mut written = 0u64;
        let mut bucket = 0u64;
        loop {
            let read = reader.read(&mut buffer).map_err(|e| e.to_string())?;
            if read == 0 {
                break;
            }
            zip.write_all(&buffer[..read])
                .map_err(|e| format!("写入 payload 失败：{e}"))?;
            written += read as u64;
            let percent = written
                .saturating_mul(100)
                .checked_div(total)
                .unwrap_or(100);
            if percent / PROGRESS_STEP != bucket {
                bucket = percent / PROGRESS_STEP;
                progress(progress_text(percent, written, total));
            }
        }

        // 循环里可能已经报过 100%，避免重复刷一行。
        if bucket != 100 / PROGRESS_STEP {
            progress(progress_text(100, written, total));
        }

        let mut file = zip.finish().map_err(|e| format!("写入 zip 失败：{e}"))?;
        file.flush().map_err(|e| e.to_string())?;
        Ok(())
    })();

    if let Err(error) = result {
        let _ = fs::remove_file(&part);
        return Err(error);
    }

    fs::rename(&part, output).map_err(|e| {
        let _ = fs::remove_file(&part);
        format!("保存 zip 失败：{e}")
    })
}

fn progress_text(percent: u64, written: u64, total: u64) -> String {
    format!(
        "正在打包 OTA：{percent}%（{} / {}）",
        format_byte_count(written),
        format_byte_count(total)
    )
}

/// 打包入口（不依赖 `AppHandle`，供命令行示例与测试使用）。
/// 返回实际写入的输出路径。
pub fn pack_ota_zip(
    kind: &str,
    version: &str,
    pk: &str,
    source: &Path,
    output: &Path,
    mut progress: impl FnMut(String),
) -> Result<String, String> {
    validate_field("版本号", version)?;
    validate_field("PK", pk)?;

    let source = validate_source(kind, source)?;
    let version = version_json(kind, version, pk)?;
    let output = validate_output(&source, output)?;

    write_entries(&source, &output, &version, &mut progress)?;

    Ok(output.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn build_ota_zip(
    app: AppHandle,
    state: State<'_, AppState>,
    request: OtaZipRequest,
) -> Result<String, String> {
    devices::ensure_backend_not_busy(state.inner())?;
    devices::set_backend_busy(state.inner(), true)?;
    let worker_app = app.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        let source = Path::new(request.source.trim());
        emit_log(
            &worker_app,
            &format!(
                "正在打包 {}，payload：{}",
                request.kind.to_ascii_uppercase(),
                source
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("")
            ),
            "info",
        );
        pack_ota_zip(
            &request.kind,
            request.version.trim(),
            request.pk.trim(),
            source,
            Path::new(request.output.trim()),
            |text| emit_progress(&worker_app, text),
        )
    })
    .await;
    let _ = devices::set_backend_busy(state.inner(), false);

    match worker.map_err(|e| e.to_string())? {
        Ok(path) => {
            emit_log(&app, &format!("OTA 包已生成：{path}"), "success");
            Ok(path)
        }
        Err(error) => {
            emit_log(&app, &format!("生成 OTA 包失败：{error}"), "error");
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APK_NAME: &str = "VLGV_ALL_V1.1.8_20260924_094241.apk";
    const ROM_NAME: &str = "MC-GSS-32_1.1.8_1080x1920_20260924_V1.1.8-ota.zip";

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rkdevtool-ota-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry_text(archive: &mut zip::ZipArchive<File>, name: &str) -> String {
        let mut entry = archive.by_name(name).unwrap();
        assert_eq!(entry.compression(), CompressionMethod::Stored);
        let mut text = String::new();
        entry.read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn apk_pack_matches_zip_zero_layout() {
        let dir = temp_dir("apk");
        let source = dir.join(APK_NAME);
        fs::write(&source, vec![7u8; 5000]).unwrap();
        let output = dir.join("apk-ota-1.1.7.zip");

        let mut progress = Vec::new();
        let written =
            pack_ota_zip("apk", "1.1.7", "aVtOGr4aXmg", &source, &output, |text| {
                progress.push(text)
            })
            .unwrap();

        assert_eq!(written, output.to_string_lossy());
        assert!(progress.last().unwrap().contains("100%"));
        assert!(!dir.join("apk-ota-1.1.7.zip.part").exists());

        let mut archive = zip::ZipArchive::new(File::open(&output).unwrap()).unwrap();
        assert_eq!(archive.len(), 2);
        assert_eq!(
            entry_text(&mut archive, VERSION_ENTRY_NAME),
            r#"{"apkVersion":"1.1.7","pk": "aVtOGr4aXmg"}"#
        );
        assert_eq!(entry_text(&mut archive, APK_NAME).len(), 5000);
        let names: Vec<String> = archive.file_names().map(str::to_string).collect();
        assert_eq!(names, vec![VERSION_ENTRY_NAME, APK_NAME]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rom_pack_uses_rom_version_key() {
        let dir = temp_dir("rom");
        let source = dir.join(ROM_NAME);
        fs::write(&source, b"rom payload").unwrap();
        let output = dir.join("rom-ota-1.1.8");

        let written =
            pack_ota_zip("rom", "1.1.8", "aVBOMFbCsbS", &source, &output, |_| {}).unwrap();

        // 允许省略 .zip 后缀，自动补齐。
        let output = dir.join("rom-ota-1.1.8.zip");
        assert_eq!(written, output.to_string_lossy());

        let mut archive = zip::ZipArchive::new(File::open(&output).unwrap()).unwrap();
        assert_eq!(
            entry_text(&mut archive, VERSION_ENTRY_NAME),
            r#"{"romVersion":"1.1.8","pk": "aVBOMFbCsbS"}"#
        );
        assert_eq!(entry_text(&mut archive, ROM_NAME), "rom payload");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_invalid_input_without_leaving_files() {
        let dir = temp_dir("invalid");
        let source = dir.join("payload.apk");
        fs::write(&source, b"payload").unwrap();
        let output = dir.join("a.zip");

        // 类型与后缀不匹配
        assert!(pack_ota_zip("rom", "1.1.8", "pk", &source, &output, |_| {}).is_err());
        // 空版本号 / 含引号的 PK
        assert!(pack_ota_zip("apk", "", "pk", &source, &output, |_| {}).is_err());
        assert!(pack_ota_zip("apk", "1.1.7", "a\"b", &source, &output, |_| {}).is_err());
        // 模板不存在
        let missing = dir.join("missing.apk");
        assert!(pack_ota_zip("apk", "1.1.7", "pk", &missing, &output, |_| {}).is_err());

        // 输出不能覆盖源文件（ROM 的 payload 与输出同为 .zip）
        let rom = dir.join("rom-ota.zip");
        fs::write(&rom, b"payload").unwrap();
        assert!(pack_ota_zip("rom", "1.1.8", "pk", &rom, &rom, |_| {}).is_err());
        assert_eq!(fs::read(&rom).unwrap(), b"payload");

        assert!(!output.exists());
        assert!(!dir.join("a.zip.part").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
