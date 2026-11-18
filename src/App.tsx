import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface IslandState {
  expanded: boolean;
  mode: string;
  title: string;
}

export function App() {
  const [state, setState] = useState<IslandState>({
    expanded: false,
    mode: "compact",
    title: "Nexora Island",
  });

  useEffect(() => {
    invoke<IslandState>("get_island_state")
      .then(setState)
      .catch(() => {});
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

  return (
    <main className="island-shell" data-tauri-drag-region>
      <section className="island-pill" data-tauri-drag-region>
        <div className="island-brand">
          <span className="island-dot" />
          <div>
            <div className="island-title">{state.title}</div>
            <div className="island-sub">Mode: {state.mode}</div>
          </div>
        </div>
        <button className="island-btn" onClick={toggle} type="button">
          {state.expanded ? "Collapse" : "Expand"}
        </button>
      </section>
    </main>
  );
}

export default App;
