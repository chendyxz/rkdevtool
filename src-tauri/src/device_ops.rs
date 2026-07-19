//! Device operations via rockusb (chip/flash/storage/capability/boot/reset/erase).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, State};

use rockfile::boot::{
    RkBootEntry, RkBootEntryBytes, RkBootHeader, RkBootHeaderBytes, RkBootHeaderEntry,
};
use rockusb::nusb::Device;
use rockusb::protocol::{Capability, ChipInfo, FlashId, FlashInfo, ResetOpcode, StorageIndex};

use crate::devices::{self, format_location_id_for, is_rockusb_device_info};
use crate::state::AppState;
use crate::upgrade_tool::{CurrentStorageInfo, LogPayload};

const EVENT_TOOL_LOG: &str = "tool-log";

/// Match rkdeveloptool `EraseEmmc` / `erase_partition` chunk size (`1024 * 32` sectors).
const ERASE_LBA_CHUNK: u32 = 1024 * 32;

/// Match rkdeveloptool `MAX_ERASE_BLOCKS` for `RKU_EraseBlock`.
const ERASE_BLOCK_CHUNK: u16 = 16;

/// Match rkdeveloptool `DEFAULT_RW_LBA` / rockusb `MAXIO_SIZE` (128 sectors).
const READ_LBA_CHUNK: u32 = 128;

fn emit_log(app: &AppHandle, text: &str) {
    if text.is_empty() {
        return;
    }
    let lower = text.to_ascii_lowercase();
    let level = if lower.contains("success") || lower.contains("成功") {
        "success"
    } else if lower.contains("fail") || lower.contains("error") || lower.contains("失败") {
        "error"
    } else {
        "default"
    };
    let _ = app.emit(
        EVENT_TOOL_LOG,
        LogPayload {
            text: text.to_string(),
            level: level.to_string(),
            in_place: false,
        },
    );
}

fn emit_lines(app: &AppHandle, text: &str) {
    for line in text.lines() {
        emit_log(app, line);
    }
}

/// Download / Advanced page storage name → 1-based UI SSD No.
pub fn storage_name_to_ui_no(name: &str) -> Result<u32, String> {
    match name.trim().to_ascii_uppercase().as_str() {
        "FLASH" | "NAND" => Ok(1),
        "EMMC" => Ok(2),
        "SD" | "SD0" => Ok(3),
        "SD1" => Ok(4),
        "SPINOR" => Ok(5),
        "SPINAND" => Ok(6),
        "RAM" => Ok(7),
        "USB" => Ok(8),
        "SATA" => Ok(9),
        "PCIE" => Ok(10),
        other if other.is_empty() => Err("Storage type is empty".to_string()),
        other => Err(format!("Unknown storage type: {other}")),
    }
}

/// UI / upgrade_tool 1-based SSD No. → protocol StorageIndex.
/// Matches Advanced / Download page numbering (SATA=9, PCIE=10).
/// SPI NOR/NAND 优先走 MTD 块设备入口（新 Loader 常见），与 GetStorageMedia 回报一致。
pub fn storage_from_ui_no(no: u32) -> Result<StorageIndex, String> {
    Ok(match no {
        1 => StorageIndex::Nand,
        2 => StorageIndex::Emmc,
        3 => StorageIndex::Sd0,
        4 => StorageIndex::Sd1,
        5 => StorageIndex::MtdBlkSpiNor,
        6 => StorageIndex::MtdBlkSpiNand,
        7 => StorageIndex::Ram,
        8 => StorageIndex::MtdBlkNand, // Advanced UI label: USB（无独立 USB 协议项）
        9 => StorageIndex::Sata,
        10 => StorageIndex::Pcie,
        other => {
            let idx = other
                .checked_sub(1)
                .ok_or_else(|| format!("Invalid storage index: {other}"))?;
            if idx > u8::MAX as u32 {
                return Err(format!("Invalid storage index: {other}"));
            }
            StorageIndex::from(idx as u8)
        }
    })
}

