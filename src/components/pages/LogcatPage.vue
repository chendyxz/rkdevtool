<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, shallowRef, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import AppButton from "../ui/AppButton.vue";
import { useAppState } from "../../composables/useAppState";
import { useI18n } from "../../i18n";
import { toolApi } from "../../composables/useToolCommand";
import type { LogcatLinesEvent } from "../../types/tool";

type LogLevel = "V" | "D" | "I" | "W" | "E" | "A" | "";

interface LogcatEntry {
  id: number;
  raw: string;
  time: string;
  pid: string;
  tid: string;
  level: LogLevel;
  tag: string;
  message: string;
}

const MAX_VISIBLE_LINES = 5_000;
const MAX_RENDERED_LINES = 1_500;
const LEVEL_ORDER: Record<LogLevel, number> = { "": 0, V: 1, D: 2, I: 3, W: 4, E: 5, A: 6 };
const LOGCAT_RE = /^(\d{2}-\d{2})\s+(\d{2}:\d{2}:\d{2}\.\d+)\s+(\d+)\s+(\d+)\s+([VDIWEAF])\s+(.+?):\s?(.*)$/;

const lines = shallowRef<LogcatEntry[]>([]);
const running = ref(false);
const paused = ref(false);
const autoScroll = ref(true);
const fullscreen = ref(false);
const level = ref<LogLevel>("");
const tagFilter = ref("");
const pidFilter = ref("");
const searchFilter = ref("");
const viewport = ref<HTMLElement | null>(null);
const { adbSerial, appendLog } = useAppState();
const { t } = useI18n();
const unlisteners: UnlistenFn[] = [];
let lineId = 0;
let pending: string[] = [];
let flushTimer: ReturnType<typeof setTimeout> | null = null;

function parseLine(raw: string): LogcatEntry {
  const match = raw.match(LOGCAT_RE);
  if (!match) {
    return { id: ++lineId, raw, time: "", pid: "", tid: "", level: "", tag: "", message: raw };
  }
  return {
    id: ++lineId,
    raw,
    time: `${match[1]} ${match[2]}`,
    pid: match[3],
    tid: match[4],
    level: match[5] as LogLevel,
    tag: match[6].trim(),
    message: match[7],
  };
}

const filteredLines = computed(() => {
  const minimum = LEVEL_ORDER[level.value];
  const tag = tagFilter.value.trim().toLowerCase();
  const pid = pidFilter.value.trim();
  const search = searchFilter.value.trim().toLowerCase();
  const result = lines.value.filter((line) => {
    if (level.value && LEVEL_ORDER[line.level] < minimum) return false;
    if (tag && !line.tag.toLowerCase().includes(tag)) return false;
    if (pid && line.pid !== pid) return false;
    if (search && !line.raw.toLowerCase().includes(search)) return false;
    return true;
  });
  return result.slice(-MAX_RENDERED_LINES);
});

function queueLines(rawLines: string[]) {
  if (paused.value) return;
  pending.push(...rawLines);
  if (flushTimer !== null) return;
  flushTimer = setTimeout(() => {
    const next = pending.map(parseLine);
    pending = [];
    flushTimer = null;
    lines.value = [...lines.value, ...next].slice(-MAX_VISIBLE_LINES);
  }, 80);
}

async function scrollToBottom() {
  if (!autoScroll.value) return;
  await nextTick();
  if (viewport.value) viewport.value.scrollTop = viewport.value.scrollHeight;
}

watch(
  () => filteredLines.value[filteredLines.value.length - 1]?.id,
  scrollToBottom,
);

async function start() {
  if (!adbSerial.value) {
    appendLog(t("logcat.noDevice"), "error");
    return;
  }
  try {
    lines.value = [];
    await toolApi.startLogcat(adbSerial.value);
    running.value = true;
    appendLog(t("logcat.running"), "info");
  } catch (error) {
    appendLog(String(error), "error");
  }
}

async function stop() {
  try {
    await toolApi.stopLogcat();
    running.value = false;
  } catch (error) {
    appendLog(String(error), "error");
  }
}

async function clear() {
  lines.value = [];
  pending = [];
  await toolApi.clearLogcat();
}

async function exportLogs() {
  const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
  const path = await save({
    title: t("logcat.exportTitle"),
    defaultPath: `logcat-${timestamp}.txt`,
    filters: [{ name: "Logcat", extensions: ["txt", "log"] }],
  });
  if (!path) return;
  try {
    const count = await toolApi.exportLogcat(path);
    appendLog(t("logcat.exportSuccess", { count: String(count), path }), "success");
  } catch (error) {
    appendLog(String(error), "error");
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape" && fullscreen.value) fullscreen.value = false;
}

onMounted(async () => {
  window.addEventListener("keydown", onKeydown);
  unlisteners.push(
    await listen<LogcatLinesEvent>("logcat-lines", (event) => queueLines(event.payload.lines)),
    await listen<string>("logcat-error", (event) => appendLog(event.payload, "error")),
    await listen("logcat-stopped", () => { running.value = false; }),
  );
  if (adbSerial.value) await start();
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown);
  if (flushTimer !== null) clearTimeout(flushTimer);
  unlisteners.forEach((unlisten) => unlisten());
  void toolApi.stopLogcat();
});
</script>

