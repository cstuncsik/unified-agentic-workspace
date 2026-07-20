import { invoke } from "@tauri-apps/api/core";
import type { AppConfig, ConfigForEdit, EditConfig } from "../types/appConfig";

export function getAppConfig(): Promise<AppConfig> {
  return invoke<AppConfig>("get_app_config");
}

export function getConfigForEdit(): Promise<ConfigForEdit> {
  return invoke<ConfigForEdit>("get_config_for_edit");
}
export function saveConfig(edits: EditConfig): Promise<AppConfig["terminal"]> {
  return invoke<AppConfig["terminal"]>("save_config", { edits });
}
