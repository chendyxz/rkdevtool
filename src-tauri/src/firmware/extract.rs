use afptool_rs::{UpdateHeader, RKAFP_MAGIC, RKAF_SIGNATURE};
use std::ffi::CStr;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct FirmwareImage {
    pub name: String,
    pub path: PathBuf,
    pub flash_offset_sectors: u64,
    pub flash_size_sectors: u64,
    pub byte_count: u64,
}

#[derive(Debug, Clone)]
pub struct ExtractedFirmware {
    pub images: Vec<FirmwareImage>,
    pub loader_path: Option<PathBuf>,
    pub log: String,
}

pub fn extract_firmware_file(path: &str, output_dir: &str) -> Result<String, String> {
    Ok(extract_firmware(path, output_dir, false)?.log)
}

pub fn extract_firmware_for_upgrade(
    path: &str,
    output_dir: &str,
) -> Result<ExtractedFirmware, String> {
    extract_firmware(path, output_dir, true)
}

fn extract_firmware(
    path: &str,
    output_dir: &str,
    keep_boot_file: bool,
) -> Result<ExtractedFirmware, String> {
    let path = Path::new(path);
    if !path.is_file() {
        return Err(format!("File not found: {}", path.display()));
    }

    std::fs::create_dir_all(output_dir).map_err(|e| format!("Failed to create output directory: {e}"))?;

    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut signature = [0u8; 4];
    file.read_exact(&mut signature).map_err(|e| e.to_string())?;

    let mut log = String::new();

    let mut images = Vec::new();
    let mut loader_path = None;

    match signature {
        [b'R', b'K', b'A', b'F'] => {
            unpack_rkaf(path, output_dir, &mut log, &mut images, &mut loader_path)?
        }
        [b'R', b'K', b'F', b'W'] => {
            let buf = read_file(path)?;
            unpack_rkfw(&buf, output_dir, &mut log)?;

            let embedded = Path::new(output_dir).join("embedded-update.img");
            if embedded.exists() {
                unpack_rkaf(
                    &embedded,
                    output_dir,
                    &mut log,
                    &mut images,
                    &mut loader_path,
                )?;
                let _ = std::fs::remove_file(&embedded);
                let boot_path = Path::new(output_dir).join("BOOT");
                if loader_path.is_none() && boot_path.is_file() {
                    loader_path = Some(boot_path.clone());
                }
                if !keep_boot_file {
                    let _ = std::fs::remove_file(boot_path);
                }
            }
        }
        _ => return Err("Unsupported firmware format (RKFW or RKAF/update.img required)".to_string()),
    }

    if images.is_empty() {
        return Err("Firmware package contains no writable images".to_string());
    }

    Ok(ExtractedFirmware {
        images,
        loader_path,
        log,
    })
}

fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

fn push_log(log: &mut String, line: impl AsRef<str>) {
    log.push_str(line.as_ref());
    log.push('\n');
}

fn unpack_rkfw(buf: &[u8], output_dir: &str, log: &mut String) -> Result<(), String> {
    if buf.len() < 0x29 {
        return Err("Incomplete RKFW header".to_string());
    }

    push_log(log, "RKFW signature detected");

    let version = format!(
        "{}.{}.{}",
        buf[9],
        buf[8],
        u16::from_le_bytes([buf[6], buf[7]])
    );
    push_log(log, &format!("version: {version}"));

    let boot_offset = u32::from_le_bytes(buf[0x19..0x1d].try_into().unwrap()) as usize;
    let boot_size = u32::from_le_bytes(buf[0x1d..0x21].try_into().unwrap()) as usize;
    let update_offset = u32::from_le_bytes(buf[0x21..0x25].try_into().unwrap()) as usize;
    let update_size = u32::from_le_bytes(buf[0x25..0x29].try_into().unwrap()) as usize;

    if boot_offset + boot_size > buf.len() {
        return Err("RKFW Boot region is out of file range".to_string());
    }

    let boot_path = Path::new(output_dir).join("BOOT");
    push_log(
        log,
        format!(
            "{boot_offset:08x}-{end:08x} {path:26} (size: {boot_size})",
            end = boot_offset + boot_size - 1,
            path = boot_path.display(),
        ),
    );
    write_bytes(&boot_path, &buf[boot_offset..boot_offset + boot_size])?;

    if update_offset + update_size > buf.len() {
        return Err("RKFW embedded update.img region is out of file range".to_string());
    }

    if &buf[update_offset..update_offset + 4] != RKAF_SIGNATURE {
        return Err("RKFW does not contain embedded RKAF update.img".to_string());
    }

    let embedded_path = Path::new(output_dir).join("embedded-update.img");
    push_log(
        log,
        format!(
            "{update_offset:08x}-{end:08x} {path:26} (size: {update_size})",
            end = update_offset + update_size - 1,
            path = embedded_path.display(),
        ),
    );
    write_bytes(
        &embedded_path,
        &buf[update_offset..update_offset + update_size],
    )?;

    Ok(())
}

