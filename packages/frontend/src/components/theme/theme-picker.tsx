import { useEffect, useState } from "react";

import {
  ACCENT_SWATCHES,
  PALETTES,
  onColor,
  paletteOf,
  prefersDark,
  type Mode,
  type PaletteId,
} from "../../lib/themes";
import { cn } from "../../lib/utils";
import { Button } from "../ui/button";

export interface ThemeValue {
  palette: PaletteId;
  /** `#rrggbb`, or null for the palette's own accent. */
  accent: string | null;
}

const ACCENT_DELAY_MS = 400;

/** Palette tiles and an accent colour (swatches, or any colour), with a preview of the result. */
export function ThemePicker({
  value,
  onChange,
  mode,
  disabled = false,
}: {
  value: ThemeValue;
  onChange: (value: ThemeValue) => void;
  /** The light or dark mode the tiles are drawn in. */
  mode: Mode;
  disabled?: boolean;
}) {
  const dark = mode === "dark" || (mode === "system" && prefersDark());
  const palette = paletteOf(value.palette);
  const tokens = dark ? palette.dark : palette.light;
  const accent = value.accent ?? tokens.accent;

  // The colour box fires on every drag; save once it settles.
  const [edit, setEdit] = useState<{ from: string; colour: string } | null>(null);
  const draft = edit && edit.from === accent ? edit.colour : accent;
  const setDraft = (colour: string) => setEdit({ from: accent, colour });
  useEffect(() => {
    if (draft.toLowerCase() === accent.toLowerCase()) return;
    const timer = setTimeout(
      () => onChange({ ...value, accent: draft.toLowerCase() }),
      ACCENT_DELAY_MS,
    );
    return () => clearTimeout(timer);
  }, [draft, accent, onChange, value]);

  return (
    <div className="flex flex-col gap-4">
      <fieldset disabled={disabled} className="flex flex-col gap-1.5">
        <legend className="label-mono mb-1 text-muted-foreground">Palette</legend>
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
          {PALETTES.map((p) => {
            const t = dark ? p.dark : p.light;
            const selected = p.id === value.palette;
            return (
              <button
                key={p.id}
                type="button"
                aria-pressed={selected}
                onClick={() => onChange({ ...value, palette: p.id })}
                className={cn(
                  "flex flex-col border text-left outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60",
                  selected ? "border-2 border-foreground" : "border-border hover:border-foreground",
                )}
              >
                <span className="flex h-9" style={{ backgroundColor: t.background }}>
                  <span className="w-2/5" style={{ backgroundColor: t.card }} />
                  <span className="w-1/5" style={{ backgroundColor: value.accent ?? t.accent }} />
                </span>
                <span className="px-2 py-1 text-xs">{p.label}</span>
              </button>
            );
          })}
        </div>
      </fieldset>

      <fieldset disabled={disabled} className="flex flex-col gap-1.5">
        <legend className="label-mono mb-1 text-muted-foreground">Accent</legend>
        <div className="flex flex-wrap items-center gap-2">
          {ACCENT_SWATCHES.map((color) => (
            <button
              key={color}
              type="button"
              aria-label={`Accent ${color}`}
              aria-pressed={accent.toLowerCase() === color}
              onClick={() => onChange({ ...value, accent: color })}
              className={cn(
                "size-6 border border-border outline-none focus-visible:ring-2 focus-visible:ring-ring",
                accent.toLowerCase() === color &&
                  "ring-2 ring-foreground ring-offset-1 ring-offset-background",
              )}
              style={{ backgroundColor: color }}
            />
          ))}
          <label className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <input
              type="color"
              aria-label="Custom accent colour"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              className="size-6 cursor-pointer border border-border bg-transparent p-0"
            />
            {draft.toLowerCase()}
          </label>
          <Button
            type="button"
            variant="ghost"
            size="xs"
            disabled={value.accent === null}
            onClick={() => onChange({ ...value, accent: null })}
          >
            Palette default
          </Button>
        </div>
      </fieldset>

      <div
        className="flex items-center gap-3 border border-border p-3"
        style={{ backgroundColor: tokens.background, color: tokens.foreground }}
        aria-label="Preview"
      >
        <span
          className="px-3 py-1.5 text-sm"
          style={{ backgroundColor: accent, color: onColor(accent) }}
        >
          Preview button
        </span>
        <span className="text-sm" style={{ color: accent }}>
          A link
        </span>
        <span className="px-2 py-1 text-xs" style={{ backgroundColor: tokens.card }}>
          A card
        </span>
      </div>
    </div>
  );
}
