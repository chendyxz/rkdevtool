use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, Manager, State};

use crate::devices;
use crate::state::AppState;
use crate::upgrade_tool::LogPayload;

const DEFAULT_APK_PATH: &str = "/system/app/lxzk/lxzk.apk";
const COPY_BUFFER: usize = 16 * 1024 * 1024;
const EVENT_TOOL_LOG: &str = "tool-log";

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

#[derive(Debug)]
struct FirmwareLayout {
    rkaf_offset: u64,
    rkaf_size: u64,
    super_offset: u64,
    super_size: u64,
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32, String> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or("Truncated firmware header")?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn c_string(data: &[u8]) -> &str {
    let end = data
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(data.len());
    std::str::from_utf8(&data[..end]).unwrap_or("")
}

fn parse_firmware_layout(path: &Path) -> Result<FirmwareLayout, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut outer = [0u8; 0x66];
    file.read_exact(&mut outer).map_err(|e| e.to_string())?;
    if &outer[..4] != b"RKFW" {
        return Err("Only RKFW firmware with an embedded RKAF image is supported".into());
    }
    let rkaf_offset = u64::from(u32_at(&outer, 0x21)?);
    let rkaf_size = u64::from(u32_at(&outer, 0x25)?);
    file.seek(SeekFrom::Start(rkaf_offset))
        .map_err(|e| e.to_string())?;
    let mut header = vec![0u8; 2048];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    if &header[..4] != b"RKAF" {
        return Err("Embedded update image is not RKAF".into());
    }
    let count = u32_at(&header, 136)? as usize;
    for index in 0..count.min(16) {
        let start = 140 + index * 112;
        let part = header
            .get(start..start + 112)
            .ok_or("Truncated RKAF table")?;
        if c_string(&part[..32]).eq_ignore_ascii_case("super") {
            return Ok(FirmwareLayout {
                rkaf_offset,
                rkaf_size,
                super_offset: rkaf_offset + u64::from(u32_at(part, 96)?),
                super_size: u64::from(u32_at(part, 108)?),
            });
        }
    }
    Err("Firmware contains no super partition".into())
}

fn copy_range(source: &Path, offset: u64, size: u64, output: &Path) -> Result<(), String> {
    let mut input = File::open(source).map_err(|e| e.to_string())?;
    input
        .seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut output = File::create(output).map_err(|e| e.to_string())?;
    std::io::copy(&mut input.take(size), &mut output).map_err(|e| e.to_string())?;
    Ok(())
}

fn tool_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if cfg!(debug_assertions) {
        return Ok(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/firmware-tools/macos-arm64")
        );
    }
    let archive = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("resources/firmware-tools/macos-arm64.tar.gz");
    let tools = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("firmware-tools-macos-arm64");
    if tools.join("debugfs").is_file() {
        return Ok(tools);
    }
    fs::create_dir_all(&tools).map_err(|e| e.to_string())?;
    let output = Command::new("/usr/bin/tar")
        .args(["-xzf"])
        .arg(&archive)
        .args(["-C"])
        .arg(&tools)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(tools)
}

fn run_tool(path: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new(path)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn system_extent(raw_super: &Path) -> Result<(u64, u64), String> {
    let mut file = File::open(raw_super).map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(4096))
        .map_err(|e| e.to_string())?;
    let mut geometry = [0u8; 52];
    file.read_exact(&mut geometry).map_err(|e| e.to_string())?;
    if u32_at(&geometry, 0)? != 0x616c4467 {
        return Err("Invalid dynamic partition geometry".into());
    }
    file.seek(SeekFrom::Start(12288))
        .map_err(|e| e.to_string())?;
    let mut header = [0u8; 132];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    if u32_at(&header, 0)? != 0x414c5030 {
        return Err("Invalid dynamic partition metadata".into());
    }
    let header_size = u64::from(u32_at(&header, 8)?);
    let part_offset = u64::from(u32_at(&header, 80)?);
    let part_count = u32_at(&header, 84)? as usize;
    let part_size = u64::from(u32_at(&header, 88)?);
    let extent_offset = u64::from(u32_at(&header, 92)?);
    let extent_size = u64::from(u32_at(&header, 100)?);
    let tables = 12288 + header_size;
    for index in 0..part_count {
        file.seek(SeekFrom::Start(
            tables + part_offset + index as u64 * part_size,
        ))
        .map_err(|e| e.to_string())?;
        let mut part = [0u8; 52];
        file.read_exact(&mut part).map_err(|e| e.to_string())?;
        if c_string(&part[..36]) != "system" {
            continue;
        }
        let first = u64::from(u32_at(&part, 40)?);
        if u32_at(&part, 44)? != 1 {
            return Err("Fragmented system partition is not supported".into());
        }
        file.seek(SeekFrom::Start(
            tables + extent_offset + first * extent_size,
        ))
        .map_err(|e| e.to_string())?;
        let mut extent = [0u8; 24];
        file.read_exact(&mut extent).map_err(|e| e.to_string())?;
        let sectors = u64::from_le_bytes(extent[..8].try_into().unwrap());
        let target = u64::from_le_bytes(extent[12..20].try_into().unwrap());
        return Ok((target * 512, sectors * 512));
    }
    Err("super.img contains no system logical partition".into())
}

