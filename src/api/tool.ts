import { invoke } from "@tauri-apps/api/core";
import type {
  ActionParams,
  CurrentStorageInfo,
  DownloadExecutePayload,
  FirmwareInfo,
  RockusbDevice,
  ToolInfo,
} from "../types/tool";

export function getToolInfo() {
  return invoke<ToolInfo>("get_tool_info");
}

export function listDevices() {
  return invoke<RockusbDevice[]>("list_devices");
}

export function listAdbDevices() {
  return invoke<string[]>("list_adb_devices");
}

export function connectAdbDevice(address: string) {
  return invoke<string>("connect_adb_device", { address });
}

export function rebootToLoader(serial: string) {
  return invoke<void>("reboot_to_loader", { serial });
}

export function installApk(serial: string, path: string) {
  return invoke<string>("install_apk", { serial, path });
}

export function runAdbControl(serial: string, action: string) {
  return invoke<string>("run_adb_control", { serial, action });
}

export function startLogcat(serial: string) {
  return invoke<void>("start_logcat", { serial });
}

export function stopLogcat() {
  return invoke<void>("stop_logcat");
}

export function clearLogcat() {
  return invoke<void>("clear_logcat");
}

export function exportLogcat(path: string) {
  return invoke<number>("export_logcat", { path });
}

export interface BurnParameter {
  kind: "voiceKey" | "dn" | "ds" | "pk";
  value: string;
}

export function burnParameters(serial: string, toolPath: string, parameters: BurnParameter[]) {
  return invoke<string>("burn_parameters", { serial, toolPath, parameters });
}

export function selectDevice(locationId: string | null) {
  return invoke<void>("select_device", { locationId });
}

export function partitionList() {
  return invoke<string>("partition_list");
}

export function upgradeFirmware(path: string, noReset = false) {
  return invoke<void>("upgrade_firmware", { path, noReset });
}

export function downloadBoot(path: string) {
  return invoke<void>("download_boot", { path });
}

export function downloadExecute(payload: DownloadExecutePayload) {
  return invoke<void>("download_execute", { payload });
}

export function parseFirmware(path: string) {
  return invoke<FirmwareInfo>("parse_firmware", { path });
}

export function extractFirmware(path: string, outputDir: string) {
  return invoke<string>("extract_firmware", { path, outputDir });
}

export function readChipInfo() {
  return invoke<string>("read_chip_info");
}

export function runAction(action: string, params?: ActionParams) {
  return invoke<string>("run_action", { action, params: params ?? null });
}

export function getCurrentStorage() {
  return invoke<CurrentStorageInfo>("get_current_storage");
}

export function isToolBusy() {
  return invoke<boolean>("is_tool_busy");
}

export function updateFirmwareApk(firmware: string, apk: string, output: string, apkPath: string) {
  return invoke<string>("update_firmware_apk", { firmware, apk, output, apkPath });
}

export interface OtaZipRequest {
  kind: "apk" | "rom";
  version: string;
  pk: string;
  source: string;
  output: string;
}

export function buildOtaZip(request: OtaZipRequest) {
  return invoke<string>("build_ota_zip", { request });
}
