<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { save } from "@tauri-apps/plugin-dialog";
import AppButton from "./AppButton.vue";
import { useAppState } from "../../composables/useAppState";
import { useI18n } from "../../i18n";
import { toolApi } from "../../composables/useToolCommand";

const props = defineProps<{ serial: string | null }>();
const emit = defineEmits<{ close: [] }>();

const { appendLog } = useAppState();
const { t } = useI18n();

const imageUrl = ref("");
const resolution = ref("");
const capturedAt = ref("");
const capturing = ref(false);
const saving = ref(false);
const error = ref("");
const savedPath = ref("");

const meta = computed(() => [capturedAt.value, resolution.value].filter(Boolean).join(" · "));

function releaseImage() {
  if (imageUrl.value) URL.revokeObjectURL(imageUrl.value);
  imageUrl.value = "";
}

function timestamp() {
  const now = new Date();
  const pad = (value: number) => String(value).padStart(2, "0");
  const date = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}`;
  const time = `${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
  return `${date}_${time}`;
}

/** 抓一帧并换成 blob URL 预览；字节同时留在 Rust 侧，保存时只传路径。 */
async function capture() {
  if (capturing.value) return;
  if (!props.serial) {
    error.value = t("logcat.noDevice");
    return;
  }
  capturing.value = true;
  error.value = "";
  savedPath.value = "";
  try {
    const png = await toolApi.captureScreenshot(props.serial);
    const url = URL.createObjectURL(new Blob([png], { type: "image/png" }));
    releaseImage();
    imageUrl.value = url;
    resolution.value = "";
    capturedAt.value = new Date().toLocaleTimeString();
    const image = new Image();
    image.onload = () => {
      // 预览可能已被新一轮截图替换，迟到的回调不要再改分辨率
      if (imageUrl.value === url) {
        resolution.value = `${image.naturalWidth} × ${image.naturalHeight}`;
      }
    };
    image.src = url;
  } catch (err) {
    error.value = String(err);
    appendLog(String(err), "error");
  } finally {
    capturing.value = false;
  }
}

async function saveImage() {
  if (!imageUrl.value || saving.value) return;
  const path = await save({
    title: t("logcat.screenshotSaveTitle"),
    defaultPath: `Screenshot_${timestamp()}.png`,
    filters: [{ name: "PNG", extensions: ["png"] }],
  });
  if (!path) return;
  saving.value = true;
  try {
    await toolApi.saveScreenshot(path);
    // 设备日志页隐藏了右侧日志面板，所以在弹窗里也要给出保存结果
    savedPath.value = path;
    appendLog(t("logcat.screenshotSaved", { path }), "success");
  } catch (err) {
    appendLog(String(err), "error");
  } finally {
    saving.value = false;
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key !== "Escape") return;
  // 子组件的监听先于父页面注册，标记后父页面就不会顺手把全屏也退了
  event.preventDefault();
  emit("close");
}

onMounted(() => {
  window.addEventListener("keydown", onKeydown);
  void capture();
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown);
  releaseImage();
});
</script>

<template>
  <div class="shot-overlay" @click.self="emit('close')">
    <div class="shot-dialog">
      <header class="shot-dialog__header">
        <span>{{ t("logcat.screenshotTitle") }}</span>
        <span v-if="meta" class="shot-dialog__meta">{{ meta }}</span>
      </header>

      <div class="shot-dialog__body">
        <img
          v-if="imageUrl"
          class="shot-dialog__image"
          :src="imageUrl"
          :alt="t('logcat.screenshotTitle')"
        />
        <p v-else-if="capturing" class="shot-dialog__hint">{{ t("logcat.screenshotCapturing") }}</p>
        <p v-else class="shot-dialog__hint shot-dialog__hint--error">
          {{ error || t("logcat.screenshotFailed") }}
        </p>
      </div>

      <p v-if="savedPath" class="shot-dialog__saved">
        {{ t("logcat.screenshotSaved", { path: savedPath }) }}
      </p>

      <footer class="shot-dialog__footer">
        <AppButton size="sm" :disabled="capturing" @click="capture">
          {{ t("logcat.screenshotRetake") }}
        </AppButton>
        <AppButton
          size="sm"
          variant="primary"
          :disabled="!imageUrl || saving"
          @click="saveImage"
        >
          {{ t("logcat.screenshotSave") }}
        </AppButton>
        <AppButton size="sm" @click="emit('close')">{{ t("logcat.screenshotClose") }}</AppButton>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.shot-overlay {
  position: fixed;
  z-index: 1200;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(15, 23, 42, 0.72);
}

.shot-dialog {
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 100%;
  max-height: 100%;
  padding: 16px;
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-lg);
  background: var(--color-surface);
  box-shadow: 0 18px 48px rgba(15, 23, 42, 0.45);
}

.shot-dialog__header {
  display: flex;
  align-items: baseline;
  gap: 10px;
  color: var(--color-text-primary);
  font-size: 13px;
  font-weight: 600;
}

.shot-dialog__meta {
  color: var(--color-text-secondary);
  font-size: 11px;
  font-weight: 400;
}

.shot-dialog__body {
  display: flex;
  min-height: 220px;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  background: #111827;
  overflow: hidden;
}

.shot-dialog__image {
  display: block;
  max-width: min(70vw, 900px);
  max-height: min(64vh, 700px);
  object-fit: contain;
}

.shot-dialog__hint {
  margin: 0;
  padding: 32px;
  color: #94a3b8;
  font-size: 12px;
}

.shot-dialog__hint--error {
  color: #fb7185;
  max-width: 460px;
  text-align: center;
  overflow-wrap: anywhere;
}

.shot-dialog__saved {
  margin: 0;
  color: var(--color-success);
  font-size: 11px;
  overflow-wrap: anywhere;
}

.shot-dialog__footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