fn rkcrc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for (index, value) in table.iter_mut().enumerate() {
        let mut crc = (index as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
        *value = crc;
    }
    table
}

fn rkcrc32(mut crc: u32, data: &[u8], table: &[u32; 256]) -> u32 {
    for &byte in data {
        let index = ((crc >> 24) ^ u32::from(byte)) as usize;
        crc = (crc << 8) ^ table[index];
    }
    crc
}

fn patch_package(
    source: &Path,
    replacement: &Path,
    output: &Path,
    layout: &FirmwareLayout,
) -> Result<(), String> {
    fs::copy(source, output).map_err(|e| e.to_string())?;
    let replacement_size = fs::metadata(replacement).map_err(|e| e.to_string())?.len();
    if replacement_size > layout.super_size {
        return Err("Modified super.img no longer fits the firmware package".into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(output)
        .map_err(|e| e.to_string())?;
    file.seek(SeekFrom::Start(layout.super_offset))
        .map_err(|e| e.to_string())?;
    let mut replacement = File::open(replacement).map_err(|e| e.to_string())?;
    std::io::copy(&mut replacement, &mut file).map_err(|e| e.to_string())?;
    let zeros = vec![0u8; 1024 * 1024];
    let mut remaining = layout.super_size - replacement_size;
    while remaining > 0 {
        let count = remaining.min(zeros.len() as u64) as usize;
        file.write_all(&zeros[..count]).map_err(|e| e.to_string())?;
        remaining -= count as u64;
    }
    file.seek(SeekFrom::Start(layout.rkaf_offset))
        .map_err(|e| e.to_string())?;
    let mut remaining = layout.rkaf_size - 4;
    let mut crc = 0;
    let crc_table = rkcrc32_table();
    let mut buffer = vec![0u8; COPY_BUFFER];
    while remaining > 0 {
        let count = remaining.min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..count])
            .map_err(|e| e.to_string())?;
        crc = rkcrc32(crc, &buffer[..count], &crc_table);
        remaining -= count as u64;
    }
    file.write_all(&crc.to_le_bytes())
        .map_err(|e| e.to_string())?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut remaining = size - 32;
    let mut md5 = md5::Context::new();
    while remaining > 0 {
        let count = remaining.min(buffer.len() as u64) as usize;
        file.read_exact(&mut buffer[..count])
            .map_err(|e| e.to_string())?;
        md5.consume(&buffer[..count]);
        remaining -= count as u64;
    }
    file.write_all(format!("{:x}", md5.compute()).as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn update_firmware_apk_with_tools(
    firmware: &Path,
    apk: &Path,
    output: &Path,
    tools: &Path,
) -> Result<String, String> {
    update_firmware_apk_with_progress(
        firmware,
        apk,
        output,
        tools,
        DEFAULT_APK_PATH,
        |_| {},
    )
}

fn update_firmware_apk_with_progress(
    firmware: &Path,
    apk: &Path,
    output: &Path,
    tools: &Path,
    apk_path: &str,
    mut progress: impl FnMut(&str),
) -> Result<String, String> {
    if !firmware.is_file() || !apk.is_file() {
        return Err("Firmware or APK file does not exist".into());
    }
    if firmware == output {
        return Err("Output must not overwrite the source firmware".into());
    }
    if !apk_path.starts_with("/system/")
        || !apk_path.ends_with(".apk")
        || apk_path.contains("..")
        || apk_path.chars().any(char::is_whitespace)
    {
        return Err("Target must be an absolute .apk path under /system without spaces or '..'".into());
    }
    progress("正在检查固件格式…");
    let layout = parse_firmware_layout(firmware)?;
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp = std::env::temp_dir().join(format!("rkdevtool-apk-{unique}"));
    fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
    let result = (|| {
        let sparse = temp.join("super.img");
        let raw = temp.join("super.raw.img");
        let system = temp.join("system.img");
        let modified = temp.join("super.modified.img");
        progress("正在提取 super 分区…");
        copy_range(firmware, layout.super_offset, layout.super_size, &sparse)?;
        progress("正在展开 Android 稀疏镜像…");
        run_tool(
            &tools.join("simg2img"),
            &[sparse.to_str().unwrap(), raw.to_str().unwrap()],
        )?;
        progress("正在定位 system 分区…");
        let (offset, size) = system_extent(&raw)?;
        copy_range(&raw, offset, size, &system)?;
        progress(&format!("正在替换 {apk_path}…"));
        run_tool(
            &tools.join("debugfs"),
            &[
                "-w",
                "-R",
                &format!("rm {apk_path}"),
                system.to_str().unwrap(),
            ],
        )?;
        run_tool(
            &tools.join("debugfs"),
            &[
                "-w",
                "-R",
                &format!("write {} {apk_path}", apk.display()),
                system.to_str().unwrap(),
            ],
        )?;
        run_tool(
            &tools.join("debugfs"),
            &[
                "-w",
                "-R",
                &format!("ea_set {apk_path} security.selinux u:object_r:system_file:s0"),
                system.to_str().unwrap(),
            ],
        )?;
        progress("正在检查 system 文件系统…");
        run_tool(&tools.join("e2fsck"), &["-fn", system.to_str().unwrap()])?;
        let mut raw_file = OpenOptions::new()
            .write(true)
            .open(&raw)
            .map_err(|e| e.to_string())?;
        raw_file
            .seek(SeekFrom::Start(offset))
            .map_err(|e| e.to_string())?;
        std::io::copy(
            &mut File::open(&system).map_err(|e| e.to_string())?,
            &mut raw_file,
        )
        .map_err(|e| e.to_string())?;
        progress("正在重新生成 super 镜像…");
        run_tool(
            &tools.join("img2simg"),
            &[raw.to_str().unwrap(), modified.to_str().unwrap()],
        )?;
        progress("正在写入新固件并更新校验值…");
        patch_package(firmware, &modified, output, &layout)?;
        Ok(format!("New firmware saved: {}", output.display()))
    })();
    let _ = fs::remove_dir_all(temp);
    result
}

fn update(
    app: &AppHandle,
    firmware: &Path,
    apk: &Path,
    output: &Path,
    apk_path: &str,
) -> Result<String, String> {
    let tools = tool_dir(app)?;
    update_firmware_apk_with_progress(firmware, apk, output, &tools, apk_path, |message| {
        emit_log(app, message, "info");
    })
}

#[tauri::command]
pub async fn update_firmware_apk(
    app: AppHandle,
    state: State<'_, AppState>,
    firmware: String,
    apk: String,
    output: String,
    apk_path: String,
) -> Result<String, String> {
    devices::ensure_backend_not_busy(state.inner())?;
    devices::set_backend_busy(state.inner(), true)?;
    let worker_app = app.clone();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        update(
            &worker_app,
            Path::new(&firmware),
            Path::new(&apk),
            Path::new(&output),
            &apk_path,
        )
    })
    .await;
    let _ = devices::set_backend_busy(state.inner(), false);
    let result = worker.map_err(|e| e.to_string())?;
    match result {
        Ok(message) => {
            emit_log(&app, &message, "success");
            Ok(message)
        }
        Err(error) => {
            emit_log(&app, &format!("生成新固件失败：{error}"), "error");
            Err(error)
        }
    }
}
