import { computed, readonly, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

export type Platform = "macos" | "windows" | "linux" | "unknown";

const platform = ref<Platform>("unknown");

function normalize(value: string): Platform {
  return value === "macos" || value === "windows" || value === "linux" ? value : "unknown";
}

/**
 * 读取一次运行平台并缓存：侧边栏等组件在挂载时就需要知道
 * 当前平台是否内置了某个功能所需的工具链。
 */
export async function loadPlatform(): Promise<Platform> {
  try {
    platform.value = normalize(await invoke<string>("get_platform"));
  } catch {
    // 浏览器预览等非 Tauri 环境：保留 unknown，不隐藏任何入口
    platform.value = "unknown";
  }
  return platform.value;
}

export function usePlatform() {
  return {
    platform: readonly(platform),
    isWindows: computed(() => platform.value === "windows"),
    // 固件 APK 替换依赖 e2fsprogs 工具链，只有 macOS 构建打包了它
    supportsFirmwareApkUpdate: computed(
      () => platform.value === "macos" || platform.value === "unknown",
    ),
  };
}
