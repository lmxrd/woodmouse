import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import type { CreatedApiKey, Role } from "woodmouse-sdk";
import {
  createApiKeyMutation,
  listApiKeysOptions,
  meOptions,
  revokeApiKeyMutation,
} from "woodmouse-sdk/react-query";

import { useFormatters } from "../../lib/useFormatters";
import { ConfirmDialog } from "../common/ConfirmDialog";
import { Field } from "../common/Field";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { Section } from "./AccountPage";

const SCOPE_LABEL: Record<Role, string> = {
  read: "Read",
  write: "Read & write",
  admin: "Admin",
};
const NEVER = "never";
const EXPIRIES = [
  { value: NEVER, label: "Never" },
  { value: "30", label: "30 days" },
  { value: "90", label: "90 days" },
  { value: "365", label: "1 year" },
];

/**
 * The person's API keys, for scripts (`Authorization: Bearer wm_...`). A key acts as them, capped at its
 * permission; it can never manage keys, two-factor, the password or the email. The token is shown once.
 */
export function ApiKeysSection() {
  const { data: me } = useQuery(meOptions());
  const { data: keys } = useQuery(listApiKeysOptions());
  const { dateOf, when } = useFormatters();
  const [created, setCreated] = useState<CreatedApiKey | null>(null);
  const [revoking, setRevoking] = useState<{ id: string; name: string } | null>(null);
  const revoke = useMutation({ ...revokeApiKeyMutation() });
  if (!me) return null;
  const isAdmin = me.super_admin || me.global_role === "admin";

  return (
    <Section title="API keys">
      <p className="text-sm text-muted-foreground">
        For scripts and integrations: send a key as{" "}
        <code className="font-mono text-xs">Authorization: Bearer &lt;key&gt;</code>. It acts as
        you, never with more than its permission.
      </p>
      {created ? (
        <NewToken created={created} onDone={() => setCreated(null)} />
      ) : (
        <CreateForm isAdmin={isAdmin} onCreated={setCreated} />
      )}
      {keys && keys.length > 0 ? (
        <div className="flex flex-col border border-border">
          {keys.map((key) => (
            <div
              key={key.id}
              className="flex flex-wrap items-center gap-x-4 gap-y-1 border-b border-border px-3 py-2 last:border-b-0"
            >
              <span className="min-w-0 flex-1">
                <span className="block truncate font-medium">{key.name}</span>
                <span className="block font-mono text-xs text-muted-foreground">{key.prefix}…</span>
              </span>
              <span className="label-mono text-muted-foreground">
                {[
                  SCOPE_LABEL[key.scope],
                  `created ${dateOf(key.created_at)}`,
                  key.last_used_at ? `used ${when(key.last_used_at)}` : "never used",
                  key.expires_at ? `expires ${dateOf(key.expires_at)}` : null,
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </span>
              <Button
                variant="outline"
                size="sm"
                onClick={() => setRevoking({ id: key.id, name: key.name })}
              >
                Revoke
              </Button>
            </div>
          ))}
        </div>
      ) : null}
      <ConfirmDialog
        open={revoking !== null}
        onOpenChange={(open) => !open && setRevoking(null)}
        title={`Revoke ${revoking?.name ?? "this key"}?`}
        description="Anything using it stops working at once. This cannot be undone."
        confirmLabel="Revoke"
        onConfirm={() => revoking && revoke.mutate({ path: { id: revoking.id } })}
      />
    </Section>
  );
}

function CreateForm({
  isAdmin,
  onCreated,
}: {
  isAdmin: boolean;
  onCreated: (created: CreatedApiKey) => void;
}) {
  const [name, setName] = useState("");
  const [scope, setScope] = useState<Role>("read");
  const [expiry, setExpiry] = useState(NEVER);
  const create = useMutation({
    ...createApiKeyMutation(),
    meta: { inline: true },
    onSuccess: (data) => {
      setName("");
      onCreated(data);
    },
  });

  return (
    <form
      className="flex flex-wrap items-end gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        create.mutate({
          body: {
            name: name.trim(),
            scope,
            expires_in_days: expiry === NEVER ? null : Number(expiry),
          },
        });
      }}
    >
      <Field label="Name" htmlFor="api-key-name">
        <Input
          id="api-key-name"
          required
          maxLength={100}
          placeholder="What it is for"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="w-56"
        />
      </Field>
      <Field label="Permission">
        <Select value={scope} onValueChange={(v) => setScope(v as Role)}>
          <SelectTrigger aria-label="Permission" className="w-40">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="read">{SCOPE_LABEL.read}</SelectItem>
            <SelectItem value="write">{SCOPE_LABEL.write}</SelectItem>
            {isAdmin ? <SelectItem value="admin">{SCOPE_LABEL.admin}</SelectItem> : null}
          </SelectContent>
        </Select>
      </Field>
      <Field label="Expires">
        <Select value={expiry} onValueChange={setExpiry}>
          <SelectTrigger aria-label="Expires" className="w-32">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {EXPIRIES.map((e) => (
              <SelectItem key={e.value} value={e.value}>
                {e.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>
      <Button type="submit" disabled={create.isPending || !name.trim()}>
        Create key
      </Button>
      {create.isError ? (
        <span role="alert" className="text-sm text-destructive">
          Couldn't create the key.
        </span>
      ) : null}
    </form>
  );
}

function NewToken({ created, onDone }: { created: CreatedApiKey; onDone: () => void }) {
  const [copied, setCopied] = useState(false);
  return (
    <div role="status" className="flex flex-col gap-2 border border-primary bg-accent p-3">
      <p className="text-sm">
        Copy the key for <b className="font-medium">{created.key.name}</b> now. It is not shown
        again.
      </p>
      <div className="flex gap-2">
        <Input
          readOnly
          value={created.token}
          aria-label="API key"
          onFocus={(e) => e.target.select()}
          className="font-mono"
        />
        <Button
          type="button"
          onClick={() => {
            void navigator.clipboard.writeText(created.token).then(() => setCopied(true));
          }}
        >
          {copied ? "Copied" : "Copy"}
        </Button>
        <Button type="button" variant="ghost" onClick={onDone}>
          Done
        </Button>
      </div>
    </div>
  );
}
