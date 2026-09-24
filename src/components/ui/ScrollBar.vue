<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";

interface Props {
  /** 需要接管滚动条的滚动容器（原生滚动条应已用 CSS 隐藏） */
  target: HTMLElement | null;
  /**
   * 滑块最小长度（px）。
   * 按比例算出的滑块长度 = 轨道长度 × 视口/内容，
   * 日志缓冲区 5000 行时内容高十万像素，算出来只有几像素，根本抓不住，
   * 所以设一个下限，拖动映射仍然覆盖完整滚动范围。
   */
  minThumbSize?: number;
  /** 轨道厚度（px） */
  thickness?: number;
  /** 轨道与容器边缘的间距（px） */
  inset?: number;
}

const props = withDefaults(defineProps<Props>(), {
  minThumbSize: 28,
  thickness: 12,
  inset: 2,
});

type AxisName = "v" | "h";

interface AxisState {
  /** 滑块长度 */
  size: number;
  /** 滑块相对轨道起点的偏移 */
  offset: number;
  /** 轨道长度，为 0 表示该轴不需要滚动条 */
  track: number;
  /** 该轴的最大可滚动距离 */
  maxScroll: number;
}

function emptyAxis(): AxisState {
  return { size: 0, offset: 0, track: 0, maxScroll: 0 };
}

const vertical = ref<AxisState>(emptyAxis());
const horizontal = ref<AxisState>(emptyAxis());
const dragging = ref<AxisName | null>(null);
const vTrack = ref<HTMLElement | null>(null);
const hTrack = ref<HTMLElement | null>(null);

let attached: HTMLElement | null = null;
let frame: number | null = null;
let resizeObserver: ResizeObserver | null = null;
let mutationObserver: MutationObserver | null = null;
let drag: { axis: AxisName; origin: number; offset: number } | null = null;

const vTrackStyle = computed(() => ({
  top: `${props.inset}px`,
  right: `${props.inset}px`,
  width: `${props.thickness}px`,
  height: `${vertical.value.track}px`,
}));

const hTrackStyle = computed(() => ({
  left: `${props.inset}px`,
  bottom: `${props.inset}px`,
  height: `${props.thickness}px`,
  width: `${horizontal.value.track}px`,
}));

const vThumbStyle = computed(() => ({
  height: `${vertical.value.size}px`,
  transform: `translateY(${vertical.value.offset}px)`,
}));

const hThumbStyle = computed(() => ({
  width: `${horizontal.value.size}px`,
  transform: `translateX(${horizontal.value.offset}px)`,
}));

function measureAxis(
  scrollSize: number,
  clientSize: number,
  scrollPos: number,
  trackSize: number,
): AxisState {
  if (trackSize <= 0 || scrollSize <= clientSize + 1) return emptyAxis();
  const size = Math.min(
    trackSize,
    Math.max(props.minThumbSize, Math.round((trackSize * clientSize) / scrollSize)),
  );
  const maxOffset = Math.max(0, trackSize - size);
  const maxScroll = scrollSize - clientSize;
  return {
    size,
    offset: maxScroll > 0 ? Math.round((scrollPos / maxScroll) * maxOffset) : 0,
    track: trackSize,
    maxScroll,
  };
}

function sync() {
  const el = attached;
  if (!el) {
    vertical.value = emptyAxis();
    horizontal.value = emptyAxis();
    return;
  }
  const needV = el.scrollHeight > el.clientHeight + 1;
  const needH = el.scrollWidth > el.clientWidth + 1;
  // 两轴同时出现时给右下角留位，避免两个滑块叠在一起
  const corner = props.thickness + props.inset;
  const vTrackSize = Math.max(0, el.clientHeight - props.inset * 2 - (needH ? corner : 0));
  const hTrackSize = Math.max(0, el.clientWidth - props.inset * 2 - (needV ? corner : 0));
  vertical.value = measureAxis(el.scrollHeight, el.clientHeight, el.scrollTop, vTrackSize);
  horizontal.value = measureAxis(el.scrollWidth, el.clientWidth, el.scrollLeft, hTrackSize);
}

