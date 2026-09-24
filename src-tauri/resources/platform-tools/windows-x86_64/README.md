# Windows ADB / 内置 ADB

The Windows build runs ADB from this directory
(`resources/platform-tools/windows-x86_64/adb.exe`).

Windows 构建从本目录调用内置 ADB（`resources/platform-tools/windows-x86_64/adb.exe`）。

The binaries are **not** committed. They come from Google's official
`platform-tools-latest-windows.zip` and are fetched by:

二进制文件**不入库**，来自 Google 官方 `platform-tools-latest-windows.zip`，由下面的脚本获取：

```bash
packaging/windows/fetch-platform-tools.sh
```

Expected contents / 需要包含：

| File | Purpose |
|------|---------|
| `adb.exe` | ADB client used by the APK install / logcat pages |
| `AdbWinApi.dll` | Required by `adb.exe` |
| `AdbWinUsbApi.dll` | Required by `adb.exe` |
| `NOTICE.txt`, `source.properties` | Upstream license and version metadata |

CI (`windows-latest`) runs the same script before building, see
`.github/workflows/build.yml`.
