import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch, type ComputedRef, type Ref } from "vue";

export interface VirtualListOptions {
  /** 尚未测量高度的行的估算高度（px） */
  estimateHeight?: number;
  /** 视口外额外渲染的行数，用于抵消快速滚动时的空白 */
  overscan?: number;
  /** 滚动容器内的顶部 sticky 元素，其高度不计入内容坐标 */
  header?: Ref<HTMLElement | null>;
}

export interface VirtualList<T> {
  /** 当前应该渲染的行（视口 + overscan） */
  visibleItems: ComputedRef<T[]>;
  /** 渲染窗口相对内容顶部的偏移，用于 transform */
  offsetTop: ComputedRef<number>;
  /** 全部内容的总高度，用于撑起滚动条 */
  totalHeight: Ref<number>;
  /** 用户当前是否停留在底部（含容差） */
  atBottom: Ref<boolean>;
  /** 绑定到滚动容器的 scroll 处理函数 */
  onScroll: () => void;
  /** 滚动到底部并保持跟随 */
  scrollToBottom: () => void;
  /** 容器尺寸变化后重新计算（挂载、全屏切换等） */
  refresh: () => void;
}

const BOTTOM_TOLERANCE = 48;
const MAX_HEIGHT_CACHE = 20_000;

/**
 * 可变行高虚拟滚动。
 *
 * 只渲染视口（含 overscan）内的行：用与总高度等高的占位元素撑起滚动条，
 * 再用 transform 把渲染窗口平移到正确位置，因此 DOM 节点数量与总行数无关。
 * 行高按行 id 缓存，渲染后用一次 offsetHeight 批量测量修正，
 * 所以消息自动换行导致的高度差异也能正确参与滚动定位。
 */
export function useVirtualList<T extends { id: number }>(
  viewport: Ref<HTMLElement | null>,
  getItems: () => T[],
  options: VirtualListOptions = {},
): VirtualList<T> {
  const estimateHeight = options.estimateHeight ?? 22;
  const overscan = options.overscan ?? 8;
  const header = options.header;

  const heights = new Map<number, number>();
  const offsets = shallowRef<number[]>([0]);
  const totalHeight = ref(0);
  const startIndex = ref(0);
  const endIndex = ref(0);
  const atBottom = ref(true);

  let scrollTop = 0;
  let viewportHeight = 0;
  let headerHeight = 0;
  let expectedScrollTop = -1;
  let measureFrame: number | null = null;
  let resizeObserver: ResizeObserver | null = null;

  const visibleItems = computed(() => getItems().slice(startIndex.value, endIndex.value));
  const offsetTop = computed(() => offsets.value[startIndex.value] ?? 0);

  function rebuildOffsets() {
    const list = getItems();
    if (heights.size > MAX_HEIGHT_CACHE) heights.clear();
    const next = new Array<number>(list.length + 1);
    next[0] = 0;
    let y = 0;
    for (let i = 0; i < list.length; i += 1) {
      y += heights.get(list[i].id) ?? estimateHeight;
      next[i + 1] = y;
    }
    offsets.value = next;
    totalHeight.value = y;
  }

  /** 返回 offsets 中最后一个 <= y 的下标，即 y 所在的行 */
  function locate(y: number) {
    const arr = offsets.value;
    let low = 0;
    let high = arr.length - 1;
    while (low < high) {
      const mid = (low + high) >> 1;
      if (arr[mid] <= y) low = mid + 1;
      else high = mid;
    }
    return Math.max(0, low - 1);
  }

  function refreshRange() {
    const count = getItems().length;
    if (count === 0) {
      startIndex.value = 0;
      endIndex.value = 0;
      return;
    }
    const top = Math.max(0, scrollTop - headerHeight);
    const bottom = Math.max(top, scrollTop + viewportHeight - headerHeight);
    const start = Math.max(0, locate(top) - overscan);
    const end = Math.min(count, locate(bottom) + 1 + overscan);
    if (start !== startIndex.value) startIndex.value = start;
    if (end !== endIndex.value) endIndex.value = end;
  }

  function syncMetrics() {
    const el = viewport.value;
    if (!el) return;
    viewportHeight = el.clientHeight;
    headerHeight = header?.value?.offsetHeight ?? 0;
    scrollTop = el.scrollTop;
  }

  function measure() {
    const el = viewport.value;
    if (!el) return;
    let changed = false;
    el.querySelectorAll<HTMLElement>("[data-vrow]").forEach((node) => {
      const id = Number(node.dataset.vrow);
      if (!Number.isFinite(id)) return;
      const height = node.offsetHeight;
      if (height > 0 && heights.get(id) !== height) {
        heights.set(id, height);
        changed = true;
      }
    });
    if (!changed) return;
    rebuildOffsets();
    refreshRange();
    if (atBottom.value) applyStick();
  }

  function scheduleMeasure() {
    if (measureFrame !== null) return;
    measureFrame = requestAnimationFrame(() => {
      measureFrame = null;
      measure();
    });
  }

  /** 贴住底部：设置滚动位置后记录预期值，用于区分程序滚动与用户滚动 */
  function applyStick() {
    const el = viewport.value;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    expectedScrollTop = el.scrollTop;
    scrollTop = el.scrollTop;
    refreshRange();
    scheduleMeasure();
  }

  function onScroll() {
    const el = viewport.value;
    if (!el) return;
    scrollTop = el.scrollTop;
    // 程序滚动（贴底）产生的第一次 scroll 事件只用于刷新范围，
    // 不参与"用户是否离开底部"的判断，避免测量修正时误判
    if (expectedScrollTop >= 0) {
      const expected = expectedScrollTop;
      expectedScrollTop = -1;
      if (Math.abs(scrollTop - expected) <= 2) {
        refreshRange();
        return;
      }
    }
    const distance = el.scrollHeight - scrollTop - el.clientHeight;
    const bottom = distance < BOTTOM_TOLERANCE;
    if (bottom !== atBottom.value) atBottom.value = bottom;
    refreshRange();
  }

  function scrollToBottom() {
    atBottom.value = true;
    applyStick();
  }

  function refresh() {
    syncMetrics();
    refreshRange();
    scheduleMeasure();
  }

  watch(getItems, () => {
    rebuildOffsets();
    refreshRange();
    scheduleMeasure();
  }, { immediate: true });

  onMounted(() => {
    refresh();
    const el = viewport.value;
    if (el && typeof ResizeObserver !== "undefined") {
      resizeObserver = new ResizeObserver(refresh);
      resizeObserver.observe(el);
    }
  });

  onBeforeUnmount(() => {
    if (measureFrame !== null) cancelAnimationFrame(measureFrame);
    resizeObserver?.disconnect();
  });

  return { visibleItems, offsetTop, totalHeight, atBottom, onScroll, scrollToBottom, refresh };
}
