#!/bin/sh
# Downloads Google's official Windows platform-tools and copies the files the
# Windows build bundles next to the app (ADB plus the DLLs it depends on).
#
# Used by CI (windows-latest) and by local Windows builds:
#   packaging/windows/fetch-platform-tools.sh
set -e

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
DEST="$ROOT_DIR/src-tauri/resources/platform-tools/windows-x86_64"
URL="https://dl.google.com/android/repository/platform-tools-latest-windows.zip"
FILES="adb.exe AdbWinApi.dll AdbWinUsbApi.dll NOTICE.txt source.properties"

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required to download platform-tools" >&2
  exit 1
fi

WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT INT TERM

echo "Downloading $URL"
curl -fsSL "$URL" -o "$WORK_DIR/platform-tools.zip"

# Windows runners ship bsdtar/7z rather than unzip, so try every extractor.
if command -v unzip >/dev/null 2>&1; then
  unzip -q -o "$WORK_DIR/platform-tools.zip" -d "$WORK_DIR"
elif command -v 7z >/dev/null 2>&1; then
  7z x -y -o"$WORK_DIR" "$WORK_DIR/platform-tools.zip" >/dev/null
else
  tar -xf "$WORK_DIR/platform-tools.zip" -C "$WORK_DIR"
fi

mkdir -p "$DEST"
for FILE in $FILES; do
  cp "$WORK_DIR/platform-tools/$FILE" "$DEST/$FILE"
done
echo "Installed ADB into $DEST"