fn storage_to_ui(index: StorageIndex) -> CurrentStorageInfo {
    // Advanced 页列表只有 1..10：FLASH/EMMC/SD/SD1/SPINOR/SPINAND/RAM/USB/SATA/PCIE。
    // MTD_* 与同介质的非 MTD 入口折叠到同一 UI 项。
    let (no, name) = match index {
        StorageIndex::Nand => (1, "FLASH"),
        StorageIndex::MtdBlkNand => (8, "USB"),
        StorageIndex::Emmc => (2, "EMMC"),
        StorageIndex::Sd0 => (3, "SD"),
        StorageIndex::Sd1 => (4, "SD1"),
        StorageIndex::SpiNor | StorageIndex::MtdBlkSpiNor => (5, "SPINOR"),
        StorageIndex::SpiNand | StorageIndex::MtdBlkSpiNand => (6, "SPINAND"),
        StorageIndex::Ram => (7, "RAM"),
        StorageIndex::Sata => (9, "SATA"),
        StorageIndex::Pcie => (10, "PCIE"),
        StorageIndex::Ufs => {
            return CurrentStorageInfo {
                no: 0,
                name: "UFS".to_string(),
            };
        }
        StorageIndex::Unknown(v) => {
            return CurrentStorageInfo {
                no: 0,
                name: format!("UNKNOWN({v})"),
            };
        }
        other => {
            return CurrentStorageInfo {
                no: 0,
                name: format!("{other}").to_ascii_uppercase(),
            };
        }
    };
    CurrentStorageInfo {
        no,
        name: name.to_string(),
    }
}

fn format_chip_info(info: &ChipInfo) -> String {
    let bytes = info.inner();
    let hex: String = bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("Chip Info: [{hex}]")
}

fn format_flash_id(id: &FlashId) -> String {
    format!("Flash ID: {}", id.to_str().trim())
}

fn format_flash_info(info: &FlashInfo) -> String {
    let mb = info.sectors() / 2048;
    format!(
        "Flash Info: size={} MB ({} sectors), block={} sectors, raw={:02x?}",
        mb,
        info.sectors(),
        info.block_size_sectors(),
        info.inner()
    )
}

fn format_capability(cap: &Capability) -> String {
    let mut lines = vec![format!("Capability raw: {:02x?}", cap.inner())];
    let mut flags = Vec::new();
    if cap.direct_lba() {
        flags.push("Direct LBA");
    }
    if cap.vendor_storage() {
        flags.push("Vendor storage");
    }
    if cap.first_4m_access() {
        flags.push("First 4M Access");
    }
    if cap.read_lba() {
        flags.push("Read LBA");
    }
    if cap.read_com_log() {
        flags.push("Read COM log");
    }
    if cap.read_idb_config() {
        flags.push("Read IDB config");
    }
    if cap.read_secure_mode() {
        flags.push("Read secure mode");
    }
    if cap.new_idb() {
        flags.push("New IDB");
    }
    if cap.switch_storage() {
        flags.push("Switch storage");
    }
    if flags.is_empty() {
        lines.push("Capability: (none)".to_string());
    } else {
        lines.push(format!("Capability: {}", flags.join(", ")));
    }
    lines.join("\n")
}

async fn open_selected_device(state: &AppState) -> Result<Device, String> {
    let selected = state
        .selected_device
        .lock()
        .map_err(|e| e.to_string())?
        .clone();

    let infos: Vec<_> = rockusb::nusb::devices()
        .await
        .map_err(|e| format!("Failed to list USB devices: {e}"))?
        .filter(is_rockusb_device_info)
        .collect();

    if infos.is_empty() {
        return Err("No Rockchip device found (enter Maskrom/Loader)".to_string());
    }

    let info = if let Some(location_id) = selected.as_deref().filter(|s| !s.is_empty()) {
        infos
            .into_iter()
            .find(|d| format_location_id_for(d) == location_id)
            .ok_or_else(|| {
                format!("Selected device {location_id} is no longer connected")
            })?
    } else if infos.len() == 1 {
        infos.into_iter().next().unwrap()
    } else {
        return Err("Multiple devices connected; select one in the status bar".to_string());
    };

    Device::from_usb_device_info(info)
        .await
        .map_err(|e| format!("Failed to open RockUSB device: {e}"))
}

