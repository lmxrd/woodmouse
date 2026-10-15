import { useState } from "react";

import { Button } from "./ui/button";
import { Input } from "./ui/input";

/** A dashed "+ Label" button that turns into a one-line form; Enter submits, Escape cancels. */
export function InlineAdd({
  label,
  placeholder,
  onSubmit,
  className,
}: {
  label: string;
  placeholder: string;
  onSubmit: (name: string) => void;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState("");

  if (!open) {
    return (
      <Button
        type="button"
        variant="ghost"
        size="sm"
        className={`justify-start border border-dashed font-normal text-muted-foreground ${className ?? ""}`}
        onClick={() => setOpen(true)}
      >
        + {label}
      </Button>
    );
  }

  const close = () => {
    setOpen(false);
    setValue("");
  };

  return (
    <form
      className={className}
      onSubmit={(e) => {
        e.preventDefault();
        const name = value.trim();
        if (name) onSubmit(name);
        close();
      }}
    >
      <Input
        autoFocus
        value={value}
        placeholder={placeholder}
        aria-label={placeholder}
        onChange={(e) => setValue(e.target.value)}
        onBlur={close}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
        }}
      />
    </form>
  );
}
