import { useState } from "react";

import { Button } from "../ui/button";
import { Input } from "../ui/input";

/** Shows a one-time invite link with a copy button. The admin shares it by hand. */
export function InviteLinkBox({
  name,
  token,
  onDismiss,
}: {
  name: string;
  token: string;
  onDismiss: () => void;
}) {
  const url = `${window.location.origin}/invite/${token}`;
  const [copied, setCopied] = useState(false);

  return (
    <div role="status" className="flex flex-col gap-2 border border-primary bg-accent p-3">
      <p className="text-sm">
        Share this link with <b className="font-medium">{name}</b>. It works once and expires in 7
        days.
      </p>
      <div className="flex gap-2">
        <Input readOnly value={url} aria-label="Invite link" onFocus={(e) => e.target.select()} />
        <Button
          type="button"
          onClick={() => {
            void navigator.clipboard.writeText(url).then(() => setCopied(true));
          }}
        >
          {copied ? "Copied" : "Copy"}
        </Button>
        <Button type="button" variant="ghost" onClick={onDismiss}>
          Done
        </Button>
      </div>
    </div>
  );
}
