/** Light / dark theme. With no saved choice, GameX follows Windows. */

export type Theme = "light" | "dark";

const KEY = "gamex-theme";

function saved(): Theme | null {
  try {
    const value = localStorage.getItem(KEY);
    return value === "light" || value === "dark" ? value : null;
  } catch {
    return null;
  }
}

export function currentTheme(): Theme {
  return saved() ?? (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
}

export function applyTheme(theme: Theme | null) {
  if (theme) document.documentElement.dataset.theme = theme;
  else delete document.documentElement.dataset.theme;
}

export function setTheme(theme: Theme) {
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    // Storage can be unavailable; the choice then lasts for this session.
  }
  applyTheme(theme);
}

/** Call once at startup so a saved choice applies before the first paint. */
export function initTheme() {
  applyTheme(saved());
}
