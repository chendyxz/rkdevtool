<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, shallowRef, triggerRef, watch } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import AppButton from "../ui/AppButton.vue";
import ScrollBar from "../ui/ScrollBar.vue";
import { useAppState } from "../../composables/useAppState";
import { useI18n } from "../../i18n";
import { toolApi } from "../../composables/useToolCommand";
import { useVirtualList } from "../../composables/useVirtualList";
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
const FLUSH_INTERVAL_MS = 80;
const FILTER_DEBOUNCE_MS = 120;
const LEVEL_ORDER: Record<LogLevel, number> = { "": 0, V: 1, D: 2, I: 3, W: 4, E: 5, A: 6 };
const LOGCAT_RE = /^(\d{2}-\d{2})\s+(\d{2}:\d{2}:\d{2}\.\d+)\s+(\d+)\s+(\d+)\s+([VDIWEAF])\s+(.+?):\s?(.*)$/;
const TOKEN_SPLIT_RE = /[,，、|\s]+/;

/** 多值输入按逗号/顿号/竖线/空白切分：同一个字段内取「或」，字段之间取「且」。 */
function parseTokens(input: string, lowerCase = true): string[] {
  const tokens = input.split(TOKEN_SPLIT_RE).filter((token) => token.length > 0);
  return lowerCase ? tokens.map((token) => token.toLowerCase()) : tokens;
}

const lines = shallowRef<LogcatEntry[]>([]);
const running = ref(false);
const paused = ref(false);
const autoScroll = ref(true);
const fullscreen = ref(false);
const level = ref<LogLevel>("");
const tagInput = ref("");
const pidInput = ref("");
const searchInput = ref("");
const tagFilter = ref("");
const pidFilter = ref("");
const searchFilter = ref("");
const viewport = ref<HTMLElement | null>(null);
const headRef = ref<HTMLElement | null>(null);
const { adbSerial, appendLog } = useAppState();
const { t } = useI18n();
const unlisteners: UnlistenFn[] = [];
let lineId = 0;
let pending: string[] = [];
let flushTimer: ReturnType<typeof setTimeout> | null = null;
let filterTimer: ReturnType<typeof setTimeout> | null = null;

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

const tagTokens = computed(() => parseTokens(tagFilter.value));
const pidTokens = computed(() => parseTokens(pidFilter.value, false));

/**
 * 筛选结果。虚拟滚动只渲染视口内的行，所以这里可以放心全量过滤；
 * 始终返回新的数组引用，保证虚拟列表能感知内容变化。
 * Tag / PID 支持多值：字段内命中任意一个即通过，字段之间需同时满足。
 */
const filteredLines = computed<LogcatEntry[]>(() => {
  const list = lines.value;
  const minimum = LEVEL_ORDER[level.value];
  const tags = tagTokens.value;
  const pids = pidTokens.value;
  const search = searchFilter.value.trim().toLowerCase();
  if (!level.value && tags.length === 0 && pids.length === 0 && !search) return list.slice();
  return list.filter((line) => {
    if (level.value && LEVEL_ORDER[line.level] < minimum) return false;
    if (tags.length > 0) {
      const tag = line.tag.toLowerCase();
      if (!tags.some((token) => tag.includes(token))) return false;
    }
    if (pids.length > 0 && !pids.includes(line.pid)) return false;
    if (search && !line.raw.toLowerCase().includes(search)) return false;
    return true;
  });
});

const { visibleItems, offsetTop, totalHeight, atBottom, onScroll, scrollToBottom, refresh } = useVirtualList(
  viewport,
  () => filteredLines.value,
  { header: headRef, estimateHeight: 23, overscan: 12 },
);

// 输入框即时回显，筛选条件合并后延迟应用，避免每个按键都触发一次全量过滤
watch([tagInput, pidInput, searchInput], () => {
  if (filterTimer !== null) clearTimeout(filterTimer);
  filterTimer = setTimeout(() => {
    filterTimer = null;
    tagFilter.value = tagInput.value;
    pidFilter.value = pidInput.value;
    searchFilter.value = searchInput.value;
  }, FILTER_DEBOUNCE_MS);
});

// 新日志到达时，仅在用户停留在底部且开启自动滚动的情况下跟随
watch(
  () => filteredLines.value.length,
  () => {
    if (autoScroll.value && atBottom.value) scrollToBottom();
  },
);

watch(autoScroll, (next) => {
  if (next) scrollToBottom();
});

// 全屏切换会改变容器尺寸，需要重新计算可见范围
watch(fullscreen, () => {
  void nextTick(refresh);
});

