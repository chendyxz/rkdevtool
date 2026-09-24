import type { FirmwareInfo } from "../types/tool";
import { toolApi } from "./useToolCommand";

// 页面用 v-if 切换，组件会反复挂载/卸载。固件解析开销大（后端要读整个固件包），
// 这里缓存解析结果，让重新挂载的页面可以同步回显，避免空白闪烁。
const infoCache = new Map<string, FirmwareInfo>();

// 同一个路径解析失败只需提示一次，否则每次切回页面都会重新报错。
const reportedIssues = new Set<string>();

export function cachedFirmwareInfo(path: string): FirmwareInfo | null {
  const key = path.trim();
  return key ? (infoCache.get(key) ?? null) : null;
}

/** 首次报告返回 true；解析成功后重置，文件再次失效时可以重新提示。 */
export function shouldReportFirmwareIssue(path: string): boolean {
  const key = path.trim();
  if (!key || reportedIssues.has(key)) return false;
  reportedIssues.add(key);
  return true;
}

// 后端 parse_firmware_info 返回 "File not found: ..."；其余形态（os error 2 等）
// 兼容权限/IO 层转述，避免漏判。
const MISSING_FILE_RE = /file not found|no such file|os error 2/i;

/** 仅"文件不存在"才值得清空路径；格式不支持、权限不足等错误应保留路径便于排查。 */
export function isMissingFileError(err: unknown): boolean {
  const message = err instanceof Error ? err.message : String(err);
  return MISSING_FILE_RE.test(message);
}

export async function loadFirmwareInfo(path: string): Promise<FirmwareInfo> {
  const key = path.trim();
  try {
    // 后端按文件指纹（mtime + size）缓存，命中时开销可忽略。
    const info = await toolApi.parseFirmware(key);
    if (key) {
      infoCache.set(key, info);
      reportedIssues.delete(key);
    }
    return info;
  } catch (err) {
    // 文件被删除/移动后解析会失败，必须丢弃缓存，
    // 否则下次挂载会先回显一份已经失效的固件信息。
    infoCache.delete(key);
    throw err;
  }
}
