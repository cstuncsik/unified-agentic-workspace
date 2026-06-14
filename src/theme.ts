export type ThemeMode = "system" | "light" | "dark";

const STORAGE_KEY = "uaw.theme";

// Exactly one of these classes is present on <html> at any time.
const THEME_CLASS: Record<ThemeMode, string> = {
  system: "theme-renascent",
  light: "theme-renascent-light",
  dark: "theme-renascent-dark",
};

export function getStoredTheme(): ThemeMode {
  const value = localStorage.getItem(STORAGE_KEY);
  // Default to the renascent dark-navy identity, not the OS preference.
  return value === "light" || value === "dark" || value === "system" ? value : "dark";
}

/** Apply a theme mode to the document root and persist the choice. */
export function applyTheme(mode: ThemeMode): void {
  const root = document.documentElement;
  root.classList.remove(...Object.values(THEME_CLASS));
  root.classList.add(THEME_CLASS[mode]);
  localStorage.setItem(STORAGE_KEY, mode);
}
