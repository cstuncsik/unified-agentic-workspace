import type { ITheme } from "@xterm/xterm";

export interface AppConfig {
  terminal: { fontSize: number; theme: ITheme };
  warning: string | null;
}

export interface AgentEdit {
  bin: string | null;
  args: string[];
}
export interface EditConfig {
  agents: Record<string, AgentEdit>;
  fontSize: number;
}
export interface ConfigForEdit {
  agents: Record<string, AgentEdit>;
  fontSize: number;
  warning: string | null;
}
