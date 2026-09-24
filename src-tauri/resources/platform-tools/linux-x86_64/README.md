# Linux ADB / 内置 ADB

No Linux ADB build is bundled yet: `src/platform.rs` looks for
`resources/platform-tools/linux-x86_64/adb` once this directory contains one, and
`tauri.linux.conf.json` already bundles everything in this directory.

暂未内置 Linux 版 ADB：本目录放进可执行文件后，`src/platform.rs` 会按
`resources/platform-tools/linux-x86_64/adb` 查找，`tauri.linux.conf.json`
已配置打包本目录内容。

To enable it, drop the official Linux `platform-tools` files here and add the
directory to `src-tauri/tauri.linux.conf.json` if the path location changes.

启用方式：把官方 Linux `platform-tools` 内容放到本目录（`adb` 需要可执行权限）。
