//! Per-target facts about the toolchains bundled next to the application.
//!
//! Installers ship prebuilt, platform-specific toolchains under `resources/`
//! (ADB platform-tools, e2fsprogs). Their directory names differ per target, so
//! every lookup goes through this module instead of hard-coding one platform in
//! feature code.

use std::path::{Path, PathBuf};
use std::process::Command;

use tauri::{AppHandle, Manager};

/// Directory holding the bundled ADB platform-tools build for this target.
pub fn platform_tools_dir() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos-arm64"
    } else if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    }
}

/// File name of the ADB executable on this target.
pub fn adb_file_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "adb.exe"
    } else {
        "adb"
    }
}

/// ADB shipped with the app: read from the source tree in dev, from the resource
/// directory in bundled builds. Windows also needs the `AdbWinApi` DLLs that sit
/// next to `adb.exe`, so the whole directory is bundled and invoked in place.
pub fn adb_path(app: &AppHandle) -> Result<PathBuf, String> {
    let relative = PathBuf::from("resources")
        .join("platform-tools")
        .join(platform_tools_dir())
        .join(adb_file_name());
    let path = if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(&relative)
    } else {
        app.path()
            .resource_dir()
            .map_err(|e| e.to_string())?
            .join(&relative)
    };
    path.is_file().then_some(path).ok_or_else(|| {
        format!(
            "Bundled ADB is missing from the application resources ({})",
            relative.display()
        )
    })
}

/// Windows opens a console window for every child process of a GUI application
/// unless `CREATE_NO_WINDOW` is set, so bundled tools (`upgrade_tool`, `adb`)
/// would flash a black window on each call.
pub fn hide_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    #[cfg(not(windows))]
    let _ = cmd;
}

/// Bundled ADB as a ready-to-spawn command, with the console window hidden.
pub fn adb_command(adb: &Path) -> Command {
    let mut cmd = Command::new(adb);
    hide_console(&mut cmd);
    cmd
}

/// `std::env::consts::OS` of the running build ("macos", "windows", "linux").
///
/// The frontend reads it once at startup to hide pages whose toolchain is not
/// bundled for the current platform.
#[tauri::command]
pub fn get_platform() -> String {
    std::env::consts::OS.to_string()
}

#[cfg(test)]
mod tests {
    use super::{adb_file_name, platform_tools_dir};

    #[test]
    fn resolves_the_toolchain_for_the_current_target() {
        assert!(matches!(
            platform_tools_dir(),
            "macos-arm64" | "windows-x86_64" | "linux-x86_64"
        ));
        assert!(matches!(adb_file_name(), "adb" | "adb.exe"));
        assert_eq!(
            adb_file_name().ends_with(".exe"),
            cfg!(target_os = "windows")
        );
    }
}