fn unpack_rkaf(
    path: &Path,
    output_dir: &str,
    log: &mut String,
    images: &mut Vec<FirmwareImage>,
    loader_path: &mut Option<PathBuf>,
) -> Result<(), String> {
    let mut fp = File::open(path).map_err(|e| e.to_string())?;
    let mut header_buf = vec![0u8; mem::size_of::<UpdateHeader>()];
    fp.read_exact(&mut header_buf).map_err(|e| e.to_string())?;

    let header = UpdateHeader::from_bytes(header_buf.as_mut());
    let magic_str = std::str::from_utf8(&header.magic).map_err(|e| e.to_string())?;
    if magic_str != RKAFP_MAGIC {
        return Err("Invalid RKAF header".to_string());
    }

    let filesize = fp.metadata().map_err(|e| e.to_string())?.len();
    push_log(log, &format!("Filesize: {filesize}"));

    let manufacturer = cstr_field(&header.manufacturer);
    let model = cstr_field(&header.model);
    push_log(log, &format!("manufacturer: {manufacturer}"));
    push_log(log, &format!("model: {model}"));

    let metadata_path = Path::new(output_dir).join("partition-metadata.txt");
    let mut metadata_file = File::create(&metadata_path).map_err(|e| e.to_string())?;

    let num_parts = header.num_parts;
    for index in 0..num_parts {
        let part = &header.parts[index as usize];
        let part_full_path = match CStr::from_bytes_until_nul(&part.full_path) {
            Ok(value) => value.to_string_lossy().into_owned(),
            Err(_) => continue,
        };

        if part_full_path.is_empty() {
            continue;
        }

        let part_name = CStr::from_bytes_until_nul(&part.name)
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
        let flash_size = part.flash_size;
        let flash_offset = part.flash_offset;
        let part_offset = part.part_offset;
        let padded_size = part.padded_size;
        let part_byte_count = part.part_byte_count;

        writeln!(
            metadata_file,
            "{},{},{:#010x},{:#010x},{:#010x},{:#010x},{:#010x}",
            part_name,
            part_full_path,
            flash_size,
            flash_offset,
            part_offset,
            padded_size,
            part_byte_count,
        )
        .map_err(|e| e.to_string())?;

        if part_full_path == "SELF" || part_full_path == "RESERVED" {
            continue;
        }

        let relative_path = safe_relative_path(&part_full_path)?;
        let output_path = Path::new(output_dir).join(relative_path);
        extract_part(
            &mut fp,
            part_offset as u64,
            part_byte_count as u64,
            &output_path,
            log,
        )?;

        if is_loader_file_name(&part_full_path) {
            *loader_path = Some(output_path.clone());
        }

        if !has_flash_target(flash_offset) {
            continue;
        }

        images.push(FirmwareImage {
            name: if part_name.trim().is_empty() {
                part_full_path.clone()
            } else {
                part_name
            },
            path: output_path,
            flash_offset_sectors: u64::from(flash_offset),
            flash_size_sectors: u64::from(flash_size),
            byte_count: u64::from(part_byte_count),
        });
    }

    push_log(
        log,
        format!("\nPartition metadata saved to: {}", metadata_path.display()),
    );

    Ok(())
}

fn safe_relative_path(full_path: &str) -> Result<PathBuf, String> {
    let normalized = full_path.replace('\\', "/");
    let mut output = PathBuf::new();

    for component in Path::new(&normalized).components() {
        match component {
            Component::Normal(part) => output.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Unsafe firmware entry path: {full_path}"));
            }
        }
    }

    if output.as_os_str().is_empty() {
        return Err("Firmware entry path is empty".to_string());
    }
    Ok(output)
}

pub(crate) fn is_loader_file_name(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let file_name = Path::new(&normalized)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    file_name.eq_ignore_ascii_case("download.bin")
        || file_name.eq_ignore_ascii_case("MiniLoaderAll.bin")
}

fn has_flash_target(flash_offset: u32) -> bool {
    flash_offset != u32::MAX
}

fn extract_part(
    fp: &mut File,
    offset: u64,
    len: u64,
    output_path: &Path,
    log: &mut String,
) -> Result<(), String> {
    push_log(
        log,
        format!(
            "{offset:08x}-{end:08x} {path}",
            end = offset + len.saturating_sub(1),
            path = output_path.display(),
        ),
    );

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }

    let mut buffer = vec![0u8; 16 * 1024];
    let mut output = File::create(output_path).map_err(|e| e.to_string())?;
    fp.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;

    let mut remaining = len;
    while remaining > 0 {
        let read_len = remaining.min(buffer.len() as u64) as usize;
        fp.read_exact(&mut buffer[..read_len])
            .map_err(|e| e.to_string())?;
        output
            .write_all(&buffer[..read_len])
            .map_err(|e| e.to_string())?;
        remaining -= read_len as u64;
    }

    Ok(())
}

fn write_bytes(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let mut file = File::create(path).map_err(|e| e.to_string())?;
    file.write_all(data).map_err(|e| e.to_string())?;
    Ok(())
}

fn cstr_field(bytes: &[u8]) -> String {
    CStr::from_bytes_until_nul(bytes)
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::{has_flash_target, is_loader_file_name, safe_relative_path};
    use std::path::PathBuf;

    #[test]
    fn recognizes_the_supported_loader_filenames() {
        assert!(is_loader_file_name("download.bin"));
        assert!(is_loader_file_name("MiniLoaderAll.bin"));
        assert!(is_loader_file_name("MINILOADERALL.BIN"));
        assert!(!is_loader_file_name("uboot.img"));
    }

    #[test]
    fn excludes_entries_without_a_flash_target_from_lba_writes() {
        assert!(has_flash_target(0x4000));
        assert!(!has_flash_target(u32::MAX));
    }

    #[test]
    fn rejects_firmware_paths_that_escape_the_output_directory() {
        assert!(safe_relative_path("../outside.img").is_err());
        assert!(safe_relative_path("/absolute.img").is_err());
        assert_eq!(safe_relative_path("Image/boot.img").unwrap(), PathBuf::from("Image/boot.img"));
    }
}
