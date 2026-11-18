/**
 * The seven Nexora themes.
 *
 * This file is the source of truth for theme ids. The ids are plain
 * strings shared with the backend `theme` field in `NexoraSettings`
 * (see `src-tauri/src/settings.rs`); storing the selected id on disk is
 * owned by the settings-persist work (Main team), so selection is kept
 * in `localStorage` until that lands. Visuals live in `src/index.css`
 * as CSS variables scoped under `:root[data-theme="<id>"]`; this file
 * only carries metadata.
 */
export type ThemeId =
  | "obsidian"
  | "liquid-glass"
  | "aurora"
  | "sunset"
  | "midnight"
  | "forest"
  | "neon-glass";

export interface ThemeDefinition {
  id: ThemeId;
  name: string;
  description: string;
  /** Representative color shown in the theme picker swatch. */
  swatch: string;
}

export const DEFAULT_THEME_ID: ThemeId = "neon-glass";

export const THEMES: readonly ThemeDefinition[] = [
  {
    id: "obsidian",
    name: "Obsidian",
    description: "Dark slate pill with a sky-blue accent.",
    swatch: "#38bdf8",
  },
  {
    id: "liquid-glass",
    name: "Liquid Glass",
    description: "Translucent frosted glass with a soft white glow.",
    swatch: "#e0f2fe",
  },
  {
    id: "aurora",
    name: "Aurora",
    description: "Deep teal with an emerald accent.",
    swatch: "#2dd4bf",
  },
  {
    id: "sunset",
    name: "Sunset",
    description: "Warm ember dark with an orange accent.",
    swatch: "#fb923c",
  },
  {
    id: "midnight",
    name: "Midnight",
    description: "Deep violet night with a lavender accent.",
    swatch: "#a78bfa",
  },
  {
    id: "forest",
    name: "Forest",
    description: "Deep green with a fresh leaf accent.",
    swatch: "#4ade80",
  },
  {
    id: "neon-glass",
    name: "Neon Glass",
    description:
      "Deep violet glass with a neon purple-blue glow — the brand-sheet look.",
    swatch: "#a78bfa",
  },
];

export function isValidThemeId(value: string): value is ThemeId {
  return THEMES.some((theme) => theme.id === value);
}

export function themeOrDefault(
  value: string | null | undefined,
): ThemeId {
  return value !== null && value !== undefined && isValidThemeId(value)
    ? value
    : DEFAULT_THEME_ID;
}

export function themeById(id: ThemeId): ThemeDefinition {
  return THEMES.find((theme) => theme.id === id) ?? THEMES[0];
}
