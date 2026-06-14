import { createApp } from "vue";
import { createPinia } from "pinia";
import "@relements/core/index.css";
import "@relements/core/themes/renascent.css";
import App from "./App.vue";
import { applyTheme, getStoredTheme } from "./theme";

// Apply the persisted theme before mount to avoid a flash of the wrong theme.
applyTheme(getStoredTheme());

createApp(App).use(createPinia()).mount("#app");