async fn with_busy_device<F, Fut, T>(
    app: &AppHandle,
    state: &State<'_, AppState>,
    op_name: &str,
    f: F,
) -> Result<T, String>
where
    F: FnOnce(Device) -> Fut,
    Fut: std::future::Future<Output = Result<(T, String), String>>,
{
    devices::ensure_backend_not_busy(state.inner())?;
    devices::set_backend_busy(state.inner(), true)?;

    let result = async {
        emit_log(app, &format!("> rockusb {op_name}"));
        let device = open_selected_device(state.inner()).await?;
        let (value, output) = f(device).await?;
        emit_lines(app, &output);
        Ok(value)
    }
    .await;

    let _ = devices::set_backend_busy(state.inner(), false);
    result
}

#[tauri::command]
pub async fn read_chip_info(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    with_busy_device(&app, &state, "chip-info", |mut device| async move {
        let info = device
            .chip_info()
            .await
            .map_err(|e| format!("Read chip info failed: {e}"))?;
        let output = format_chip_info(&info);
        Ok((output.clone(), output))
    })
    .await
}

#[tauri::command]
pub async fn get_current_storage(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CurrentStorageInfo, String> {
    with_busy_device(&app, &state, "storage", |mut device| async move {
        let index = device
            .storage()
            .await
            .map_err(|e| format!("Get current storage failed: {e}"))?;
        let info = storage_to_ui(index);
        let output = format!("Current storage: No={} {} (*)", info.no, info.name);
        Ok((info, output))
    })
    .await
}

pub async fn switch_storage(
    app: AppHandle,
    state: State<'_, AppState>,
    ui_no: u32,
) -> Result<String, String> {
    let target = storage_from_ui_no(ui_no)?;
    let label = storage_to_ui(target).name;
    with_busy_device(
        &app,
        &state,
        &format!("switch-storage {ui_no} ({label})"),
        move |mut device| async move {
            device
                .switch_storage(target)
                .await
                .map_err(|e| format!("Switch storage failed: {e}"))?;
            // Protocol has no return code; re-query like rockusb example.
            let current = device
                .storage()
                .await
                .map_err(|e| format!("Switch storage verify failed: {e}"))?;
            let info = storage_to_ui(current);
            if current == target {
                let output = format!("Switched storage to {} (No={})", info.name, info.no);
                Ok((output.clone(), output))
            } else {
                Err(format!(
                    "Failed to switch storage to {label}; current is {} (No={})",
                    info.name, info.no
                ))
            }
        },
    )
    .await
}

pub async fn read_flash_id(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    with_busy_device(&app, &state, "flash-id", |mut device| async move {
        let id = device
            .flash_id()
            .await
            .map_err(|e| format!("Read Flash ID failed: {e}"))?;
        let output = format_flash_id(&id);
        Ok((output.clone(), output))
    })
    .await
}

pub async fn read_flash_info(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    with_busy_device(&app, &state, "flash-info", |mut device| async move {
        let info = device
            .flash_info()
            .await
            .map_err(|e| format!("Read Flash info failed: {e}"))?;
        let output = format_flash_info(&info);
        Ok((output.clone(), output))
    })
    .await
}

pub async fn read_capability(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    with_busy_device(&app, &state, "capability", |mut device| async move {
        let cap = device
            .capability()
            .await
            .map_err(|e| format!("Read capability failed: {e}"))?;
        let output = format_capability(&cap);
        Ok((output.clone(), output))
    })
    .await
}

async fn download_boot_entry(
    device: &mut Device,
    header: RkBootHeaderEntry,
    code: u16,
    file: &mut File,
    log: &mut String,
) -> Result<(), String> {
    for i in 0..header.count {
        let mut entry_bytes: RkBootEntryBytes = [0; 57];
        file.seek(SeekFrom::Start(
            u64::from(header.offset) + u64::from(header.size) * u64::from(i),
        ))
        .map_err(|e| format!("Seek boot entry failed: {e}"))?;
        file.read_exact(&mut entry_bytes)
            .map_err(|e| format!("Read boot entry failed: {e}"))?;

        let entry = RkBootEntry::from_bytes(&entry_bytes);
        let name = String::from_utf16(entry.name.as_slice())
            .unwrap_or_else(|_| format!("entry-{i}"))
            .trim_end_matches('\0')
            .to_string();
        log.push_str(&format!("Downloading 0x{code:x} #{i} ({name})...\n"));

        let mut data = vec![0u8; entry.data_size as usize];
        file.seek(SeekFrom::Start(u64::from(entry.data_offset)))
            .map_err(|e| format!("Seek boot data failed: {e}"))?;
        file.read_exact(&mut data)
            .map_err(|e| format!("Read boot data failed: {e}"))?;

        device
            .write_maskrom_area(code, &data)
            .await
            .map_err(|e| format!("Write maskrom area 0x{code:x} failed: {e}"))?;

        if entry.data_delay > 0 {
            thread::sleep(Duration::from_millis(u64::from(entry.data_delay)));
        }
    }
    Ok(())
}

/// Download Boot / Loader into Maskrom (upgrade_tool `DB`).
#[tauri::command]
pub async fn download_boot(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let boot_path = Path::new(&path).to_path_buf();
    if !boot_path.is_file() {
        return Err(format!("Boot/Loader file not found: {path}"));
    }

    with_busy_device(&app, &state, &format!("download-boot {path}"), move |mut device| async move {
        let mut file =
            File::open(&boot_path).map_err(|e| format!("Open boot file failed: {e}"))?;
        let mut header_bytes: RkBootHeaderBytes = [0; 102];
        file.read_exact(&mut header_bytes)
            .map_err(|e| format!("Read boot header failed: {e}"))?;
        let header = RkBootHeader::from_bytes(&header_bytes)
            .ok_or_else(|| {
                "Failed to parse Loader/Boot header (use MiniLoaderAll.bin or download.bin)"
                    .to_string()
            })?;

        let mut log = String::from("Download Boot Start\n");
        download_boot_entry(&mut device, header.entry_471, 0x471, &mut file, &mut log).await?;
        download_boot_entry(&mut device, header.entry_472, 0x472, &mut file, &mut log).await?;
        log.push_str("Download Boot Success");
        Ok(((), log))
    })
    .await
}

/// Test device connectivity via Test Unit Ready (rkdeveloptool / upgrade_tool `TD`).
pub async fn test_device(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    with_busy_device(&app, &state, "test-device", |mut device| async move {
        device
            .test_unit_ready()
            .await
            .map_err(|e| format!("Test Device failed: {e}"))?;
        let output = "Test Device OK.".to_string();
        Ok((output.clone(), output))
    })
    .await
}

pub async fn reset_device(
    app: AppHandle,
    state: State<'_, AppState>,
    opcode: ResetOpcode,
) -> Result<String, String> {
    let label = opcode.to_string();
    with_busy_device(&app, &state, &format!("reset-device {label}"), move |mut device| async move {
        device
            .reset_device(opcode)
            .await
            .map_err(|e| format!("Reset device failed: {e}"))?;
        let output = match opcode {
            ResetOpcode::Maskrom => "Enter Maskrom Success".to_string(),
            ResetOpcode::Reset => "Reset Device Success".to_string(),
            other => format!("Reset device ({other}) success"),
        };
        Ok((output.clone(), output))
    })
    .await
}

/// Plan LBA operation chunks: `(offset, count)` pairs.
fn lba_chunks(start: u32, count: u32, chunk_size: u32) -> Result<Vec<(u32, u16)>, String> {
    if count == 0 {
        return Err("Sector count must be greater than 0".to_string());
    }
    if chunk_size == 0 || chunk_size > u32::from(u16::MAX) {
        return Err("Invalid LBA chunk size".to_string());
    }
    let _end = start
        .checked_add(count)
        .ok_or_else(|| "Start sector + count overflows u32".to_string())?;

    let mut chunks = Vec::new();
    let mut offset = start;
    let mut remaining = count;
    while remaining > 0 {
        let chunk = remaining.min(chunk_size) as u16;
        chunks.push((offset, chunk));
        offset += u32::from(chunk);
        remaining -= u32::from(chunk);
    }
    Ok(chunks)
}

fn erase_lba_chunks(start: u32, count: u32) -> Result<Vec<(u32, u16)>, String> {
    lba_chunks(start, count, ERASE_LBA_CHUNK)
}

fn read_lba_chunks(start: u32, count: u32) -> Result<Vec<(u32, u16)>, String> {
    lba_chunks(start, count, READ_LBA_CHUNK)
}

/// Erase LBA sectors (upgrade_tool `EL` / rkdeveloptool `RKU_EraseLBA`).
pub async fn erase_sectors(
    app: AppHandle,
    state: State<'_, AppState>,
    start: u32,
    count: u32,
) -> Result<String, String> {
    let chunks = erase_lba_chunks(start, count)?;
    let chunk_total = chunks.len();

    with_busy_device(
        &app,
        &state,
        &format!("erase-lba start={start} count={count}"),
        move |mut device| async move {
            for (offset, chunk) in chunks {
                device.erase_lba(offset, chunk).await.map_err(|e| {
                    format!("Erase LBA failed at sector {offset} (count {chunk}): {e}")
                })?;
            }
            let output = format!(
                "Erase sectors OK: start={start}, count={count}, chunks={chunk_total}"
            );
            Ok((output.clone(), output))
        },
    )
    .await
}

fn is_emmc_flash_id(id: &FlashId) -> bool {
    id.to_str().as_bytes().get(..4) == Some(b"EMMC")
}

/// Plan `RKU_EraseBlock` chunks: `(block_offset, block_count)`.
fn erase_block_chunks(block_count: u32) -> Result<Vec<(u32, u16)>, String> {
    if block_count == 0 {
        return Err("Flash block count is 0".to_string());
    }
    let mut chunks = Vec::new();
    let mut offset = 0u32;
    let mut remaining = block_count;
    while remaining > 0 {
        let chunk = remaining.min(u32::from(ERASE_BLOCK_CHUNK)) as u16;
        chunks.push((offset, chunk));
        offset += u32::from(chunk);
        remaining -= u32::from(chunk);
    }
    Ok(chunks)
}

fn flash_block_count(info: &FlashInfo) -> Result<u32, String> {
    let block_size = u32::from(info.block_size_sectors());
    if block_size == 0 {
        return Err("Flash reports block size 0".to_string());
    }
    let sectors = info.sectors();
    if sectors == 0 {
        return Err("Flash reports 0 sectors".to_string());
    }
    Ok(sectors / block_size)
}

/// Erase entire flash (upgrade_tool `EF` / rkdeveloptool `ef` / `EraseAllBlocks`).
///
/// Expects Loader mode (user downloads Boot separately). Direct LBA / eMMC uses
/// `EraseLBA` from sector 0; otherwise `EraseForce` by flash blocks (CS0).
pub async fn erase_all(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let app_for_progress = app.clone();
    with_busy_device(&app, &state, "erase-flash", move |mut device| async move {
        let info = device
            .flash_info()
            .await
            .map_err(|e| {
                format!(
                    "Read Flash info failed: {e} (in Maskrom, download Boot/Loader first, then retry)"
                )
            })?;
        let sectors = info.sectors();
        if sectors == 0 {
            return Err("Flash reports 0 sectors".to_string());
        }

        let direct_lba = device
            .capability()
            .await
            .map(|cap| cap.direct_lba())
            .unwrap_or(false);
        let is_emmc = device
            .flash_id()
            .await
            .map(|id| is_emmc_flash_id(&id))
            .unwrap_or(false);

        let mut log = format!(
            "Flash: {} sectors ({} MB), block={} sectors, direct_lba={}, emmc={}\n",
            sectors,
            sectors / 2048,
            info.block_size_sectors(),
            direct_lba,
            is_emmc
        );

        if direct_lba || is_emmc {
            let chunks = erase_lba_chunks(0, sectors)?;
            let total = chunks.len();
            log.push_str(&format!("Erase path: LBA ({total} chunks)\n"));
            for (i, (offset, chunk)) in chunks.into_iter().enumerate() {
                device.erase_lba(offset, chunk).await.map_err(|e| {
                    format!("Erase LBA failed at sector {offset} (count {chunk}): {e}")
                })?;
                if (i + 1) % 8 == 0 || i + 1 == total {
                    let done = offset.saturating_add(u32::from(chunk)).min(sectors);
                    emit_log(
                        &app_for_progress,
                        &format!("Erase progress: {done}/{sectors} sectors"),
                    );
                }
            }
        } else {
            let block_count = flash_block_count(&info)?;
            let chunks = erase_block_chunks(block_count)?;
            let total = chunks.len();
            log.push_str(&format!(
                "Erase path: Force block erase, {block_count} blocks ({total} chunks)\n"
            ));
            for (i, (offset, chunk)) in chunks.into_iter().enumerate() {
                device.erase_force(offset, chunk).await.map_err(|e| {
                    format!("Erase block failed at block {offset} (count {chunk}): {e}")
                })?;
                if (i + 1) % 8 == 0 || i + 1 == total {
                    let done = offset.saturating_add(u32::from(chunk)).min(block_count);
                    emit_log(
                        &app_for_progress,
                        &format!("Erase progress: {done}/{block_count} blocks"),
                    );
                }
            }
        }

        log.push_str("Erase Flash OK");
        Ok((log.clone(), log))
    })
    .await
}

/// Export flash image via ReadLBA (rkdeveloptool `rl`).
///
/// `count == None` means read from `start` to end of flash.
pub async fn export_image(
    app: AppHandle,
    state: State<'_, AppState>,
    start: u32,
    count: Option<u32>,
    output_path: String,
) -> Result<String, String> {
    if output_path.trim().is_empty() {
        return Err("Output path is required".to_string());
    }
    let out = Path::new(&output_path).to_path_buf();
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(format!("Output directory does not exist: {}", parent.display()));
        }
    }

    let app_for_progress = app.clone();
    with_busy_device(
        &app,
        &state,
        &format!("read-lba start={start} -> {}", out.display()),
        move |mut device| async move {
            let total_sectors = device
                .flash_info()
                .await
                .map_err(|e| {
                    format!(
                        "Read Flash info failed: {e} (in Maskrom, download Boot/Loader first, then retry)"
                    )
                })?
                .sectors();
            if total_sectors == 0 {
                return Err("Flash reports 0 sectors".to_string());
            }
            if start >= total_sectors {
                return Err(format!(
                    "Start sector {start} is beyond flash size ({total_sectors} sectors)"
                ));
            }

            let count = match count {
                Some(c) if c > 0 => c,
                Some(_) => return Err("Sector count must be greater than 0".to_string()),
                None => total_sectors - start,
            };
            let end = start
                .checked_add(count)
                .ok_or_else(|| "Start sector + count overflows u32".to_string())?;
            if end > total_sectors {
                return Err(format!(
                    "Read range [{start}, {end}) exceeds flash size ({total_sectors} sectors)"
                ));
            }

            let chunks = read_lba_chunks(start, count)?;
            let chunk_total = chunks.len();
            let mut file = File::create(&out)
                .map_err(|e| format!("Create output file failed: {e}"))?;
            let mut buf = vec![0u8; READ_LBA_CHUNK as usize * 512];

            for (i, (offset, chunk)) in chunks.into_iter().enumerate() {
                let nbytes = chunk as usize * 512;
                let slice = &mut buf[..nbytes];
                let transferred = device.read_lba(offset, slice).await.map_err(|e| {
                    format!("Read LBA failed at sector {offset} (count {chunk}): {e}")
                })?;
                if transferred as usize != nbytes {
                    return Err(format!(
                        "Short read at sector {offset}: got {transferred} bytes, expected {nbytes}"
                    ));
                }
                file.write_all(slice)
                    .map_err(|e| format!("Write output file failed: {e}"))?;

                if (i + 1) % 32 == 0 || i + 1 == chunk_total {
                    let done = offset.saturating_add(u32::from(chunk)).saturating_sub(start);
                    let pct = ((u64::from(done) * 100) / u64::from(count)).min(100);
                    emit_log(
                        &app_for_progress,
                        &format!("Export progress: {done}/{count} sectors ({pct}%)"),
                    );
                }
            }
            file.flush()
                .map_err(|e| format!("Flush output file failed: {e}"))?;

            let bytes = u64::from(count) * 512;
            let output = format!(
                "Export image OK: start={start}, count={count}, bytes={bytes}, file={}",
                out.display()
            );
            Ok((output.clone(), output))
        },
    )
    .await
}

