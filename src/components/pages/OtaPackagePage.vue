<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import AppButton from "../ui/AppButton.vue";
import PathField from "../ui/PathField.vue";
import { pickFile, pickSavePath } from "../../composables/useFilePicker";
import { useAppState } from "../../composables/useAppState";
import { useToolCommand, toolApi } from "../../composables/useToolCommand";
import { useI18n } from "../../i18n";

type OtaKind = "apk" | "rom";

interface OtaForm {
  source: string;
  version: string;
  pk: string;
  output: string;
}

const CONFIG_KEY = "rkdevtool.ota-package-config.v1";
const ZIP_FILTER = { name: "OTA zip", extensions: ["zip"] };

function emptyForm(): OtaForm {
  return { source: "", version: "", pk: "", output: "" };
}

function readForm(value: unknown): OtaForm {
  const raw = (value ?? {}) as Partial<OtaForm>;
  return {
    source: typeof raw.source === "string" ? raw.source : "",
    version: typeof raw.version === "string" ? raw.version : "",
    pk: typeof raw.pk === "string" ? raw.pk : "",
    output: typeof raw.output === "string" ? raw.output : "",
  };
}

function loadConfig() {
  try {
    const saved = JSON.parse(localStorage.getItem(CONFIG_KEY) ?? "null");
    if (saved && typeof saved === "object") {
      return {
        mode: saved.mode === "rom" ? ("rom" as OtaKind) : ("apk" as OtaKind),
        apk: readForm(saved.apk),
        rom: readForm(saved.rom),
      };
    }
  } catch {
    // Ignore invalid or obsolete local data.
  }
  return { mode: "apk" as OtaKind, apk: emptyForm(), rom: emptyForm() };
}

const config = reactive(loadConfig());
const status = ref("");
const failed = ref(false);
const { appendLog, busy } = useAppState();
const { run } = useToolCommand();
const { t } = useI18n();

const form = computed(() => config[config.mode]);
const sourceLabel = computed(() => t(config.mode === "apk" ? "otaPackage.apk" : "otaPackage.rom"));
const versionLabel = computed(() =>
  t(config.mode === "apk" ? "otaPackage.apkVersion" : "otaPackage.romVersion"),
);
const sourceHint = computed(() =>
  t(config.mode === "apk" ? "otaPackage.apkPlaceholder" : "otaPackage.romPlaceholder"),
);
/** 展示即将写入 version.json 的内容，方便确认格式。 */
const versionPreview = computed(() => {
  const key = config.mode === "apk" ? "apkVersion" : "romVersion";
  const version = form.value.version.trim() || "...";
  const pk = form.value.pk.trim() || "...";
  return `{"${key}":"${version}","pk": "${pk}"}`;
});

watch(
  config,
  () => localStorage.setItem(CONFIG_KEY, JSON.stringify(config)),
  { deep: true, flush: "sync" },
);

function sourceDir(path: string): string {
  const index = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return index > 0 ? path.slice(0, index) : "";
}

function suggestedOutput(kind: OtaKind, version: string, source: string) {
  const prefix = kind === "apk" ? "apk-ota" : "rom-ota";
  const value = version.trim();
  const name = value ? `${prefix}-${value}.zip` : `${prefix}.zip`;
  const dir = sourceDir(source.trim());
  return dir ? `${dir}/${name}` : name;
}

async function browseSource() {
  const target = config[config.mode];
  const title = t(config.mode === "apk" ? "otaPackage.pickApk" : "otaPackage.pickRom");
  target.source = (await pickFile(title)) ?? target.source;
}

async function browseOutput() {
  const target = config[config.mode];
  const suggested = suggestedOutput(config.mode, target.version, target.source);
  target.output =
    (await pickSavePath(t("otaPackage.pickOutput"), suggested, ZIP_FILTER)) ?? target.output;
}

function fail(message: string) {
  failed.value = true;
  status.value = message;
  appendLog(message, "error");
}

