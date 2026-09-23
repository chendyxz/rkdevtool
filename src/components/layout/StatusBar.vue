<script setup lang="ts">
import { computed, ref } from "vue";
import { useAppState } from "../../composables/useAppState";
import { useI18n } from "../../i18n";
import { toolApi } from "../../composables/useToolCommand";

const emit = defineEmits<{
  deviceChange: [locationId: string];
}>();

const { deviceState, devices, selectedDeviceId, adbSerial, busy, appendLog } = useAppState();
const { t } = useI18n();
const switching = ref(false);

const statusLabel = computed(() => {
  switch (deviceState.value) {
    case "adb":
      return t("status.adb");
    case "connected":
      return t("status.maskrom");
    case "loader":
      return t("status.loader");
    default:
      return t("status.disconnected");
  }
});

const dotColor = computed(() => {
  switch (deviceState.value) {
    case "adb":
      return "var(--color-warning)";
    case "connected":
      return "var(--color-success)";
    case "loader":
      return "var(--color-primary)";
    default:
      return "var(--color-danger)";
  }
});

function onSelect(event: Event) {
  const value = (event.target as HTMLSelectElement).value;
  if (value) emit("deviceChange", value);
}

async function switchToLoader() {
  if (!adbSerial.value || switching.value) return;
  switching.value = true;
  appendLog(t("status.switchingToLoader"), "info");
  try {
    await toolApi.rebootToLoader(adbSerial.value);
    appendLog(t("status.switchedToLoader"), "success");
  } catch (error) {
    appendLog(String(error), "error");
  } finally {
    switching.value = false;
  }
}
</script>

<template>
  <footer class="status-bar">
    <div class="status-bar__left">
      <span class="status-bar__dot" :style="{ background: dotColor }" />
      <span class="status-bar__text">{{ statusLabel }}</span>
      <span v-if="busy" class="status-bar__busy">{{ t("status.busy") }}</span>
      <button
        v-if="deviceState === 'adb'"
        class="status-bar__switch"
        :disabled="switching"
        @click="switchToLoader"
      >
        {{ switching ? t("status.switching") : t("status.switch") }}
      </button>
    </div>
    <select
      class="status-bar__select"
      :value="selectedDeviceId ?? ''"
      :disabled="devices.length === 0"
      @change="onSelect"
    >
      <option v-if="devices.length === 0" value="">
        {{ adbSerial ? `ADB: ${adbSerial}` : t("status.noDevice") }}
      </option>
      <option v-for="device in devices" :key="device.location_id" :value="device.location_id">
        {{ device.label }}
      </option>
    </select>
  </footer>
</template>

<style scoped>
.status-bar {
  height: var(--status-bar-height);
  background: var(--color-surface);
  border-top: 1px solid var(--color-border);
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 16px;
  flex-shrink: 0;
}

.status-bar__left {
  display: flex;
  align-items: center;
  gap: 10px;
}

.status-bar__dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  flex-shrink: 0;
}

.status-bar__text {
  font-size: 14px;
  font-weight: 600;
  color: var(--color-text-primary);
}

.status-bar__busy {
  font-size: 12px;
  color: var(--color-primary);
}

.status-bar__switch {
  height: 28px;
  padding: 0 14px;
  border: 0;
  border-radius: var(--border-radius-md);
  background: var(--color-primary);
  color: white;
  cursor: pointer;
}

.status-bar__switch:disabled {
  cursor: default;
  opacity: 0.6;
}

.status-bar__select {
  min-width: 280px;
  height: 36px;
  padding: 0 10px;
  border-radius: var(--border-radius-md);
  border: 1px solid var(--color-border);
  background: var(--color-surface);
  color: var(--color-text-primary);
  font-size: 12px;
}

.status-bar__select:disabled {
  opacity: 0.6;
}
</style>
