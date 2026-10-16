/**
 * Colour palettes. Each has a light and a dark version; the accent colour (buttons, links, focus) is
 * chosen separately and falls back to the palette's own. Components read semantic tokens (`--background`,
 * `--primary`, ...), so this file is the one place the colours live: `themeCss()` turns it into CSS that
 * `installThemes()` adds to the page, and `applyTheme()` switches between them.
 */

export type PaletteId = "paper_pine" | "slate" | "mono" | "lavender";
export type Mode = "light" | "dark" | "system";

interface Tokens {
  background: string;
  foreground: string;
  card: string;
  muted: string;
  mutedForeground: string;
  border: string;
  /** Solid dark (or, in dark mode, light) controls and active tabs. */
  secondary: string;
  secondaryForeground: string;
  destructive: string;
  /** Orange for due soon and urgent only. */
  signal: string;
  /** The palette's own accent, and the text on it. */
  accent: string;
  onAccent: string;
}

export interface Palette {
  id: PaletteId;
  label: string;
  light: Tokens;
  dark: Tokens;
}

export const PALETTES: Palette[] = [
  {
    id: "paper_pine",
    label: "Paper and pine",
    light: {
      background: "#e8ebe5",
      foreground: "#17231e",
      card: "#fafbf8",
      muted: "#dde2d9",
      mutedForeground: "#5d6b63",
      border: "#c8cec4",
      secondary: "#17231e",
      secondaryForeground: "#fafbf8",
      destructive: "#b42318",
      signal: "#e4570f",
      accent: "#1f4d3a",
      onAccent: "#ffffff",
    },
    dark: {
      background: "#101713",
      foreground: "#e6ece7",
      card: "#17201b",
      muted: "#1d2822",
      mutedForeground: "#93a39a",
      border: "#2c3a33",
      secondary: "#e6ece7",
      secondaryForeground: "#101713",
      destructive: "#f0614f",
      signal: "#ff7a3a",
      accent: "#4f9b79",
      onAccent: "#07120d",
    },
  },
  {
    id: "slate",
    label: "Slate",
    light: {
      background: "#e9edf2",
      foreground: "#172230",
      card: "#f8fafc",
      muted: "#dde3ea",
      mutedForeground: "#5a6877",
      border: "#c9d1db",
      secondary: "#172230",
      secondaryForeground: "#f8fafc",
      destructive: "#b42318",
      signal: "#e4570f",
      accent: "#2f5d8a",
      onAccent: "#ffffff",
    },
    dark: {
      background: "#0f151d",
      foreground: "#e5ebf2",
      card: "#161e28",
      muted: "#1c2631",
      mutedForeground: "#8fa0b3",
      border: "#2a3644",
      secondary: "#e5ebf2",
      secondaryForeground: "#0f151d",
      destructive: "#f0614f",
      signal: "#ff7a3a",
      accent: "#5b95d0",
      onAccent: "#06101b",
    },
  },
  {
    id: "mono",
    label: "Mono",
    light: {
      background: "#f0f0f0",
      foreground: "#111111",
      card: "#ffffff",
      muted: "#e2e2e2",
      mutedForeground: "#5c5c5c",
      border: "#cfcfcf",
      secondary: "#111111",
      secondaryForeground: "#ffffff",
      destructive: "#b42318",
      signal: "#e4570f",
      accent: "#111111",
      onAccent: "#ffffff",
    },
    dark: {
      background: "#0d0d0d",
      foreground: "#f0f0f0",
      card: "#161616",
      muted: "#1f1f1f",
      mutedForeground: "#a0a0a0",
      border: "#2e2e2e",
      secondary: "#f0f0f0",
      secondaryForeground: "#0d0d0d",
      destructive: "#f0614f",
      signal: "#ff7a3a",
      accent: "#f0f0f0",
      onAccent: "#0d0d0d",
    },
  },
  {
    id: "lavender",
    label: "Lavender",
    light: {
      background: "#ece9f3",
      foreground: "#20182f",
      card: "#faf8fd",
      muted: "#e1dcec",
      mutedForeground: "#665c7a",
      border: "#cfc8de",
      secondary: "#20182f",
      secondaryForeground: "#faf8fd",
      destructive: "#b42318",
      signal: "#e4570f",
      accent: "#5b3fa0",
      onAccent: "#ffffff",
    },
    dark: {
      background: "#14101c",
      foreground: "#ebe6f5",
      card: "#1b1626",
      muted: "#231d30",
      mutedForeground: "#a398b8",
      border: "#31283f",
      secondary: "#ebe6f5",
      secondaryForeground: "#14101c",
      destructive: "#f0614f",
      signal: "#ff7a3a",
      accent: "#9a7fe0",
      onAccent: "#120a24",
    },
  },
];