function scheduleSync() {
  if (frame !== null) return;
  frame = requestAnimationFrame(() => {
    frame = null;
    sync();
  });
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

function beginDrag(axis: AxisName, event: PointerEvent) {
  const state = axis === "v" ? vertical.value : horizontal.value;
  if (state.size <= 0) return;
  drag = {
    axis,
    origin: axis === "v" ? event.clientY : event.clientX,
    offset: state.offset,
  };
  dragging.value = axis;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  event.preventDefault();
  event.stopPropagation();
}

function moveDrag(event: PointerEvent) {
  const el = attached;
  if (!el || !drag) return;
  const axis = drag.axis;
  const state = axis === "v" ? vertical.value : horizontal.value;
  const maxOffset = Math.max(0, state.track - state.size);
  if (maxOffset <= 0 || state.maxScroll <= 0) return;
  const position = axis === "v" ? event.clientY : event.clientX;
  const next = clamp(drag.offset + position - drag.origin, 0, maxOffset);
  const scroll = (next / maxOffset) * state.maxScroll;
  if (axis === "v") el.scrollTop = scroll;
  else el.scrollLeft = scroll;
  sync();
}

function endDrag(event: PointerEvent) {
  const el = event.currentTarget as HTMLElement;
  if (el.hasPointerCapture(event.pointerId)) el.releasePointerCapture(event.pointerId);
  drag = null;
  dragging.value = null;
}

/** 点击轨道空白处翻页（滑块自身由 beginDrag 处理，这里用 .self 限定） */
function pageBy(axis: AxisName, event: PointerEvent) {
  const el = attached;
  const track = axis === "v" ? vTrack.value : hTrack.value;
  if (!el || !track) return;
  const state = axis === "v" ? vertical.value : horizontal.value;
  const rect = track.getBoundingClientRect();
  const position = axis === "v" ? event.clientY - rect.top : event.clientX - rect.left;
  const step = (axis === "v" ? el.clientHeight : el.clientWidth) * 0.9;
  const delta = position < state.offset ? -step : step;
  if (axis === "v") el.scrollTop += delta;
  else el.scrollLeft += delta;
  event.preventDefault();
}

function observeChildren(el: HTMLElement) {
  if (!resizeObserver) return;
  for (const child of Array.from(el.children)) resizeObserver.observe(child);
}

function detach() {
  attached?.removeEventListener("scroll", scheduleSync);
  attached = null;
  resizeObserver?.disconnect();
  resizeObserver = null;
  mutationObserver?.disconnect();
  mutationObserver = null;
  if (frame !== null) {
    cancelAnimationFrame(frame);
    frame = null;
  }
}

function attach(el: HTMLElement | null) {
  detach();
  if (!el) {
    vertical.value = emptyAxis();
    horizontal.value = emptyAxis();
    return;
  }
  attached = el;
  el.addEventListener("scroll", scheduleSync, { passive: true });

  if (typeof ResizeObserver !== "undefined") {
    resizeObserver = new ResizeObserver(scheduleSync);
    resizeObserver.observe(el);
    // 直接子元素的尺寸变化代表内容增减（虚拟列表的撑高容器也在其中）
    observeChildren(el);
  }
  if (typeof MutationObserver !== "undefined") {
    mutationObserver = new MutationObserver(() => {
      observeChildren(el);
      scheduleSync();
    });
    mutationObserver.observe(el, { childList: true });
  }
  scheduleSync();
}

watch(() => props.target, attach, { immediate: true });
onBeforeUnmount(detach);
</script>

<template>
  <div class="scrollbar" aria-hidden="true">
    <div
      v-show="vertical.size > 0"
      ref="vTrack"
      class="scrollbar__track scrollbar__track--v"
      :style="vTrackStyle"
      @pointerdown.self="pageBy('v', $event)"
    >
      <div
        class="scrollbar__thumb"
        :class="{ 'scrollbar__thumb--active': dragging === 'v' }"
        :style="vThumbStyle"
        @pointerdown="beginDrag('v', $event)"
        @pointermove="moveDrag"
        @pointerup="endDrag"
        @pointercancel="endDrag"
      />
    </div>

    <div
      v-show="horizontal.size > 0"
      ref="hTrack"
      class="scrollbar__track scrollbar__track--h"
      :style="hTrackStyle"
      @pointerdown.self="pageBy('h', $event)"
    >
      <div
        class="scrollbar__thumb"
        :class="{ 'scrollbar__thumb--active': dragging === 'h' }"
        :style="hThumbStyle"
        @pointerdown="beginDrag('h', $event)"
        @pointermove="moveDrag"
        @pointerup="endDrag"
        @pointercancel="endDrag"
      />
    </div>
  </div>
</template>

<style scoped>
.scrollbar {
  position: absolute;
  z-index: 3;
  inset: 0;
  pointer-events: none;
}

.scrollbar__track {
  position: absolute;
  pointer-events: auto;
  touch-action: none;
  user-select: none;
}

.scrollbar__thumb {
  position: absolute;
  border-radius: 999px;
  background: rgba(203, 213, 225, 0.7);
  transition: background 0.15s ease;
}

.scrollbar__track--v .scrollbar__thumb {
  top: 0;
  right: 0;
  left: 0;
}

.scrollbar__track--h .scrollbar__thumb {
  top: 0;
  bottom: 0;
  left: 0;
}

.scrollbar__thumb:hover,
.scrollbar__thumb--active {
  background: #e2e8f0;
}
</style>
