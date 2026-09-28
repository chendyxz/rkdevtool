<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";
import AppButton from "./AppButton.vue";
import { useAppUpdater } from "../../composables/useAppUpdater";
import { useI18n } from "../../i18n";

const {
  prompt,
  updating,
  progressText,
  errorText,
  releaseUrl,
  installUpdate,
  dismissPrompt,
} = useAppUpdater();
const { t } = useI18n();

const expanded = ref(false);

// 摘要为空（比如发布说明里一条要点都没有）时直接摊开全文，别让弹窗空着
const showFull = computed(() => expanded.value || (prompt.value?.summary.length ?? 0) === 0);
const canExpand = computed(
  () => Boolean(prompt.value?.truncated && prompt.value.summary.length && prompt.value.full),
);

watch(prompt, (value) => {
  expanded.value = value?.summary.length === 0;
});

function close() {
  dismissPrompt();
}

async function openReleasePage() {
  const url = releaseUrl.value;
  if (!url) return;
  try {
    await openUrl(url);
  } catch {
    window.open(url, "_blank", "noopener,noreferrer");
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key !== "Escape") return;
  event.preventDefault();
  close();
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <div v-if="prompt" class="update-overlay" @click.self="close">
    <div class="update-dialog" role="dialog" aria-modal="true">
      <header class="update-dialog__header">
        <span>{{ t("update.title") }}</span>
      </header>

      <p class="update-dialog__intro">
        {{ t("update.available", { version: prompt.version }) }}
      </p>

      <!-- 说明区是唯一会长高的部分，滚动条留给它，按钮永远钉在底部 -->
      <div class="update-dialog__notes">
        <ul v-if="prompt.summary.length" class="update-dialog__summary">
          <li v-for="(line, index) in prompt.summary" :key="index">{{ line }}</li>
        </ul>
        <pre v-if="showFull && prompt.full" class="update-dialog__full">{{ prompt.full }}</pre>
      </div>

      <div class="update-dialog__links">
        <button v-if="canExpand" type="button" class="update-dialog__link" @click="expanded = !expanded">
          {{ expanded ? t("update.collapseNotes") : t("update.expandNotes") }}
        </button>
        <button type="button" class="update-dialog__link" @click="openReleasePage">
          {{ t("update.openRelease") }}
        </button>
      </div>

      <p v-if="errorText" class="update-dialog__error">
        {{ t("update.failed", { error: errorText }) }}
      </p>

      <footer class="update-dialog__footer">
        <span class="update-dialog__status">{{ updating ? progressText : "" }}</span>
        <AppButton size="sm" :disabled="updating" @click="close">{{ t("update.later") }}</AppButton>
        <AppButton size="sm" variant="primary" :disabled="updating" @click="installUpdate">
          {{ updating ? t("update.working") : t("update.install") }}
        </AppButton>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.update-overlay {
  position: fixed;
  z-index: 1300;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(15, 23, 42, 0.72);
}

.update-dialog {
  display: flex;
  flex-direction: column;
  gap: 12px;
  width: 100%;
  max-width: 560px;
  max-height: 100%;
  padding: 16px;
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-lg);
  background: var(--color-surface);
  box-shadow: 0 18px 48px rgba(15, 23, 42, 0.45);
  overflow: hidden;
}

.update-dialog__header {
  flex: none;
  color: var(--color-text-primary);
  font-size: 13px;
  font-weight: 600;
}

.update-dialog__intro {
  flex: none;
  margin: 0;
  color: var(--color-text-secondary);
  font-size: 12px;
}

/* 唯一允许收缩（也唯一滚动）的区域，窗口再矮也压不到底部按钮 */
.update-dialog__notes {
  flex: 1;
  min-height: 96px;
  max-height: min(40vh, 320px);
  padding: 12px;
  border: 1px solid var(--color-border);
  border-radius: 8px;
  background: var(--color-surface-hover);
  color: var(--color-text-secondary);
  font-size: 12px;
  line-height: 1.6;
  overflow: auto;
}

.update-dialog__summary {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 0;
  padding-left: 18px;
}

.update-dialog__full {
  margin: 0;
  font-family: inherit;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.update-dialog__summary + .update-dialog__full {
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px solid var(--color-border);
}

.update-dialog__links {
  flex: none;
  display: flex;
  gap: 16px;
}

.update-dialog__link {
  padding: 0;
  border: none;
  background: none;
  color: var(--color-primary);
  font-size: 12px;
}

.update-dialog__link:hover {
  text-decoration: underline;
}

.update-dialog__error {
  flex: none;
  margin: 0;
  color: var(--color-danger);
  font-size: 12px;
  overflow-wrap: anywhere;
}

.update-dialog__footer {
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
}

.update-dialog__status {
  margin-right: auto;
  color: var(--color-text-secondary);
  font-size: 11px;
}
</style>