export const DEFAULT_PALETTE: PaletteId = "paper_pine";

/** A few starting points for the accent colour (any colour can be chosen). */
export const ACCENT_SWATCHES = ["#2f5d8a", "#1f4d3a", "#b4540b", "#0f6b8a", "#a1304a", "#5b3fa0"];

export function paletteOf(id: string | null | undefined): Palette {
  return PALETTES.find((p) => p.id === id) ?? PALETTES[0];
}

function rules(selector: string, t: Tokens): string {
  return `${selector} {
  --background: ${t.background};
  --foreground: ${t.foreground};
  --card: ${t.card};
  --card-foreground: ${t.foreground};
  --popover: ${t.card};
  --popover-foreground: ${t.foreground};
  --secondary: ${t.secondary};
  --secondary-foreground: ${t.secondaryForeground};
  --muted: ${t.muted};
  --muted-foreground: ${t.mutedForeground};
  --border: ${t.border};
  --input: ${t.border};
  --destructive: ${t.destructive};
  --signal: ${t.signal};
  --accent-default: ${t.accent};
  --on-accent-default: ${t.onAccent};
}`;
}

/** The CSS for every palette, in both modes (dark ones need the `dark` class on the root element). */
export function themeCss(): string {
  return PALETTES.map(
    (p) =>
      `${rules(`:root[data-palette="${p.id}"]`, p.light)}\n${rules(`:root[data-palette="${p.id}"].dark`, p.dark)}`,
  ).join("\n");
}

let installed = false;

/** Add the palettes' CSS to the page (once). */
export function installThemes() {
  if (installed || typeof document === "undefined") return;
  installed = true;
  const style = document.createElement("style");
  style.id = "wm-themes";
  style.textContent = themeCss();
  document.head.appendChild(style);
}

function channel(value: number): number {
  const v = value / 255;
  return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
}

function luminance(hex: string): number {
  const n = parseInt(hex.slice(1), 16);
  return (
    0.2126 * channel((n >> 16) & 255) + 0.7152 * channel((n >> 8) & 255) + 0.0722 * channel(n & 255)
  );
}

/** White or near-black, whichever reads better on a button of this colour. */
export function onColor(accent: string): string {
  const l = luminance(accent);
  const withWhite = 1.05 / (l + 0.05);
  // Near-black text (#0b0b0b) has luminance 0.003.
  const withBlack = (l + 0.05) / 0.053;
  return withWhite >= withBlack ? "#ffffff" : "#0b0b0b";
}

export interface Theme {
  palette: PaletteId;
  /** `#rrggbb`, or null for the palette's own. */
  accent: string | null;
  mode: Mode;
}

const CACHE_KEY = "wm-theme";

export function readCachedTheme(): Theme {
  try {
    const raw = JSON.parse(localStorage.getItem(CACHE_KEY) ?? "null") as Partial<Theme> | null;
    return {
      palette: paletteOf(raw?.palette).id,
      accent: typeof raw?.accent === "string" ? raw.accent : null,
      mode: raw?.mode === "light" || raw?.mode === "dark" ? raw.mode : "system",
    };
  } catch {
    return { palette: DEFAULT_PALETTE, accent: null, mode: "system" };
  }
}

export const prefersDark = () =>
  typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches;

/** Show a theme: palette, accent and light or dark, and remember it for the next page load. */
export function applyTheme(theme: Theme) {
  installThemes();
  const root = document.documentElement;
  const dark = theme.mode === "dark" || (theme.mode === "system" && prefersDark());
  root.dataset.palette = theme.palette;
  root.classList.toggle("dark", dark);
  root.style.colorScheme = dark ? "dark" : "light";
  if (theme.accent) {
    root.style.setProperty("--accent-color", theme.accent);
    root.style.setProperty("--on-accent-color", onColor(theme.accent));
  } else {
    root.style.removeProperty("--accent-color");
    root.style.removeProperty("--on-accent-color");
  }
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(theme));
  } catch {
    // Private windows can refuse storage; the theme still applies.
  }
}

/** What a person sees in a workspace: their own choice if they made one, else the workspace's. */
export function effectiveChoice(view: {
  workspace: { palette?: string | null; accent?: string | null };
  mine: { palette?: string | null; accent?: string | null };
}): { palette: PaletteId; accent: string | null } {
  const own = view.mine.palette || view.mine.accent ? view.mine : view.workspace;
  return { palette: paletteOf(own.palette).id, accent: own.accent ?? null };
}
