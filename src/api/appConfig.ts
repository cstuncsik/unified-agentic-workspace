import { invoke } from "@tauri-apps/api/core";
import type { AppConfig } from "../types/appConfig";

export function getAppConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("get_app_config");
}