async function build() {
  const mode = config.mode;
  const target = config[mode];
  const extension = mode === "apk" ? ".apk" : ".zip";

  if (!target.source.trim()) {
    fail(t("otaPackage.missingSource", { name: sourceLabel.value }));
    return;
  }
  if (!target.source.trim().toLowerCase().endsWith(extension)) {
    fail(t("otaPackage.invalidSource", { name: sourceLabel.value, extension }));
    return;
  }
  if (!target.version.trim()) {
    fail(t("otaPackage.missingVersion", { name: versionLabel.value }));
    return;
  }
  if (!target.pk.trim()) {
    fail(t("otaPackage.missingPk"));
    return;
  }
  if (!target.output.trim()) {
    fail(t("otaPackage.missingOutput"));
    return;
  }

  failed.value = false;
  status.value = t("otaPackage.running");
  try {
    const result = await run(() =>
      toolApi.buildOtaZip({
        kind: mode,
        version: target.version.trim(),
        pk: target.pk.trim(),
        source: target.source.trim(),
        output: target.output.trim(),
      }),
    );
    if (!result) return;
    status.value = t("otaPackage.success", { path: result });
    appendLog(status.value, "success");
  } catch (error) {
    fail(t("otaPackage.failed", { error: String(error) }));
  }
}
</script>

<template>
  <div class="ota-page">
    <div class="tab-row">
      <button
        type="button"
        class="tab"
        :class="{ 'tab--active': config.mode === 'apk' }"
        @click="config.mode = 'apk'"
      >
        {{ t("otaPackage.tabApk") }}
      </button>
      <button
        type="button"
        class="tab"
        :class="{ 'tab--active': config.mode === 'rom' }"
        @click="config.mode = 'rom'"
      >
        {{ t("otaPackage.tabRom") }}
      </button>
    </div>

    <div class="card">
      <p class="card__hint">{{ t("otaPackage.hint") }}</p>

      <label>{{ sourceLabel }}</label>
      <PathField v-model="form.source" browse-variant="inline" :placeholder="sourceHint" @browse="browseSource" />

      <label>{{ versionLabel }}</label>
      <input v-model="form.version" class="text-input" :placeholder="t('otaPackage.versionPlaceholder')" :disabled="busy" />

      <label>{{ t("otaPackage.pk") }}</label>
      <input v-model="form.pk" class="text-input" :placeholder="t('otaPackage.pkPlaceholder')" :disabled="busy" />

      <label>{{ t("otaPackage.output") }}</label>
      <PathField v-model="form.output" browse-variant="inline" :placeholder="t('otaPackage.outputPlaceholder')" @browse="browseOutput" />

      <label>{{ t("otaPackage.preview") }}</label>
      <code class="preview">{{ versionPreview }}</code>

      <AppButton variant="primary" :disabled="busy" @click="build">
        {{ t("otaPackage.create") }}
      </AppButton>

      <div v-if="status" class="result" :class="{ 'result--error': failed }">{{ status }}</div>
    </div>
  </div>
</template>

<style scoped>
.ota-page {
  width: 100%;
  max-width: 720px;
}

.tab-row {
  display: flex;
  gap: 8px;
  margin-bottom: 16px;
}

.tab {
  height: 32px;
  padding: 0 16px;
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-md);
  background: var(--color-surface);
  color: var(--color-text-secondary);
  font-size: 13px;
  font-weight: 600;
}

.tab--active {
  border-color: var(--color-primary);
  color: var(--color-primary);
  background: var(--color-surface-hover);
}

.card {
  display: grid;
  grid-template-columns: 92px minmax(0, 1fr);
  align-items: center;
  gap: 16px;
  padding: 20px;
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-lg);
  background: var(--color-surface);
}

.card__hint {
  grid-column: 1 / -1;
  margin: 0;
  font-size: 12px;
  line-height: 1.6;
  color: var(--color-text-secondary);
}

label {
  font-size: 13px;
  color: var(--color-text-secondary);
}

.text-input {
  width: 100%;
  height: 36px;
  padding: 0 12px;
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-md);
  background: var(--color-surface);
  color: var(--color-text-primary);
  font-size: 13px;
}

.preview {
  padding: 8px 12px;
  border-radius: var(--border-radius-md);
  background: var(--color-bg);
  color: var(--color-text-secondary);
  font-family: var(--font-family-mono);
  font-size: 12px;
  overflow-wrap: anywhere;
}

.card :deep(.app-button) {
  grid-column: 2;
  justify-self: end;
}

.result {
  grid-column: 2;
  font-size: 12px;
  color: var(--color-log-success);
  overflow-wrap: anywhere;
}

.result--error {
  color: #dc2626;
}
</style>
