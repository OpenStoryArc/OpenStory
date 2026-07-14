/** Light/dark theme toggle.
 *
 *  The palette lives in CSS variables on `:root` (dark default) with a
 *  `[data-theme="light"]` override block in index.css. This button flips the
 *  `data-theme` attribute and persists the choice; index.html applies the
 *  saved theme before first paint so there's no flash.
 */

import { useEffect, useState } from "react";

const KEY = "os.theme";
type Theme = "dark" | "light";

function readTheme(): Theme {
  try {
    return localStorage.getItem(KEY) === "light" ? "light" : "dark";
  } catch {
    return "dark";
  }
}

export function ThemeToggle() {
  const [theme, setTheme] = useState<Theme>(readTheme);

  useEffect(() => {
    if (theme === "light") {
      document.documentElement.dataset.theme = "light";
    } else {
      delete document.documentElement.dataset.theme;
    }
    try {
      localStorage.setItem(KEY, theme);
    } catch {
      /* ignore quota/private-mode */
    }
  }, [theme]);

  const next: Theme = theme === "dark" ? "light" : "dark";
  return (
    <button
      type="button"
      onClick={() => setTheme(next)}
      className="rounded border border-[color:var(--border)] px-2 py-1 text-[length:var(--fs-body)] text-[color:var(--text-muted)] transition-colors hover:border-[color:var(--accent)] hover:text-[color:var(--text)]"
      title={`Switch to ${next} mode`}
      aria-label={`Switch to ${next} mode`}
    >
      {theme === "dark" ? "☀ light" : "☾ dark"}
    </button>
  );
}
