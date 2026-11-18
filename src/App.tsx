import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  THEMES,
  themeById,
  themeOrDefault,
  type ThemeId,
} from "./themes";

interface IslandState {
  expanded: boolean;
  mode: string;
  title: string;
}

interface NexoraSettings {
  auto_hide_taskbar: boolean;
  always_on_top: boolean;
  theme: string;
}

const STORAGE_KEY = "nexora-theme";

function readSavedTheme(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

function saveTheme(id: ThemeId) {
  try {
    localStorage.setItem(STORAGE_KEY, id);
  } catch {
    // Storage unavailable (private mode, etc.) — the theme still applies
    // for this session.
  }
}

export function App() {
  const [state, setState] = useState<IslandState>({
    expanded: false,
    mode: "compact",
    title: "Nexora Island",
  });
  const [theme, setTheme] = useState<ThemeId>("obsidian");

  const applyTheme = (id: ThemeId) => {
    setTheme(id);
    document.documentElement.dataset.theme = id;
    saveTheme(id);
  };

  useEffect(() => {
    // Saved choice wins; the backend default is the fallback.
    const saved = readSavedTheme();
    invoke<NexoraSettings>("get_settings")
      .then((settings) => applyTheme(themeOrDefault(saved ?? settings.theme)))
      .catch(() => applyTheme(themeOrDefault(saved)));
    invoke<IslandState>("get_island_state")
      .then(setState)
      .catch(() => {});
    // Runs once on mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const toggle = () => {
    const next = !state.expanded;
    invoke<IslandState>("set_island_expanded", { expanded: next })
      .then(setState)
      .catch(() =>
        setState((prev) => ({
          ...prev,
          expanded: next,
          mode: next ? "expanded" : "compact",
        }))
      );
  };

  const currentTheme = themeById(theme);

  return (
    <main className="island-shell" data-tauri-drag-region>
      <section
        className={`island-pill${state.expanded ? " expanded" : ""}`}
        data-tauri-drag-region
      >
        <div className="island-row" data-tauri-drag-region>
          <div className="island-brand">
            <span className="island-dot" />
            <div>
              <div className="island-title">{state.title}</div>
              <div className="island-sub">
                Mode: {state.mode} · {currentTheme.name}
              </div>
            </div>
          </div>
          <button className="island-btn" onClick={toggle} type="button">
            {state.expanded ? "Collapse" : "Expand"}
          </button>
        </div>
        {state.expanded && (
          <div
            className="island-themes"
            role="group"
            aria-label="Island theme"
          >
            <span className="theme-label">Theme:</span>
            {THEMES.map((candidate) => (
              <button
                key={candidate.id}
                type="button"
                className={`theme-swatch${
                  candidate.id === theme ? " selected" : ""
                }`}
                style={{ background: candidate.swatch }}
                title={`${candidate.name} — ${candidate.description}`}
                aria-label={candidate.name}
                aria-pressed={candidate.id === theme}
                onClick={() => applyTheme(candidate.id)}
              />
            ))}
          </div>
        )}
      </section>
    </main>
  );
}

export default App;
