import { ref } from "vue";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { ask, message } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../i18n";

export function useAppUpdater() {
  const { t } = useI18n();
  const checking = ref(false);
  const updating = ref(false);
  const progressText = ref("");

  async function checkForUpdates() {
    if (checking.value || updating.value) return;

    checking.value = true;
    progressText.value = "";

    try {
      const update = await check();
      if (!update) {
        await message(t("update.upToDate"), {
          title: t("update.title"),
          kind: "info",
        });
        return;
      }

      const notes = update.body?.trim();
      const detail = notes
        ? `${t("update.available", { version: update.version })}\n\n${notes}`
        : t("update.available", { version: update.version });

      const confirmed = await ask(detail, {
        title: t("update.title"),
        kind: "info",
        okLabel: t("update.install"),
        cancelLabel: t("update.later"),
      });
      if (!confirmed) return;

      updating.value = true;
      let downloaded = 0;
      let contentLength = 0;

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
      const detail = error instanceof Error ? error.message : String(error);
      await message(t("update.failed", { error: detail }), {
        title: t("update.title"),
        kind: "error",
      });
    } finally {
      checking.value = false;
      updating.value = false;
    }
  }

  return {
    checking,
    updating,
    progressText,
    checkForUpdates,
  };
}
