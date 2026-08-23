export interface AdvancedAction {
  command: string;
  labelKey: string;
  destructive?: boolean;
}

export const ADVANCED_ACTIONS: AdvancedAction[] = [
  { command: "read-flash-id", labelKey: "action.readFlashId" },
  { command: "read-flash-info", labelKey: "action.readFlashInfo" },
  { command: "read-chip-info", labelKey: "action.readChipInfo" },
  { command: "read-capability", labelKey: "action.readCapability" },
  { command: "test-device", labelKey: "action.testDevice" },
  { command: "reboot-device", labelKey: "action.rebootDevice" },
  { command: "enter-maskrom", labelKey: "action.enterMaskrom" },
  { command: "switch-storage", labelKey: "action.switchStorage" },
  { command: "clear-serial", labelKey: "action.clearSerial" },
  { command: "detect-secure-mode", labelKey: "action.detectSecureMode" },
  { command: "export-serial-log", labelKey: "action.exportSerialLog" },
  { command: "get-current-storage", labelKey: "action.getCurrentStorage" },
  { command: "export-image", labelKey: "action.exportImage" },
  { command: "erase-sector", labelKey: "action.eraseSector", destructive: true },
  { command: "erase-all", labelKey: "action.eraseAll", destructive: true },
  { command: "switch-usb3", labelKey: "action.switchUsb3" },
];
