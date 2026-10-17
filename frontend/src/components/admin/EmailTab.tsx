import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  adminGetEmailSettingsOptions,
  adminGetEmailSettingsQueryKey,
  adminTestEmailMutation,
  adminUpdateEmailSettingsMutation,
} from "woodmouse-client/react-query";

import { Field } from "../common/Field";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";

/**
 * SMTP settings. What is typed here is stored in the database and overrides the matching `SMTP_*`
 * environment variable; a field left empty falls back to the environment.
 */
export function EmailTab() {
  const queryClient = useQueryClient();
  const { data } = useQuery(adminGetEmailSettingsOptions());
  const [tls, setTls] = useState<string | null>(null);
  const [testTo, setTestTo] = useState("");
  const save = useMutation({
    ...adminUpdateEmailSettingsMutation(),
    meta: { inline: true },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: adminGetEmailSettingsQueryKey() }),
  });
  const test = useMutation({ ...adminTestEmailMutation(), meta: { inline: true } });
  if (!data) return null;
  const env = data.env;

  return (
    <section className="flex flex-col gap-4">
      <h2 className="font-display text-xl font-bold">Email</h2>
      <p className="text-sm text-muted-foreground">
        {data.configured
          ? "Email is on: notification emails, summaries and invite links can be sent."
          : "Email is off until a host and a from address are set here or in the environment."}{" "}
        Invite links are only emailed when <code>PUBLIC_URL</code> is set.
      </p>
      <form
        key={JSON.stringify([data.host, data.port, data.username, data.from_address, data.tls])}
        className="flex flex-col gap-4 border border-border bg-card p-5"
        onSubmit={(e) => {
          e.preventDefault();
          const f = new FormData(e.currentTarget);
          const text = (k: string) => String(f.get(k) ?? "").trim() || null;
          const port = Number(f.get("port"));
          const password = String(f.get("password") ?? "");
          save.mutate({
            body: {
              host: text("host"),
              port: port > 0 ? port : null,
              username: text("username"),
              from_address: text("from"),
              tls: tls ?? data.tls ?? null,
              ...(password ? { password } : {}),
            },
          });
        }}
      >
        <Field label="SMTP host" htmlFor="smtp-host">
          <Input
            id="smtp-host"
            name="host"
            defaultValue={data.host ?? ""}
            placeholder={env.host ?? "smtp.example.com"}
          />
        </Field>
        <div className="flex flex-wrap gap-4">
          <Field label="Port" htmlFor="smtp-port">
            <Input
              id="smtp-port"
              name="port"
              type="number"
              className="w-28"
              defaultValue={data.port ?? ""}
              placeholder={String(env.port ?? 587)}
            />
          </Field>
          <Field label="Security">
            <Select value={tls ?? data.tls ?? env.tls} onValueChange={setTls}>
              <SelectTrigger aria-label="Security" className="w-40">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="starttls">STARTTLS</SelectItem>
                <SelectItem value="tls">TLS</SelectItem>
                <SelectItem value="none">None</SelectItem>
              </SelectContent>
            </Select>
          </Field>
        </div>
        <Field label="User name" htmlFor="smtp-user">
          <Input
            id="smtp-user"
            name="username"
            autoComplete="off"
            defaultValue={data.username ?? ""}
            placeholder={env.username ?? ""}
          />
        </Field>
        <Field label="Password" htmlFor="smtp-password">
          <Input
            id="smtp-password"
            name="password"
            type="password"
            autoComplete="new-password"
            placeholder={
              data.has_password
                ? "Saved. Type to replace"
                : env.has_password
                  ? "From the environment"
                  : ""
            }
          />
        </Field>
        <Field label="From address" htmlFor="smtp-from">
          <Input
            id="smtp-from"
            name="from"
            defaultValue={data.from_address ?? ""}
            placeholder={env.from_address ?? "Woodmouse <woodmouse@example.com>"}
          />
        </Field>
        {save.isError ? (
          <p role="alert" className="text-sm text-destructive">
            Couldn't save. Check the values.
          </p>
        ) : null}
        <div>
          <Button type="submit" disabled={save.isPending}>
            Save
          </Button>
        </div>
      </form>
      <div className="flex flex-col gap-2 border border-border bg-card p-5">
        <h3 className="label-mono text-muted-foreground">Send a test email</h3>
        <div className="flex flex-wrap gap-2">
          <Input
            aria-label="Send the test to"
            className="max-w-xs"
            value={testTo}
            placeholder="Your own address"
            onChange={(e) => setTestTo(e.target.value)}
          />
          <Button
            variant="outline"
            disabled={test.isPending || !data.configured}
            onClick={() => test.mutate({ body: { to: testTo.trim() || null } })}
          >
            Send test
          </Button>
        </div>
        {test.isSuccess ? <p className="text-sm">Sent to {test.data.sent_to}.</p> : null}
        {test.isError ? (
          <p role="alert" className="text-sm text-destructive">
            The server could not send it. Check the settings.
          </p>
        ) : null}
      </div>
    </section>
  );
}
