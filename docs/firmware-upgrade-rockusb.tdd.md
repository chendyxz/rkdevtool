# RockUSB Firmware Upgrade TDD Evidence

Source plan: derived from the requested upgrade workflow in this task.

## User Journeys

- A Maskrom user can select an RK3576 update image, upload its `MiniLoaderAll.bin` using RockUSB Boot, then write each partition through RockUSB LBA.
- A Maskrom user can select an RV1106G3 update image, upload its `download.bin` using RockUSB Boot, then write each partition through RockUSB LBA.
- An Android user can flash a `TYPE: GPT` firmware image so its partition table is created before its Android partition images are written.
- An Android user can flash an Android sparse image so its logical output blocks, rather than sparse container bytes, are written.
- A malformed firmware entry cannot write outside the temporary extraction directory or outside the target flash.

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
| Android sparse raw, fill, and hole chunks are parsed into their logical block output | `cargo test` | PASS |
| Android sparse `DONT_CARE` chunks advance the logical image offset without a USB write | `cargo test` | PASS |
| Firmware progress emits at most once per integer percentage point | `cargo test` | PASS |
| Firmware entry paths cannot escape the extraction directory | `cargo test` | PASS |
| Frontend type check and production bundle still succeed | `npm run build` | PASS |
| RK3576 fixture exposes `MiniLoaderAll.bin` | `cargo run --example parse_firmware -- --unpack <temp> update_dshanpi-a1_buildroot.img` | PASS |
| RV1106G3 fixture exposes `download.bin` | `cargo run --example parse_firmware -- --unpack <temp> update_luckfox_rv1106g3.img` | PASS |
| TisonPi Android fixture exposes a GPT parameter file and sparse `super.img` | `cargo run --example parse_firmware -- --unpack <temp> update_tisonpi_android.img` | PASS |

## RED / GREEN

The initial Rust test run failed because Loader identification and LBA write planning were absent. A second RED run caught the missing exclusion for the `0xffffffff` sentinel offset. A regression test then caught the missing readiness probe after Loader download. Formatting tests also failed before human-readable byte units were implemented. Android GPT and sparse parsing tests failed before their respective implementations. A final regression test caught binary data appended to a GPT parameter table. Sparse write optimization was protected by a regression test for `DONT_CARE` chunks. Progress update throttling was also introduced to prevent log reflow. After implementation, `cargo test` completed with 45 passing tests.

## Coverage

This repository does not configure a Rust coverage runner or frontend test runner. The focused Rust unit tests, two real firmware parsing runs, and the frontend production build are the available validation evidence.