<template>
  <div class="logcat-page" :class="{ 'logcat-page--fullscreen': fullscreen }">
    <div class="logcat-toolbar">
      <div class="logcat-toolbar__actions">
        <AppButton v-if="!running" variant="primary" size="sm" @click="start">{{ t("logcat.start") }}</AppButton>
        <AppButton v-else size="sm" @click="stop">{{ t("logcat.stop") }}</AppButton>
        <AppButton size="sm" @click="paused = !paused">{{ paused ? t("logcat.resume") : t("logcat.pause") }}</AppButton>
        <AppButton size="sm" @click="clear">{{ t("logcat.clear") }}</AppButton>
        <AppButton size="sm" @click="exportLogs">{{ t("logcat.export") }}</AppButton>
        <AppButton size="sm" @click="fullscreen = !fullscreen">
          {{ fullscreen ? t("logcat.exitFullscreen") : t("logcat.fullscreen") }}
        </AppButton>
      </div>

      <div class="logcat-filters">
        <label>
          <span>{{ t("logcat.level") }}</span>
          <select v-model="level">
            <option value="">{{ t("logcat.allLevels") }}</option>
            <option v-for="item in ['V', 'D', 'I', 'W', 'E', 'A']" :key="item" :value="item">{{ item }}</option>
          </select>
        </label>
        <label>
          <span>{{ t("logcat.tag") }}</span>
          <input v-model="tagFilter" :placeholder="t('logcat.tagPlaceholder')" />
        </label>
        <label class="pid-filter">
          <span>{{ t("logcat.pid") }}</span>
          <input v-model="pidFilter" :placeholder="t('logcat.pidPlaceholder')" inputmode="numeric" />
        </label>
        <label class="search-filter">
          <span>{{ t("logcat.search") }}</span>
          <input v-model="searchFilter" :placeholder="t('logcat.searchPlaceholder')" />
        </label>
        <label class="auto-scroll">
          <input v-model="autoScroll" type="checkbox" />
          <span>{{ t("logcat.autoScroll") }}</span>
        </label>
      </div>
    </div>

    <div ref="viewport" class="logcat-view">
      <div class="logcat-head">
        <span>Time</span><span>PID</span><span>TID</span><span>Level</span><span>Tag</span><span>Message</span>
      </div>
      <div v-if="filteredLines.length" class="logcat-lines">
        <div v-for="line in filteredLines" :key="line.id" class="logcat-line" :class="`logcat-line--${line.level || 'system'}`">
          <span class="logcat-cell logcat-cell--time">{{ line.time }}</span>
          <span class="logcat-cell logcat-cell--pid">{{ line.pid }}</span>
          <span class="logcat-cell logcat-cell--tid">{{ line.tid }}</span>
          <span class="logcat-cell logcat-cell--level">{{ line.level }}</span>
          <span class="logcat-cell logcat-cell--tag" :title="line.tag">{{ line.tag }}</span>
          <span class="logcat-cell logcat-cell--message">{{ line.message }}</span>
        </div>
      </div>
      <div v-else class="logcat-empty">{{ lines.length ? t("logcat.filteredEmpty") : t("logcat.empty") }}</div>
    </div>
  </div>
</template>

<style scoped>
.logcat-page { display: flex; flex: 1; flex-direction: column; gap: 12px; width: 100%; min-height: 0; }
.logcat-page--fullscreen { position: fixed; z-index: 1000; inset: 0; padding: 16px; background: var(--color-bg); }
.logcat-toolbar { display: flex; flex-direction: column; gap: 12px; padding: 12px; border: 1px solid var(--color-border); border-radius: var(--border-radius-lg); background: var(--color-surface); }
.logcat-toolbar__actions { display: flex; flex-wrap: wrap; gap: 8px; }
.logcat-filters { display: grid; grid-template-columns: 130px minmax(120px, 180px) 100px minmax(180px, 1fr) auto; gap: 10px; align-items: end; }
.logcat-filters label { display: flex; flex-direction: column; gap: 5px; color: var(--color-text-secondary); font-size: 11px; }
.logcat-filters input, .logcat-filters select { width: 100%; height: 32px; padding: 0 9px; border: 1px solid var(--color-border); border-radius: 6px; background: var(--color-surface-hover); color: var(--color-text-primary); }
.logcat-filters .auto-scroll { flex-direction: row; align-items: center; height: 32px; white-space: nowrap; }
.auto-scroll input { width: 15px; height: 15px; accent-color: var(--color-primary); }
.logcat-view { flex: 1; min-height: 280px; overflow: auto; border: 1px solid #334155; border-radius: 8px; background: #111827; font-family: var(--font-family-mono); font-size: 11px; }
.logcat-head, .logcat-line { display: grid; grid-template-columns: 145px 54px 54px 42px minmax(110px, 180px) minmax(360px, 1fr); min-width: 900px; }
.logcat-head { position: sticky; z-index: 2; top: 0; background: #1f2937; color: #9ca3af; border-bottom: 1px solid #374151; font-weight: 600; }
.logcat-head span, .logcat-cell { padding: 3px 7px; border-right: 1px solid rgba(75, 85, 99, 0.35); white-space: pre; }
.logcat-line { min-height: 22px; border-bottom: 1px solid rgba(55, 65, 81, 0.35); color: #d1d5db; line-height: 16px; }
.logcat-line:hover { background: #263244; }
.logcat-cell--tag { overflow: hidden; text-overflow: ellipsis; }
.logcat-cell--message { white-space: pre-wrap; overflow-wrap: anywhere; }
.logcat-line--V { color: #9ca3af; }
.logcat-line--D { color: #60a5fa; }
.logcat-line--I { color: #6ee7b7; }
.logcat-line--W { color: #fbbf24; background: rgba(146, 64, 14, 0.12); }
.logcat-line--E { color: #fb7185; background: rgba(153, 27, 27, 0.16); }
.logcat-line--A { color: #f0abfc; background: rgba(134, 25, 143, 0.16); font-weight: 600; }
.logcat-line--system { color: #c4b5fd; font-style: italic; }
.logcat-empty { display: flex; min-height: 240px; align-items: center; justify-content: center; color: #6b7280; }
@media (max-width: 1180px) { .logcat-filters { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
