import type { ITheme } from "@xterm/xterm";

export interface AppConfig {
  terminal: { fontSize: number; theme: ITheme };
  warning: string | null;
}
