import { createApp } from "vue";
import App from "./App.vue";
import { loadPlatform } from "./composables/usePlatform";
import "./styles/global.css";

// 先探测平台再挂载，避免侧边栏闪现当前平台不支持的入口
void loadPlatform().finally(() => {
  createApp(App).mount("#app");
});
