# RockUSB Firmware Upgrade TDD Evidence

Source plan: derived from the requested upgrade workflow in this task.

## User Journeys

- A Maskrom user can select an RK3576 update image, upload its `MiniLoaderAll.bin` using RockUSB Boot, then write each partition through RockUSB LBA.
- A Maskrom user can select an RV1106G3 update image, upload its `download.bin` using RockUSB Boot, then write each partition through RockUSB LBA.
- An Android user can flash a `TYPE: GPT` firmware image so its partition table is created before its Android partition images are written.
- An Android user can flash an Android sparse image so its logical output blocks, rather than sparse container bytes, are written.
- A malformed firmware entry cannot write outside the temporary extraction directory or outside the target flash.
- A Download Image user can upload one Loader and write named GPT or legacy `parameter` partitions through RockUSB, without invoking `upgrade_tool`.
- A Download Image user can force-write an image to either hexadecimal or decimal LBA addresses through RockUSB.
- A Download Image user gets one RockUSB device reset after all selected images succeed; a Loader-only operation leaves the device running.

## Evidence

| Guarantee | Validation | Result |
|---|---|---|
| `download.bin` and `MiniLoaderAll.bin` are Loader names | `cargo test` | PASS |
| LBA writes use 128-sector chunks and pad only the last sector | `cargo test` | PASS |
| Entries with `flash_offset = 0xffffffff` are not LBA writes | `cargo test` | PASS |
| Loader readiness is determined by a successful RockUSB flash probe, not displayed USB mode | `cargo test` | PASS |
| Firmware write logs display byte counts using B, KiB, MiB, or GiB as appropriate | `cargo test` | PASS |
| `TYPE: GPT` parameter files produce valid primary and backup GPT data before partition writes | `cargo test` | PASS |
| A `grow` partition spans from its declared start sector through the final usable GPT sector | `cargo test` | PASS |
| Binary bytes following the final GPT parameter entry do not invalidate the partition table | `cargo test` | PASS |
| Length-prefixed `PARM` payloads exclude trailing binary data even when the final UUID has no line terminator | `cargo test parm_payload_length_excludes_binary_data_after_final_uuid` | PASS |
| Android sparse raw, fill, and hole chunks are parsed into their logical block output | `cargo test` | PASS |
| Android sparse `DONT_CARE` chunks advance the logical image offset without a USB write | `cargo test` | PASS |
| Firmware progress emits at most once per integer percentage point | `cargo test` | PASS |
| Firmware entry paths cannot escape the extraction directory | `cargo test` | PASS |
| Download Image accepts hexadecimal and decimal LBA addresses | `cargo test download_address_accepts_hexadecimal_and_decimal_lba` | PASS |
| Download Image resolves partition names case-insensitively | `cargo test resolves_download_partition_without_case_sensitivity` | PASS |
| Download Image reads named partitions from standard GPT entries | `cargo test reads_download_partitions_from_gpt_entries` | PASS |
| Download Image reads legacy Rockchip `parameter` partition ranges | `cargo test parses_legacy_parameter_partition_ranges_without_gpt_marker` | PASS |
| Loader readiness exposes an increasing 0-100% progress line before image writes | `cargo test loader_ready_progress_increases_across_retry_attempts` | PASS |
| Download Image resets only after one or more non-Loader images are written | `cargo test download_resets_only_after_an_image_was_written` | PASS |
| Frontend type check and production bundle still succeed | `npm run build` | PASS |
| RK3576 fixture exposes `MiniLoaderAll.bin` | `cargo run --example parse_firmware -- --unpack <temp> update_dshanpi-a1_buildroot.img` | PASS |
| RV1106G3 fixture exposes `download.bin` | `cargo run --example parse_firmware -- --unpack <temp> update_luckfox_rv1106g3.img` | PASS |
| TisonPi Android fixture exposes a GPT parameter file and sparse `super.img` | `cargo run --example parse_firmware -- --unpack <temp> update_tisonpi_android.img` | PASS |

## RED / GREEN

The initial Rust test run failed because Loader identification and LBA write planning were absent. A second RED run caught the missing exclusion for the `0xffffffff` sentinel offset. A regression test then caught the missing readiness probe after Loader download. Formatting tests also failed before human-readable byte units were implemented. Android GPT and sparse parsing tests failed before their respective implementations. A final regression test caught binary data appended to a GPT parameter table. Sparse write optimization was protected by a regression test for `DONT_CARE` chunks. Progress update throttling was also introduced to prevent log reflow. For Download Image migration, the new address parser, partition resolver, and legacy `parameter` parser first failed to compile because those capabilities did not exist. Loader readiness progress and final reset behavior likewise began as missing test targets. After implementation, `cargo test` completed with 51 passing tests.

## Coverage

This repository does not configure a Rust coverage runner or frontend test runner. The focused Rust unit tests, two real firmware parsing runs, and the frontend production build are the available validation evidence. No real device was written during Download Image validation.
