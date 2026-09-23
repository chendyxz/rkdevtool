<script setup lang="ts">
import { ref, watch } from "vue";
import AppButton from "../ui/AppButton.vue";
import PathField from "../ui/PathField.vue";
import { pickFile, pickSavePath } from "../../composables/useFilePicker";
import { useAppState } from "../../composables/useAppState";
import { useToolCommand, toolApi } from "../../composables/useToolCommand";
import { useI18n } from "../../i18n";

const APK_UPDATE_CONFIG_KEY = "rkdevtool.apk-update-config.v1";

function loadConfig() {
  try {
    const saved = JSON.parse(localStorage.getItem(APK_UPDATE_CONFIG_KEY) ?? "null");
    if (saved && typeof saved === "object") {
      return {
        firmware: typeof saved.firmware === "string" ? saved.firmware : "",
        apk: typeof saved.apk === "string" ? saved.apk : "",
        output: typeof saved.output === "string" ? saved.output : "",
        target: typeof saved.target === "string" ? saved.target : "/system/app/lxzk/lxzk.apk",
      };
    }
  } catch {
    // Ignore invalid or obsolete local data.
  }
  return { firmware: "", apk: "", output: "", target: "/system/app/lxzk/lxzk.apk" };
}

const savedConfig = loadConfig();
const firmware = ref(savedConfig.firmware);
const apk = ref(savedConfig.apk);
const output = ref(savedConfig.output);
const target = ref(savedConfig.target);
const status = ref("");
const failed = ref(false);
const { appendLog, busy } = useAppState();
const { run } = useToolCommand();
const { t } = useI18n();

watch([firmware, apk, output, target], () => {
  localStorage.setItem(
    APK_UPDATE_CONFIG_KEY,
    JSON.stringify({ firmware: firmware.value, apk: apk.value, output: output.value, target: target.value }),
  );
}, { flush: "sync" });

async function browseFirmware() { firmware.value = (await pickFile(t("apkUpdate.pickFirmware"))) ?? firmware.value; }
async function browseApk() { apk.value = (await pickFile(t("apkUpdate.pickApk"))) ?? apk.value; }
async function browseOutput() {
  const suggested = firmware.value.replace(/\.img$/i, "-lxzk-updated.img");
  output.value = (await pickSavePath(t("apkUpdate.pickOutput"), suggested)) ?? output.value;
}
async function createFirmware() {
  if (!firmware.value || !apk.value || !output.value || !target.value) { appendLog(t("apkUpdate.missing"), "error"); return; }
  status.value = t("apkUpdate.running");
  failed.value = false;
  try {
    const result = await run(() => toolApi.updateFirmwareApk(firmware.value, apk.value, output.value, target.value));
    if (result) {
      status.value = t("apkUpdate.success", { path: output.value });
      appendLog(status.value, "success");
    }
  } catch (error) {
    failed.value = true;
    status.value = t("apkUpdate.failed", { error: String(error) });
    appendLog(status.value, "error");
  }
}
</script>

<template>
  <div class="apk-page">
    <div class="card">
      <label>{{ t("apkUpdate.firmware") }}</label><PathField v-model="firmware" browse-variant="inline" @browse="browseFirmware" />
      <label>{{ t("apkUpdate.apk") }}</label><PathField v-model="apk" browse-variant="inline" @browse="browseApk" />
      <label>{{ t("apkUpdate.output") }}</label><PathField v-model="output" browse-variant="inline" @browse="browseOutput" />
      <label>{{ t("apkUpdate.target") }}</label><input v-model="target" class="target-input" :placeholder="t('apkUpdate.targetPlaceholder')" />
      <AppButton variant="primary" :disabled="busy" @click="createFirmware">{{ t("apkUpdate.create") }}</AppButton>
      <div v-if="status" class="result" :class="{ 'result--error': failed }">{{ status }}</div>
    </div>
  </div>
</template>

<style scoped>
.apk-page { width: 100%; max-width: 652px; }
.card { display: grid; grid-template-columns: 92px minmax(0, 1fr); align-items: center; gap: 16px; padding: 20px; border: 1px solid var(--color-border); border-radius: var(--border-radius-lg); background: var(--color-surface); }
label { font-size: 13px; color: var(--color-text-secondary); }
.target-input { width: 100%; height: 36px; padding: 0 12px; border: 1px solid var(--color-border); border-radius: var(--border-radius-md); background: var(--color-surface); color: var(--color-text-primary); font-size: 13px; }
.card :deep(.app-button) { grid-column: 2; justify-self: end; }
.result { grid-column: 2; font-size: 12px; color: var(--color-log-success); overflow-wrap: anywhere; }
.result--error { color: #dc2626; }
</style>