function flushPending() {
  flushTimer = null;
  if (pending.length === 0) return;
  const parsed = pending.map((line) => parseLine(line));
  pending = [];
  const list = lines.value;
  for (const entry of parsed) list.push(entry);
  const overflow = list.length - MAX_VISIBLE_LINES;
  if (overflow > 0) list.splice(0, overflow);
  triggerRef(lines);
}

function queueLines(rawLines: string[]) {
  if (paused.value) return;
  for (const line of rawLines) pending.push(line);
  if (flushTimer !== null) return;
  flushTimer = setTimeout(flushPending, FLUSH_INTERVAL_MS);
}

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
  if (flushTimer !== null) {
    clearTimeout(flushTimer);
    flushTimer = null;
  }
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
  if (filterTimer !== null) clearTimeout(filterTimer);
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
          <input v-model="tagInput" :placeholder="t('logcat.tagPlaceholder')" :title="t('logcat.multiValueHint')" />
        </label>
        <label class="pid-filter">
          <span>{{ t("logcat.pid") }}</span>
          <input v-model="pidInput" :placeholder="t('logcat.pidPlaceholder')" :title="t('logcat.multiValueHint')" />
        </label>
        <label class="search-filter">
          <span>{{ t("logcat.search") }}</span>
          <input v-model="searchInput" :placeholder="t('logcat.searchPlaceholder')" />
        </label>
        <label class="auto-scroll">
          <input v-model="autoScroll" type="checkbox" />
          <span>{{ t("logcat.autoScroll") }}</span>
        </label>
      </div>
    </div>

    <div class="logcat-viewport">
      <div ref="viewport" class="logcat-view" @scroll.passive="onScroll">
        <div ref="headRef" class="logcat-head">
          <span>Time</span><span>PID</span><span>TID</span><span>Level</span><span>Tag</span><span>Message</span>
        </div>
        <div v-if="filteredLines.length" class="logcat-canvas" :style="{ height: `${totalHeight}px` }">
          <div class="logcat-window" :style="{ transform: `translateY(${offsetTop}px)` }">
            <div
              v-for="line in visibleItems"
              :key="line.id"
              :data-vrow="line.id"
              class="logcat-line"
              :class="`logcat-line--${line.level || 'system'}`"
            >
              <span class="logcat-cell logcat-cell--time">{{ line.time }}</span>
              <span class="logcat-cell logcat-cell--pid">{{ line.pid }}</span>
              <span class="logcat-cell logcat-cell--tid">{{ line.tid }}</span>
              <span class="logcat-cell logcat-cell--level">{{ line.level }}</span>
              <span class="logcat-cell logcat-cell--tag" :title="line.tag">{{ line.tag }}</span>
              <span class="logcat-cell logcat-cell--message">{{ line.message }}</span>
            </div>
          </div>
        </div>
        <div v-else class="logcat-empty">{{ lines.length ? t("logcat.filteredEmpty") : t("logcat.empty") }}</div>
      </div>
      <ScrollBar :target="viewport" />
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
.logcat-viewport {
  position: relative;
  display: flex;
  flex: 1;
  min-height: 280px;
}
.logcat-view {
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow: auto;
  overscroll-behavior: contain;
  /* 底部留出空间，避免横向滚动条压住最后一行日志 */
  padding-bottom: 14px;
  border: 1px solid #334155;
  border-radius: 8px;
  background: #111827;
  font-family: var(--font-family-mono);
  font-size: 11px;
  /* 日志底始终是深色，声明 dark 才能让光标/选区等原生元素也按深色绘制 */
  color-scheme: dark;
  /* 原生滚动条交给 ScrollBar 自绘 */
  scrollbar-width: none;
}
.logcat-view::-webkit-scrollbar {
  display: none;
}
.logcat-head, .logcat-line { display: grid; grid-template-columns: 145px 54px 54px 42px minmax(110px, 180px) minmax(360px, 1fr); min-width: 900px; }
.logcat-head { position: sticky; z-index: 2; top: 0; background: #1f2937; color: #9ca3af; border-bottom: 1px solid #374151; font-weight: 600; }
.logcat-canvas { position: relative; min-width: 900px; }
.logcat-window { position: absolute; top: 0; left: 0; right: 0; will-change: transform; }
.logcat-head span, .logcat-cell { padding: 3px 7px; border-right: 1px solid rgba(75, 85, 99, 0.35); white-space: pre; }
.logcat-line { min-height: 22px; border-bottom: 1px solid rgba(55, 65, 81, 0.35); color: #d1d5db; line-height: 16px; }
.logcat-line:hover { background: #263244; }
.logcat-cell--tag { overflow: hidden; text-overflow: ellipsis; }
.logcat-cell--message { white-space: pre-wrap; overflow-wrap: anywhere; padding-right: 18px; }
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
