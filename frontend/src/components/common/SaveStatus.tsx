type Status = "idle" | "pending" | "success" | "error";

/** Tiny autosave indicator. A Save button can replace or sit beside it later. */
export function SaveStatus({ status }: { status: Status }) {
  if (status === "idle") return <span className="label-mono invisible">Saved</span>;
  return (
    <span
      role="status"
      className={
        status === "error" ? "label-mono text-destructive" : "label-mono text-muted-foreground"
      }
    >
      {status === "pending" ? "Saving…" : status === "success" ? "Saved" : "Couldn't save"}
    </span>
  );
}
