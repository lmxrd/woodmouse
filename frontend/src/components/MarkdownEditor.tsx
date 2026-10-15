import { useState } from "react";

import { cn } from "../lib/utils";
import { Markdown } from "./Markdown";
import { Textarea } from "./ui/textarea";

/**
 * A markdown text box with Write and Preview tabs. Controlled: the owner decides when to save.
 * `initialMode` lets a filled-in description open rendered, and an empty composer open for typing.
 */
export function MarkdownEditor({
  id,
  value,
  onChange,
  onBlur,
  onSubmit,
  rows = 5,
  placeholder,
  initialMode = "write",
  readOnly = false,
}: {
  id: string;
  value: string;
  onChange: (value: string) => void;
  onBlur?: () => void;
  /** Called on Ctrl or Cmd + Enter. */
  onSubmit?: () => void;
  rows?: number;
  placeholder?: string;
  initialMode?: "write" | "preview";
  readOnly?: boolean;
}) {
  const [mode, setMode] = useState<"write" | "preview">(readOnly ? "preview" : initialMode);
  const tab = (name: "write" | "preview", label: string) => (
    <button
      type="button"
      role="tab"
      aria-selected={mode === name}
      onClick={() => setMode(name)}
      className={cn(
        "px-3 py-1 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring",
        mode === name
          ? "border-b-2 border-foreground font-medium"
          : "text-muted-foreground hover:text-foreground",
      )}
    >
      {label}
    </button>
  );

  return (
    <div className="flex flex-col">
      <div className="flex items-end justify-between border-b border-border">
        {readOnly ? (
          <span />
        ) : (
          <div role="tablist" aria-label="Editor mode" className="flex">
            {tab("write", "Write")}
            {tab("preview", "Preview")}
          </div>
        )}
        <span className="label-mono pb-1 text-[10px] text-muted-foreground">Markdown</span>
      </div>
      {mode === "write" && !readOnly ? (
        <Textarea
          id={id}
          rows={rows}
          value={value}
          placeholder={placeholder}
          className="rounded-t-none border-t-0"
          onChange={(e) => onChange(e.target.value)}
          onBlur={onBlur}
          onKeyDown={(e) => {
            if (onSubmit && e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              onSubmit();
            }
          }}
        />
      ) : (
        <div
          className="min-h-20 border border-t-0 border-input bg-card px-3 py-2"
          onDoubleClick={() => !readOnly && setMode("write")}
        >
          {value.trim() ? (
            <Markdown>{value}</Markdown>
          ) : (
            <p className="text-sm text-muted-foreground">Nothing to preview.</p>
          )}
        </div>
      )}
    </div>
  );
}
