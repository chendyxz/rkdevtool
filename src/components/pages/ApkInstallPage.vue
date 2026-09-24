<script setup lang="ts">
import { ref } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import AppButton from "../ui/AppButton.vue";
import PathField from "../ui/PathField.vue";
import { pickFile } from "../../composables/useFilePicker";
import { useAppState } from "../../composables/useAppState";
import { useToolCommand, toolApi } from "../../composables/useToolCommand";
import { useI18n } from "../../i18n";

const apkPath = ref("");
const wirelessAddress = ref("");
const { adbSerial, appendLog, busy, setAdbSerial } = useAppState();
const { run } = useToolCommand();
const { t } = useI18n();

const controlGroups = [
  {
    title: "apkInstall.controls.volume",
    actions: ["volumeUp", "volumeDown", "mute"],
  },
  {
    title: "apkInstall.controls.navigation",
    actions: ["back", "home", "recents", "up", "down", "left", "right", "enter"],
  },
  {
    title: "apkInstall.controls.device",
    actions: ["power", "restartApp", "reboot"],
  },
  {
    title: "apkInstall.controls.settingsGroup",
    actions: ["settings"],
  },
] as const;

async function browseApk() {
  apkPath.value = (await pickFile(t("apkInstall.pick"))) ?? apkPath.value;
}

async function install() {
  const path = apkPath.value.trim();
  if (!path) {
    appendLog(t("apkInstall.selectFirst"), "error");
    return;
  }
  if (!path.toLowerCase().endsWith(".apk")) {
    appendLog(t("apkInstall.invalidFile"), "error");
    return;
  }
  if (!adbSerial.value) {
    appendLog(t("apkInstall.noDevice"), "error");
    return;
  }

  try {
    await run(() => toolApi.installApk(adbSerial.value!, path));
    appendLog(t("apkInstall.success"), "success");
  } catch (error) {
    appendLog(String(error), "error");
  }
}

async function runControl(action: string) {
  if (!adbSerial.value) {
    appendLog(t("apkInstall.noDevice"), "error");
    return;
  }
  if (action === "reboot") {
    const confirmed = await ask(t("apkInstall.controls.rebootConfirm"), {
      title: t("apkInstall.controls.reboot"),
      kind: "warning",
    });
    if (!confirmed) return;
  }

  try {
    const result = await run(() => toolApi.runAdbControl(adbSerial.value!, action));
    const message = action === "restartApp" && result
      ? t("apkInstall.controls.restartAppSuccess", { package: result })
      : t("apkInstall.controls.executed", { action: t(`apkInstall.controls.${action}`) });
    appendLog(message, "success");
  } catch (error) {
    appendLog(String(error), "error");
  }
}

async function connectWireless() {
  const address = wirelessAddress.value.trim();
  if (!address) {
    appendLog(t("apkInstall.wireless.empty"), "error");
    return;
  }

  try {
    const serial = await run(() => toolApi.connectAdbDevice(address));
    if (!serial) return;
    wirelessAddress.value = serial;
    setAdbSerial(serial);
    appendLog(t("apkInstall.wireless.success", { address: serial }), "success");
  } catch (error) {
    appendLog(String(error), "error");
  }
}
</script>

<template>
  <div class="apk-install-page">
    <div class="install-card">
      <p class="install-card__hint">{{ t("apkInstall.hint") }}</p>
      <div class="install-row">
        <span class="install-row__label">{{ t("apkInstall.apk") }}</span>
        <PathField
          v-model="apkPath"
          browse-variant="inline"
          :placeholder="t('apkInstall.placeholder')"
          @browse="browseApk"
        />
        <AppButton variant="primary" :disabled="busy" @click="install">
          {{ t("apkInstall.install") }}
        </AppButton>
      </div>
    </div>

    <section class="control-card">
      <h2 class="control-card__title">{{ t("apkInstall.controls.title") }}</h2>
      <div class="wireless-row">
        <span class="wireless-row__label">{{ t("apkInstall.wireless.label") }}</span>
        <input
          v-model="wirelessAddress"
          class="wireless-row__input"
          type="text"
          :placeholder="t('apkInstall.wireless.placeholder')"
          :disabled="busy"
          @keyup.enter="connectWireless"
        />
        <AppButton size="sm" variant="primary" :disabled="busy" @click="connectWireless">
          {{ t("apkInstall.wireless.connect") }}
        </AppButton>
      </div>
      <div class="control-groups">
        <div v-for="group in controlGroups" :key="group.title" class="control-group">
          <h3 class="control-group__title">{{ t(group.title) }}</h3>
          <div class="control-group__buttons">
            <AppButton
              v-for="action in group.actions"
              :key="action"
              size="sm"
              :variant="action === 'reboot' ? 'destructive' : 'secondary'"
              :disabled="busy"
              @click="runControl(action)"
            >
              {{ t(`apkInstall.controls.${action}`) }}
            </AppButton>
          </div>
        </div>
      </div>
    </section>
  </div>
</template>

<style scoped>
.apk-install-page { display: flex; flex-direction: column; gap: 16px; width: 100%; max-width: 652px; }
.install-card { padding: 20px; border: 1px solid var(--color-border); border-radius: var(--border-radius-lg); background: var(--color-surface); }
.install-card__hint { margin: 0 0 18px; color: var(--color-text-secondary); line-height: 1.6; }
.install-row { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 12px; }
.install-row__label { color: var(--color-text-secondary); white-space: nowrap; }
.install-row :deep(.path-field) { min-width: 0; }
.control-card { padding: 20px; border: 1px solid var(--color-border); border-radius: var(--border-radius-lg); background: var(--color-surface); }
.control-card__title { margin: 0 0 18px; font-size: 14px; }
.wireless-row { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: 12px; margin-bottom: 20px; padding-bottom: 18px; border-bottom: 1px solid var(--color-border); }
.wireless-row__label { color: var(--color-text-secondary); white-space: nowrap; }
.wireless-row__input { width: 100%; height: 36px; padding: 0 12px; border: 1px solid var(--color-border); border-radius: var(--border-radius-md); background: var(--color-surface); color: var(--color-text-primary); }
.control-groups { display: flex; flex-direction: column; gap: 18px; }
.control-group__title { margin: 0 0 10px; color: var(--color-text-secondary); font-size: 12px; font-weight: 600; }
.control-group__buttons { display: flex; flex-wrap: wrap; gap: 8px; }
</style>
