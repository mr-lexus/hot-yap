import { getCurrentWindow } from "@tauri-apps/api/window";

export type Theme = "dark" | "light";
export type Accent = "amber" | "blue" | "violet" | "green" | "rose" | "cyan";
export type IconPreference = "system" | "dark-panel" | "light-panel";

export const ACCENTS: Accent[] = ["amber", "blue", "violet", "green", "rose", "cyan"];
export const ICON_PREFERENCES: IconPreference[] = ["system", "dark-panel", "light-panel"];

let iconApplyGeneration = 0;

export function savedTheme(): Theme {
  return window.localStorage.getItem("hotyap-theme") === "light" ? "light" : "dark";
}

export function savedAccent(): Accent {
  const value = window.localStorage.getItem("hotyap-accent") as Accent | null;
  return value && ACCENTS.includes(value) ? value : "amber";
}

export function savedIconPreference(): IconPreference {
  const value = window.localStorage.getItem("hotyap-icon-preference") as IconPreference | null;
  return value && ICON_PREFERENCES.includes(value) ? value : "system";
}

export function applyAppearance(theme: Theme, accent: Accent) {
  document.documentElement.dataset.theme = theme;
  document.documentElement.dataset.accent = accent;
}

export function browserSystemTheme(): Theme {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function resolvePanelTheme(preference: IconPreference, systemTheme: Theme): Theme {
  return preference === "system" ? systemTheme : preference === "dark-panel" ? "dark" : "light";
}

export function resolveLogoVariant(preference: IconPreference, systemTheme: Theme): Theme {
  return resolvePanelTheme(preference, systemTheme) === "dark" ? "light" : "dark";
}

function iconSource(preference: IconPreference, systemTheme: Theme): string {
  return `/${resolveLogoVariant(preference, systemTheme)}.png`;
}

function applyFavicon(source: string) {
  document.querySelectorAll("link[data-hotyap-favicon]").forEach((link) => link.remove());
  const favicon = document.createElement("link");
  favicon.dataset.hotyapFavicon = "true";
  favicon.rel = "icon";
  favicon.type = "image/png";
  favicon.href = source;
  document.head.append(favicon);
}

export async function applyIconPreference(preference: IconPreference) {
  const generation = ++iconApplyGeneration;
  const currentWindow = getCurrentWindow();
  const systemTheme = await currentWindow.theme().catch(browserSystemTheme) ?? browserSystemTheme();
  const source = iconSource(preference, systemTheme);
  applyFavicon(source);

  if (currentWindow.label !== "main") return;
  const response = await fetch(source);
  if (!response.ok || generation !== iconApplyGeneration) return;
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (generation !== iconApplyGeneration) return;
  await currentWindow.setIcon(bytes);
}

export function initializeAppearance() {
  applyAppearance(savedTheme(), savedAccent());
  void applyIconPreference(savedIconPreference()).catch(() => {});
}
