import { computed, ref, shallowRef } from "vue";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { message } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../i18n";
import { GITHUB_REPO_URL } from "../constants/app";
import { plainReleaseNotes, summarizeReleaseNotes } from "../utils/releaseNotes";

export interface UpdatePrompt {
  version: string;
  /** 摘要要点：弹窗默认只显示这几条 */
  summary: string[];
  /** 完整说明的纯文本，展开后放进可滚动区域 */
  full: string;
  /** 摘要被截断时才有展开的必要 */
  truncated: boolean;
}

/*
 * 更新状态挂在模块作用域：侧边栏的“检查更新”按钮和更新弹窗读的是同一份，
 * 弹窗因此不必层层接 props。
 */
const checking = ref(false);
const updating = ref(false);
const progressText = ref("");
const errorText = ref("");
const prompt = ref<UpdatePrompt | null>(null);
const pending = shallowRef<Update | null>(null);

/**
 * 发布说明是 Markdown，动辄几十行。这里备两份：摘要用于默认展示，
 * 纯文本全文留给“展开完整说明”，两者都不会撑破弹窗。
 */
function buildPrompt(update: Update): UpdatePrompt {
  const digest = summarizeReleaseNotes(update.body);
  return {
    version: update.version,
    summary: digest.text
      ? digest.text.split("\n").map((line) => line.replace(/^•\s*/, ""))
      : [],
    full: plainReleaseNotes(update.body),
    truncated: digest.truncated,
  };
}

export function useAppUpdater() {
  const { t } = useI18n();

  const releaseUrl = computed(() =>
    prompt.value ? `${GITHUB_REPO_URL}/releases/tag/v${prompt.value.version}` : "",
  );

  async function checkForUpdates({ silent = false }: { silent?: boolean } = {}) {
    if (checking.value || updating.value) return;

    checking.value = true;
    progressText.value = "";

    try {
      const update = await check();
      if (!update) {
        if (!silent) {
          await message(t("update.upToDate"), {
            title: t("update.title"),
            kind: "info",
          });
        }
        return;
      }

      pending.value = update;
      errorText.value = "";
      prompt.value = buildPrompt(update);
    } catch (error) {
      if (!silent) {
        const detail = error instanceof Error ? error.message : String(error);
        await message(t("update.failed", { error: detail }), {
          title: t("update.title"),
          kind: "error",
        });
      }
    } finally {
      checking.value = false;
    }
  }

  async function installUpdate() {
    const update = pending.value;
    if (!update || updating.value) return;

    updating.value = true;
    errorText.value = "";
    let downloaded = 0;
    let contentLength = 0;

    try {
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            contentLength = event.data.contentLength ?? 0;
            progressText.value = t("update.downloading", { percent: "0" });
            break;
          case "Progress":
            downloaded += event.data.chunkLength;
            if (contentLength > 0) {
              const percent = Math.min(100, Math.round((downloaded / contentLength) * 100));
              progressText.value = t("update.downloading", {
                percent: String(percent),
              });
            } else {
              progressText.value = t("update.downloadingBytes", {
                bytes: String(downloaded),
              });
            }
            break;
          case "Finished":
            progressText.value = t("update.installing");
            break;
        }
      });

      await relaunch();
    } catch (error) {
      // 失败信息留在弹窗里就地重试，比再弹一个原生对话框少一次打断
      errorText.value = error instanceof Error ? error.message : String(error);
    } finally {
      updating.value = false;
    }
  }

  function dismissPrompt() {
    // 下载/安装期间关掉弹窗会让进度无处可看，此时只能等它跑完
    if (updating.value) return;
    prompt.value = null;
    pending.value = null;
    errorText.value = "";
  }

  return {
    checking,
    updating,
    progressText,
    errorText,
    prompt,
    releaseUrl,
    checkForUpdates,
    installUpdate,
    dismissPrompt,
  };
}
