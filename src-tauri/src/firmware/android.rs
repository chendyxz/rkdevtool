const SECTOR_SIZE: usize = 512;
const GPT_ENTRY_SIZE: usize = 128;
const GPT_ENTRY_COUNT: usize = 128;
const GPT_ENTRY_SECTORS: u64 = (GPT_ENTRY_SIZE * GPT_ENTRY_COUNT / SECTOR_SIZE) as u64;
const GPT_PRIMARY_SECTORS: u64 = 2 + GPT_ENTRY_SECTORS;
const GPT_BACKUP_SECTORS: u64 = 1 + GPT_ENTRY_SECTORS;

const BASIC_DATA_GUID: [u8; 16] = [
    0xa2, 0xa0, 0xd0, 0xeb, 0xe5, 0xb9, 0x33, 0x44, 0x87, 0xc0, 0x68, 0xb6, 0xb7, 0x26, 0x99,
    0xc7,
];
const DISK_GUID: [u8; 16] = [
    0x52, 0x4b, 0x44, 0x54, 0x4f, 0x4f, 0x4c, 0x00, 0x91, 0x80, 0x64, 0x65, 0x76, 0x74, 0x6f, 0x6f,
];
const SPARSE_MAGIC: u32 = 0xed26_ff3a;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SparseChunkKind {
    Raw,
    Fill,
    DontCare,
    Crc32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SparseHeader {
    pub file_header_size: usize,
    pub chunk_header_size: usize,
    pub block_size: u64,
    pub total_chunks: u32,
    pub output_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SparseChunk {
    pub kind: SparseChunkKind,
    pub output_bytes: u64,
    pub payload_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GptPartition {
    pub name: String,
    pub start_sector: u64,
    pub sector_count: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct GptTables {
    pub primary: Vec<u8>,
    pub backup_start_sector: u32,
    pub backup: Vec<u8>,
}

pub(crate) fn parse_gpt_parameter(data: &[u8]) -> Result<Option<Vec<GptPartition>>, String> {
    let text = String::from_utf8_lossy(data);
    if !text.contains("TYPE: GPT") {
        return Ok(None);
    }

    let cmdline_start = text
        .find("CMDLINE:")
        .ok_or_else(|| "GPT parameter has no CMDLINE".to_string())?;
    let cmdline = text[cmdline_start + "CMDLINE:".len()..]
        .split('\0')
        .next()
        .unwrap_or_default();
    let (_, entries) = cmdline
        .split_once(':')
        .ok_or_else(|| "GPT parameter CMDLINE has no mtdparts entries".to_string())?;

    let mut partitions = Vec::new();
    for entry in entries.split(',') {
        let (size, rest) = entry
            .trim()
            .split_once('@')
            .ok_or_else(|| format!("Invalid GPT partition entry: {entry}"))?;
        let (offset, name) = rest
            .split_once('(')
            .ok_or_else(|| format!("Invalid GPT partition entry: {entry}"))?;
        let name = name
            .split_once(')')
            .map(|(name, _)| name)
            .ok_or_else(|| format!("Invalid GPT partition entry: {entry}"))?
            .split(':')
            .next()
            .unwrap_or_default()
            .trim();
        if name.is_empty() || !name.is_ascii() {
            return Err(format!("Invalid GPT partition name: {name}"));
        }

        let sector_count = if size.trim() == "-" {
            None
        } else {
            Some(parse_hex_sector(size.trim())?)
        };
        partitions.push(GptPartition {
            name: name.to_string(),
            start_sector: parse_hex_sector(offset.trim())?,
            sector_count,
        });
    }

    if partitions.is_empty() {
        return Err("GPT parameter has no partitions".to_string());
    }
    Ok(Some(partitions))
}

pub(crate) fn parse_sparse_header(data: &[u8]) -> Result<Option<SparseHeader>, String> {
    if data.len() < 4 || u32::from_le_bytes(data[..4].try_into().unwrap()) != SPARSE_MAGIC {
        return Ok(None);
    }
    if data.len() < 28 {
        return Err("Incomplete Android sparse header".to_string());
    }
    let major_version = u16::from_le_bytes(data[4..6].try_into().unwrap());
    let file_header_size = u16::from_le_bytes(data[8..10].try_into().unwrap()) as usize;
    let chunk_header_size = u16::from_le_bytes(data[10..12].try_into().unwrap()) as usize;
    let block_size = u64::from(u32::from_le_bytes(data[12..16].try_into().unwrap()));
    let total_blocks = u64::from(u32::from_le_bytes(data[16..20].try_into().unwrap()));
    let total_chunks = u32::from_le_bytes(data[20..24].try_into().unwrap());

    if major_version != 1 {
        return Err(format!("Unsupported Android sparse major version: {major_version}"));
    }
    if file_header_size < 28 || chunk_header_size < 12 {
        return Err("Invalid Android sparse header size".to_string());
    }
    if block_size == 0 || block_size % SECTOR_SIZE as u64 != 0 {
        return Err("Android sparse block size is not a multiple of 512".to_string());
    }
    if total_blocks == 0 || total_chunks == 0 {
        return Err("Android sparse image has no output blocks".to_string());
    }

    Ok(Some(SparseHeader {
        file_header_size,
        chunk_header_size,
        block_size,
        total_chunks,
        output_bytes: total_blocks
            .checked_mul(block_size)
            .ok_or_else(|| "Android sparse output size overflows".to_string())?,
    }))
}

pub(crate) fn parse_sparse_chunk_header(
    sparse: &SparseHeader,
    data: &[u8],
) -> Result<SparseChunk, String> {
    if data.len() < sparse.chunk_header_size {
        return Err("Incomplete Android sparse chunk header".to_string());
    }
    let chunk_type = u16::from_le_bytes(data[..2].try_into().unwrap());
    let block_count = u64::from(u32::from_le_bytes(data[4..8].try_into().unwrap()));
    let total_size = u64::from(u32::from_le_bytes(data[8..12].try_into().unwrap()));
    let output_bytes = block_count
        .checked_mul(sparse.block_size)
        .ok_or_else(|| "Android sparse chunk output size overflows".to_string())?;
    let header_size = sparse.chunk_header_size as u64;

    let (kind, expected_payload, output_bytes) = match chunk_type {
        0xcac1 => (SparseChunkKind::Raw, output_bytes, output_bytes),
        0xcac2 => (SparseChunkKind::Fill, 4, output_bytes),
        0xcac3 => (SparseChunkKind::DontCare, 0, output_bytes),
        0xcac4 => (SparseChunkKind::Crc32, 4, 0),
        _ => return Err(format!("Unsupported Android sparse chunk type: 0x{chunk_type:04x}")),
    };
    if total_size != header_size + expected_payload {
        return Err(format!("Invalid Android sparse chunk size for 0x{chunk_type:04x}"));
    }

    Ok(SparseChunk {
        kind,
        output_bytes,
        payload_bytes: expected_payload,
    })
}

pub(crate) fn build_gpt_tables(
    partitions: &[GptPartition],
    flash_sectors: u32,
) -> Result<GptTables, String> {
    let flash_sectors = u64::from(flash_sectors);
    if flash_sectors <= GPT_PRIMARY_SECTORS + GPT_BACKUP_SECTORS {
        return Err("Flash is too small for a GPT".to_string());
    }
    if partitions.len() > GPT_ENTRY_COUNT {
        return Err("GPT parameter has too many partitions".to_string());
    }

    let first_usable = GPT_PRIMARY_SECTORS;
    let last_usable = flash_sectors - GPT_BACKUP_SECTORS - 1;
    let mut resolved = Vec::with_capacity(partitions.len());
    for partition in partitions {
        let end_sector = match partition.sector_count {
            Some(count) if count > 0 => partition
                .start_sector
                .checked_add(count - 1)
                .ok_or_else(|| format!("GPT partition {} range overflows", partition.name))?,
            Some(_) => return Err(format!("GPT partition {} has zero size", partition.name)),
            None => last_usable,
        };
        if partition.start_sector < first_usable || end_sector > last_usable {
            return Err(format!("GPT partition {} is outside the usable flash range", partition.name));
        }
        resolved.push((partition, end_sector));
    }
    let mut ranges: Vec<_> = resolved
        .iter()
        .map(|(partition, end_sector)| (partition.start_sector, *end_sector, partition.name.as_str()))
        .collect();
    ranges.sort_unstable_by_key(|range| range.0);
    for pair in ranges.windows(2) {
        if pair[0].1 >= pair[1].0 {
            return Err(format!("GPT partitions {} and {} overlap", pair[0].2, pair[1].2));
        }
    }

    let mut entries = vec![0u8; GPT_ENTRY_SIZE * GPT_ENTRY_COUNT];
    for (index, (partition, end_sector)) in resolved.iter().enumerate() {
        let entry = &mut entries[index * GPT_ENTRY_SIZE..(index + 1) * GPT_ENTRY_SIZE];
        entry[..16].copy_from_slice(&BASIC_DATA_GUID);
        entry[16] = (index + 1) as u8;
        entry[32..40].copy_from_slice(&partition.start_sector.to_le_bytes());
        entry[40..48].copy_from_slice(&end_sector.to_le_bytes());
        for (offset, codepoint) in partition.name.encode_utf16().take(36).enumerate() {
            entry[56 + offset * 2..58 + offset * 2].copy_from_slice(&codepoint.to_le_bytes());
        }
    }
    let entries_crc = crc32(&entries);

    let mut primary = vec![0u8; GPT_PRIMARY_SECTORS as usize * SECTOR_SIZE];
    write_protective_mbr(&mut primary[..SECTOR_SIZE], flash_sectors);
    primary[SECTOR_SIZE..2 * SECTOR_SIZE].copy_from_slice(&gpt_header(
        1,
        flash_sectors - 1,
        2,
        first_usable,
        last_usable,
        entries_crc,
    ));
    primary[2 * SECTOR_SIZE..].copy_from_slice(&entries);

    let backup_start = flash_sectors - GPT_BACKUP_SECTORS;
    let mut backup = vec![0u8; GPT_BACKUP_SECTORS as usize * SECTOR_SIZE];
    backup[..entries.len()].copy_from_slice(&entries);
    backup[entries.len()..entries.len() + SECTOR_SIZE].copy_from_slice(&gpt_header(
        flash_sectors - 1,
        1,
        backup_start,
        first_usable,
        last_usable,
        entries_crc,
    ));

    Ok(GptTables {
        primary,
        backup_start_sector: u32::try_from(backup_start)
            .map_err(|_| "Flash exceeds the RockUSB LBA address range".to_string())?,
        backup,
    })
}

fn parse_hex_sector(value: &str) -> Result<u64, String> {
    u64::from_str_radix(value.trim_start_matches("0x"), 16)
        .map_err(|_| format!("Invalid GPT sector value: {value}"))
}

fn write_protective_mbr(mbr: &mut [u8], flash_sectors: u64) {
    mbr[446 + 4] = 0xee;
    mbr[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    mbr[446 + 12..446 + 16]
        .copy_from_slice(&u32::try_from(flash_sectors.saturating_sub(1)).unwrap_or(u32::MAX).to_le_bytes());
    mbr[510..512].copy_from_slice(&[0x55, 0xaa]);
}

fn gpt_header(
    current_lba: u64,
    backup_lba: u64,
    entries_lba: u64,
    first_usable: u64,
    last_usable: u64,
    entries_crc: u32,
) -> [u8; SECTOR_SIZE] {
    let mut header = [0u8; SECTOR_SIZE];
    header[..8].copy_from_slice(b"EFI PART");
    header[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    header[12..16].copy_from_slice(&92u32.to_le_bytes());
    header[24..32].copy_from_slice(&current_lba.to_le_bytes());
    header[32..40].copy_from_slice(&backup_lba.to_le_bytes());
    header[40..48].copy_from_slice(&first_usable.to_le_bytes());
    header[48..56].copy_from_slice(&last_usable.to_le_bytes());
    header[56..72].copy_from_slice(&DISK_GUID);
    header[72..80].copy_from_slice(&entries_lba.to_le_bytes());
    header[80..84].copy_from_slice(&(GPT_ENTRY_COUNT as u32).to_le_bytes());
    header[84..88].copy_from_slice(&(GPT_ENTRY_SIZE as u32).to_le_bytes());
    header[88..92].copy_from_slice(&entries_crc.to_le_bytes());
    let header_crc = crc32(&header[..92]);
    header[16..20].copy_from_slice(&header_crc.to_le_bytes());
    header
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARAMETER: &[u8] = b"PARM\0TYPE: GPT\0CMDLINE:mtdparts=rk29xxnand:0x00002000@0x00002000(security),0x00002000@0x00004000(uboot),0x00014000@0x0000c800(boot),-@0x00020800(userdata:grow)\0";

    #[test]
    fn parses_gpt_partition_ranges_from_rockchip_parameter() {
        let partitions = parse_gpt_parameter(PARAMETER).unwrap().unwrap();

        assert_eq!(partitions[2].name, "boot");
        assert_eq!(partitions[2].start_sector, 0xc800);
        assert_eq!(partitions[2].sector_count, Some(0x14000));
        assert_eq!(partitions[3].name, "userdata");
        assert_eq!(partitions[3].sector_count, None);
    }

    #[test]
    fn ignores_binary_data_after_the_final_gpt_partition_entry() {
        let parameter = b"TYPE: GPT\0CMDLINE:mtdparts=rk29xxnand:0x00002000@0x00002000(security),-@0x007ed000(userdata:grow)\x0b\x42";
        let partitions = parse_gpt_parameter(parameter).unwrap().unwrap();

        assert_eq!(partitions.len(), 2);
        assert_eq!(partitions[1].name, "userdata");
        assert_eq!(partitions[1].start_sector, 0x007e_d000);
        assert_eq!(partitions[1].sector_count, None);
    }

    #[test]
    fn generated_gpt_contains_the_primary_header_and_named_partition() {
        let partitions = parse_gpt_parameter(PARAMETER).unwrap().unwrap();
        let tables = build_gpt_tables(&partitions, 0x0080_0000).unwrap();

        assert_eq!(&tables.primary[512..520], b"EFI PART");
        let boot_entry = 2 * 512 + 2 * 128;
        assert_eq!(u64::from_le_bytes(tables.primary[boot_entry + 32..boot_entry + 40].try_into().unwrap()), 0xc800);
        assert_eq!(u64::from_le_bytes(tables.primary[boot_entry + 40..boot_entry + 48].try_into().unwrap()), 0x207ff);
        assert_eq!(tables.backup_start_sector, 0x0080_0000 - 33);
    }

    #[test]
    fn grow_partition_ends_at_the_last_usable_gpt_sector() {
        let partitions = parse_gpt_parameter(PARAMETER).unwrap().unwrap();
        let tables = build_gpt_tables(&partitions, 0x0080_0000).unwrap();
        let userdata_entry = 2 * 512 + 3 * 128;

        assert_eq!(u64::from_le_bytes(tables.primary[userdata_entry + 32..userdata_entry + 40].try_into().unwrap()), 0x20800);
        assert_eq!(u64::from_le_bytes(tables.primary[userdata_entry + 40..userdata_entry + 48].try_into().unwrap()), 0x0080_0000 - 34);
    }

    #[test]
    fn gpt_builder_rejects_a_partition_that_exceeds_the_flash() {
        let partitions = parse_gpt_parameter(PARAMETER).unwrap().unwrap();

        assert!(build_gpt_tables(&partitions, 0x0002_0000).is_err());
    }

    #[test]
    fn parses_android_sparse_header_and_raw_chunk() {
        let mut header = [0u8; 28];
        header[..4].copy_from_slice(&0xed26_ff3au32.to_le_bytes());
        header[4..6].copy_from_slice(&1u16.to_le_bytes());
        header[8..10].copy_from_slice(&28u16.to_le_bytes());
        header[10..12].copy_from_slice(&12u16.to_le_bytes());
        header[12..16].copy_from_slice(&4096u32.to_le_bytes());
        header[16..20].copy_from_slice(&2u32.to_le_bytes());
        header[20..24].copy_from_slice(&1u32.to_le_bytes());
        let sparse = parse_sparse_header(&header).unwrap().unwrap();

        let mut chunk = [0u8; 12];
        chunk[..2].copy_from_slice(&0xcac1u16.to_le_bytes());
        chunk[4..8].copy_from_slice(&2u32.to_le_bytes());
        chunk[8..12].copy_from_slice(&(12u32 + 8192).to_le_bytes());
        let chunk = parse_sparse_chunk_header(&sparse, &chunk).unwrap();

        assert_eq!(sparse.output_bytes, 8192);
        assert_eq!(chunk.kind, SparseChunkKind::Raw);
        assert_eq!(chunk.output_bytes, 8192);
        assert_eq!(chunk.payload_bytes, 8192);
    }

    #[test]
    fn parses_sparse_fill_and_dont_care_chunks() {
        let sparse = SparseHeader {
            file_header_size: 28,
            chunk_header_size: 12,
            block_size: 4096,
            total_chunks: 2,
            output_bytes: 8192,
        };
        let mut fill = [0u8; 12];
        fill[..2].copy_from_slice(&0xcac2u16.to_le_bytes());
        fill[4..8].copy_from_slice(&1u32.to_le_bytes());
        fill[8..12].copy_from_slice(&16u32.to_le_bytes());
        let fill = parse_sparse_chunk_header(&sparse, &fill).unwrap();

        let mut dont_care = [0u8; 12];
        dont_care[..2].copy_from_slice(&0xcac3u16.to_le_bytes());
        dont_care[4..8].copy_from_slice(&1u32.to_le_bytes());
        dont_care[8..12].copy_from_slice(&12u32.to_le_bytes());
        let dont_care = parse_sparse_chunk_header(&sparse, &dont_care).unwrap();

        assert_eq!(fill.kind, SparseChunkKind::Fill);
        assert_eq!(fill.payload_bytes, 4);
        assert_eq!(dont_care.kind, SparseChunkKind::DontCare);
        assert_eq!(dont_care.output_bytes, 4096);
    }
}
