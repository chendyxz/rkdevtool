<script setup lang="ts">
import { computed, reactive, ref } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import AppButton from "../ui/AppButton.vue";
import PathField from "../ui/PathField.vue";
import { useAppState } from "../../composables/useAppState";
import { useToolCommand, toolApi } from "../../composables/useToolCommand";
import { pickFile } from "../../composables/useFilePicker";
import type { BurnParameter } from "../../api/tool";
import { useI18n } from "../../i18n";

type ParameterKind = BurnParameter["kind"];

interface ParameterRow {
  kind: ParameterKind;
  name: string;
  value: string;
  selected: boolean;
}

const rows = reactive<ParameterRow[]>([
  { kind: "dn", name: "DN", value: "", selected: true },
  { kind: "ds", name: "DS", value: "", selected: true },
  { kind: "pk", name: "PK", value: "", selected: true },
  { kind: "voiceKey", name: "VoiceKey", value: "", selected: true },
]);
const toolPath = ref("");

const { adbSerial, appendLog, busy } = useAppState();
const { run } = useToolCommand();
const { t } = useI18n();
const selectedCount = computed(() => rows.filter((row) => row.selected).length);

async function burn(targets: ParameterRow[]) {
  if (!adbSerial.value) {
    appendLog(t("burnParameters.noDevice"), "error");
    return;
  }
  if (targets.length === 0) {
    appendLog(t("burnParameters.selectAtLeastOne"), "error");
    return;
  }
  if (!toolPath.value.trim()) {
    appendLog(t("burnParameters.selectToolFirst"), "error");
    return;
  }

  const empty = targets.find((row) => !row.value.trim());
  if (empty) {
    appendLog(t("burnParameters.emptyValue", { name: empty.name }), "error");
    return;
  }

  const names = targets.map((row) => row.name).join("、");
  const confirmed = await ask(t("burnParameters.confirm", { names }), {
    title: t("burnParameters.confirmTitle"),
    kind: "warning",
  });
  if (!confirmed) return;

  try {
    const result = await run(() =>
      toolApi.burnParameters(
        adbSerial.value!,
        toolPath.value.trim(),
        targets.map(({ kind, value }) => ({ kind, value: value.trim() })),
      ),
    );
    if (result) appendLog(result, "info");
    appendLog(t("burnParameters.success"), "success");
  } catch (error) {
    appendLog(String(error), "error");
  }
}

async function browseTool() {
  toolPath.value = (await pickFile(t("burnParameters.pickTool"))) ?? toolPath.value;
}
</script>

<template>
  <div class="burn-page">
    <div class="burn-card">
      <p class="burn-card__hint">{{ t("burnParameters.hint") }}</p>

      <div class="tool-row">
        <span class="tool-row__label">read_write_id</span>
        <PathField
          v-model="toolPath"
          browse-variant="inline"
          :placeholder="t('burnParameters.toolPlaceholder')"
          @browse="browseTool"
        />
      </div>

      <div class="burn-table">
        <div class="burn-table__head">{{ t("burnParameters.selected") }}</div>
        <div class="burn-table__head">{{ t("burnParameters.value") }}</div>
        <div class="burn-table__head" />

        <template v-for="row in rows" :key="row.kind">
          <label class="burn-row__check">
            <input v-model="row.selected" type="checkbox" :disabled="busy" />
            <span>{{ row.name }}</span>
          </label>
          <input
            v-model="row.value"
            class="burn-row__input"
            type="text"
            :placeholder="row.name"
            :disabled="busy"
            autocomplete="off"
            spellcheck="false"
          />
          <AppButton size="sm" :disabled="busy || !row.value.trim()" @click="burn([row])">
            {{ t("burnParameters.burnOne") }}
          </AppButton>
        </template>
      </div>

      <div class="burn-card__footer">
        <AppButton
          variant="primary"
          :disabled="busy || selectedCount === 0"
          @click="burn(rows.filter((row) => row.selected))"
        >
          {{ t("burnParameters.burnSelected") }}（{{ selectedCount }}）
        </AppButton>
      </div>
    </div>
  </div>
</template>

<style scoped>
.burn-page { width: 100%; max-width: 652px; }
.burn-card { padding: 20px; border: 1px solid var(--color-border); border-radius: var(--border-radius-lg); background: var(--color-surface); }
.burn-card__hint { margin: 0 0 18px; color: var(--color-text-secondary); line-height: 1.6; }
.tool-row { display: grid; grid-template-columns: 116px minmax(0, 1fr); align-items: center; gap: 12px; margin-bottom: 20px; }
.tool-row__label { font-weight: 600; }
.burn-table { display: grid; grid-template-columns: 116px minmax(0, 1fr) auto; gap: 12px; align-items: center; }
.burn-table__head { padding-bottom: 2px; color: var(--color-text-muted); font-size: 12px; }
.burn-row__check { display: flex; align-items: center; gap: 9px; font-weight: 600; }
.burn-row__check input { width: 16px; height: 16px; accent-color: var(--color-primary); }
.burn-row__input { width: 100%; height: 36px; padding: 0 12px; border: 1px solid var(--color-border); border-radius: var(--border-radius-md); background: var(--color-surface); color: var(--color-text-primary); }
.burn-card__footer { display: flex; justify-content: flex-end; margin-top: 20px; padding-top: 16px; border-top: 1px solid var(--color-border); }
</style>
