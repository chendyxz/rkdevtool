<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from "vue";
import ScrollBar from "../ui/ScrollBar.vue";
import { useAppState } from "../../composables/useAppState";
import { useI18n } from "../../i18n";

const { logs, clearLogs } = useAppState();
const { t } = useI18n();

const contentRef = ref<HTMLElement | null>(null);
const stickToBottom = ref(true);
const LOG_WIDTH_KEY = "rkdevtool.log-panel-width.v1";
const LOG_WRAP_KEY = "rkdevtool.log-wrap.v1";
const savedWidth = Number(localStorage.getItem(LOG_WIDTH_KEY));
const panelWidth = ref(Number.isFinite(savedWidth) && savedWidth >= 260 ? savedWidth : 360);
const wrapLines = ref(localStorage.getItem(LOG_WRAP_KEY) === "true");
let resizeStartX = 0;
let resizeStartWidth = 0;

const levelClass = computed(() => (level: string) => `log-line--${level}`);

function onScroll() {
  const el = contentRef.value;
  if (!el) return;
  stickToBottom.value = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
}

async function scrollToBottom() {
  await nextTick();
  const el = contentRef.value;
  if (!el || !stickToBottom.value) return;
  el.scrollTop = el.scrollHeight;
}

watch(
  () => `${logs.value.length}:${logs.value[logs.value.length - 1]?.text ?? ""}`,
  () => {
    scrollToBottom();
  },
);

function handleClear() {
  clearLogs();
  stickToBottom.value = true;
  scrollToBottom();
}

function toggleWrap() {
  wrapLines.value = !wrapLines.value;
  localStorage.setItem(LOG_WRAP_KEY, String(wrapLines.value));
}

function clampWidth(width: number) {
  return Math.max(260, Math.min(width, window.innerWidth * 0.75));
}

function onResize(event: PointerEvent) {
  panelWidth.value = clampWidth(resizeStartWidth + resizeStartX - event.clientX);
}

function stopResize() {
  window.removeEventListener("pointermove", onResize);
  window.removeEventListener("pointerup", stopResize);
  document.body.style.cursor = "";
  document.body.style.userSelect = "";
  localStorage.setItem(LOG_WIDTH_KEY, String(Math.round(panelWidth.value)));
}

function startResize(event: PointerEvent) {
  resizeStartX = event.clientX;
  resizeStartWidth = panelWidth.value;
  document.body.style.cursor = "col-resize";
  document.body.style.userSelect = "none";
  window.addEventListener("pointermove", onResize);
  window.addEventListener("pointerup", stopResize);
}

onUnmounted(stopResize);
</script>

<template>
  <aside class="log-panel" :style="{ width: `${panelWidth}px`, flexBasis: `${panelWidth}px` }">
    <div class="log-panel__resize" @pointerdown="startResize" />
    <header class="log-panel__header">
      <span>{{ t("log.title") }}</span>
      <div class="log-panel__actions">
        <button type="button" class="log-panel__clear" :class="{ 'log-panel__button--active': wrapLines }" @click="toggleWrap">
          {{ wrapLines ? t("log.noWrap") : t("log.wrap") }}
        </button>
        <button type="button" class="log-panel__clear" @click="handleClear">{{ t("log.clear") }}</button>
      </div>
    </header>
    <div class="log-panel__viewport">
      <div ref="contentRef" class="log-panel__content" :class="{ 'log-panel__content--wrap': wrapLines }" @scroll="onScroll">
        <p
          v-for="entry in logs"
          :key="entry.id"
          class="log-line"
          :class="[levelClass(entry.level), entry.kind === 'progress' && 'log-line--progress']"
        >
          {{ entry.text }}
        </p>
      </div>
      <ScrollBar :target="contentRef" />
    </div>
  </aside>
</template>

<style scoped>
.log-panel {
  position: relative;
  flex: 0 0 360px;
  min-width: 260px;
  min-height: 0;
  align-self: stretch;
  background: var(--color-log-bg);
  display: flex;
  flex-direction: column;
  /* 面板底始终是深色，声明 dark 才能得到浅色的系统滚动条 */
  color-scheme: dark;
}

.log-panel__resize {
  position: absolute;
  z-index: 2;
  top: 0;
  bottom: 0;
  left: -4px;
  width: 8px;
  cursor: col-resize;
}

.log-panel__resize:hover {
  background: rgba(59, 130, 246, 0.45);
}

.log-panel__header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 44px;
  padding: 0 16px;
  background: #1e293b;
  color: #94a3b8;
  font-size: 13px;
  font-weight: 600;
  flex-shrink: 0;
}

.log-panel__clear {
  border: none;
  background: transparent;
  color: #94a3b8;
  font-size: 12px;
  padding: 4px 8px;
  border-radius: 4px;
}

.log-panel__clear:hover {
  color: #e2e8f0;
  background: rgba(255, 255, 255, 0.06);
}

.log-panel__actions {
  display: flex;
  align-items: center;
  gap: 4px;
}

.log-panel__button--active {
  color: #60a5fa;
}

.log-panel__viewport {
  position: relative;
  display: flex;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.log-panel__content {
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow: auto;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  /* 原生滚动条交给 ScrollBar 自绘 */
  scrollbar-width: none;
}

.log-panel__content::-webkit-scrollbar {
  display: none;
}

.log-line {
  margin: 0;
  max-width: 100%;
  font-family: var(--font-family-mono);
  font-size: 12px;
  line-height: 18px;
  color: var(--color-log-text);
  white-space: pre;
  overflow-wrap: normal;
  word-break: normal;
  flex-shrink: 0;
}

.log-line--progress {
  color: #fbbf24;
  white-space: pre;
  overflow-wrap: normal;
  word-break: normal;
}

.log-line--success {
  color: var(--color-log-success);
  font-weight: 600;
}

.log-line--info {
  color: var(--color-log-info);
}

.log-line--error {
  color: #f87171;
}

.log-panel__content--wrap .log-line,
.log-panel__content--wrap .log-line--progress {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  word-break: break-word;
}
</style>