/// Handle rockusb-backed advanced actions. Returns `None` if action stays on upgrade_tool.
pub async fn try_run_action(
    app: AppHandle,
    state: State<'_, AppState>,
    action: &str,
    start_sector: Option<&str>,
    sector_count: Option<&str>,
    output_path: Option<&str>,
) -> Result<Option<String>, String> {
    match action {
        "读取FlashID" => Ok(Some(read_flash_id(app, state).await?)),
        "读取Flash信息" => Ok(Some(read_flash_info(app, state).await?)),
        "读取Chip信息" => Ok(Some(read_chip_info(app, state).await?)),
        "读取Capability" => Ok(Some(read_capability(app, state).await?)),
        "测试设备" => Ok(Some(test_device(app, state).await?)),
        "重启设备" => Ok(Some(reset_device(app, state, ResetOpcode::Reset).await?)),
        "进入Maskrom" => Ok(Some(reset_device(app, state, ResetOpcode::Maskrom).await?)),
        "切换存储" => {
            let no: u32 = start_sector
                .filter(|s| !s.is_empty())
                .unwrap_or("1")
                .parse()
                .map_err(|_| "Invalid storage index".to_string())?;
            Ok(Some(switch_storage(app, state, no).await?))
        }
        "擦除扇区" => {
            let start: u32 = start_sector
                .filter(|s| !s.is_empty())
                .unwrap_or("0")
                .parse()
                .map_err(|_| "Invalid start sector".to_string())?;
            let count: u32 = sector_count
                .filter(|s| !s.is_empty())
                .unwrap_or("1")
                .parse()
                .map_err(|_| "Invalid sector count".to_string())?;
            Ok(Some(erase_sectors(app, state, start, count).await?))
        }
        "擦除所有" => Ok(Some(erase_all(app, state).await?)),
        "导出镜像" => {
            let start: u32 = start_sector
                .filter(|s| !s.is_empty())
                .unwrap_or("0")
                .parse()
                .map_err(|_| "Invalid start sector".to_string())?;
            let count = match sector_count.filter(|s| !s.is_empty()) {
                Some(s) => Some(
                    s.parse::<u32>()
                        .map_err(|_| "Invalid sector count".to_string())?,
                ),
                None => None,
            };
            let path = output_path
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "Output path is required".to_string())?
                .to_string();
            Ok(Some(export_image(app, state, start, count, path).await?))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_storage_roundtrip_common() {
        for no in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] {
            let idx = storage_from_ui_no(no).unwrap();
            let info = storage_to_ui(idx);
            assert_eq!(info.no, no, "no={no} idx={idx:?}");
        }
    }

    #[test]
    fn storage_name_maps_spinand() {
        assert_eq!(storage_name_to_ui_no("SPINAND").unwrap(), 6);
        assert_eq!(storage_name_to_ui_no("spinand").unwrap(), 6);
        assert_eq!(
            storage_from_ui_no(storage_name_to_ui_no("SPINAND").unwrap()).unwrap(),
            StorageIndex::MtdBlkSpiNand
        );
    }

    #[test]
    fn mtd_spi_nand_maps_to_spinand_ui_slot() {
        let info = storage_to_ui(StorageIndex::MtdBlkSpiNand);
        assert_eq!(info.no, 6);
        assert_eq!(info.name, "SPINAND");
        assert_eq!(
            storage_from_ui_no(6).unwrap(),
            StorageIndex::MtdBlkSpiNand
        );
    }

    #[test]
    fn sata_pcie_map_to_protocol_bits() {
        assert_eq!(storage_from_ui_no(9).unwrap(), StorageIndex::Sata);
        assert_eq!(storage_from_ui_no(10).unwrap(), StorageIndex::Pcie);
        assert_eq!(storage_to_ui(StorageIndex::Sata).name, "SATA");
        assert_eq!(storage_to_ui(StorageIndex::SpiNand).no, 6);
    }

    #[test]
    fn erase_lba_chunks_single_and_split() {
        assert_eq!(erase_lba_chunks(0, 1).unwrap(), vec![(0, 1)]);
        assert_eq!(
            erase_lba_chunks(100, ERASE_LBA_CHUNK).unwrap(),
            vec![(100, ERASE_LBA_CHUNK as u16)]
        );
        assert_eq!(
            erase_lba_chunks(0, ERASE_LBA_CHUNK + 10).unwrap(),
            vec![(0, ERASE_LBA_CHUNK as u16), (ERASE_LBA_CHUNK, 10)]
        );
        assert!(erase_lba_chunks(0, 0).is_err());
        assert!(erase_lba_chunks(u32::MAX, 1).is_err());
    }

    #[test]
    fn read_lba_chunks_match_rkdeveloptool_default() {
        assert_eq!(read_lba_chunks(0, 128).unwrap(), vec![(0, 128)]);
        assert_eq!(
            read_lba_chunks(10, 200).unwrap(),
            vec![(10, 128), (138, 72)]
        );
    }

    #[test]
    fn erase_block_chunks_respect_max() {
        assert_eq!(erase_block_chunks(1).unwrap(), vec![(0, 1)]);
        assert_eq!(
            erase_block_chunks(20).unwrap(),
            vec![(0, 16), (16, 4)]
        );
        assert!(erase_block_chunks(0).is_err());
    }

    #[test]
    fn flash_block_count_from_sectors() {
        // 256 MiB / 128 KiB blocks → 2048 blocks
        let mut raw = [0u8; 11];
        raw[0..4].copy_from_slice(&524288u32.to_le_bytes()); // sectors
        raw[4..6].copy_from_slice(&256u16.to_le_bytes()); // block size in sectors (128KiB)
        let info = FlashInfo::from_bytes(raw);
        assert_eq!(flash_block_count(&info).unwrap(), 2048);
    }
}
