import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import type { OidcSettingsView } from "woodmouse-sdk";
import { adminGetOidcOptions, adminUpdateOidcMutation } from "woodmouse-sdk/react-query";

import { Field } from "../common/Field";
import { SaveStatus } from "../common/SaveStatus";
import { Button } from "../ui/button";
import { Checkbox } from "../ui/checkbox";
import { Input } from "../ui/input";

/**
 * Single sign-on through an OpenID Connect provider. A setting the environment provides (`OIDC_*`)
 * wins and is shown read-only. Saving asks the provider first and applies at once.
 */
export function SsoTab() {
  const { data } = useQuery(adminGetOidcOptions());
  // Kept here, not in the form: the form is remounted from the saved data, which would lose it.
  const [saved, setSaved] = useState(false);
  if (!data) return null;

  return (
    <section className="flex flex-col gap-4">
      <h2 className="font-display text-xl font-bold">Single sign-on</h2>
      <p className="text-sm text-muted-foreground">
        {data.active
          ? "Single sign-on is on: the login page offers it next to the password."
          : "Single sign-on is off. Passwords always work."}{" "}
        Sign-in works with any OpenID Connect provider (Keycloak, Authentik, Authelia, Google, ...).
      </p>
      <div className="flex flex-col gap-1 border border-border bg-card p-5">
        <h3 className="label-mono text-muted-foreground">Callback URL to register</h3>
        {data.callback_url ? (
          <code className="text-sm break-all">{data.callback_url}</code>
        ) : (
          <p className="text-sm text-muted-foreground">
            Unknown: set <code>PUBLIC_URL</code> or the redirect URL below.
          </p>
        )}
        <p className="text-sm text-muted-foreground">
          Over https, <code>WOODMOUSE_SECURE_COOKIES</code> must be on; over plain http, off.
        </p>
      </div>
      <SsoForm
        key={JSON.stringify(data)}
        data={data}
        onSaving={() => setSaved(false)}
        onSaved={() => setSaved(true)}
      />
      {saved ? <SaveStatus status="success" /> : null}
      <p className="text-sm text-muted-foreground">
        An account is matched by the provider&apos;s user id once linked, and the first time by an
        email the provider says is verified. Providers that never say so (Microsoft Entra ID) cannot
        match existing accounts by email; their users need accounts created automatically, or to be
        linked some other way. With automatic accounts on, anyone the provider lets sign in gets an
        account with no workspaces. Only use a provider you trust to verify emails: any account,
        yours included, is linked to whoever first signs in with its email.
      </p>
    </section>
  );
}

function SsoForm({
  data,
  onSaving,
  onSaved,
}: {
  data: OidcSettingsView;
  onSaving: () => void;
  onSaved: () => void;
}) {
  const locked = data.locked;
  const [enabled, setEnabled] = useState(data.enabled);
  const [autoCreate, setAutoCreate] = useState(data.auto_create);
  const [removeSecret, setRemoveSecret] = useState(false);
  const save = useMutation({
    ...adminUpdateOidcMutation(),
    meta: { inline: true },
    onSuccess: onSaved,
  });
  const fromEnv = (variable: string) => (
    <p className="font-mono text-[11px] text-muted-foreground">Set by {variable}</p>
  );

  return (
    <form
      className="flex flex-col gap-4 border border-border bg-card p-5"
      onSubmit={(e) => {
        e.preventDefault();
        onSaving();
        const f = new FormData(e.currentTarget);
        const text = (k: string) => String(f.get(k) ?? "").trim() || null;
        const secret = String(f.get("client_secret") ?? "");
        // A field the environment sets is left out: the server refuses to change it.
        save.mutate({
          body: {
            ...(locked.enabled ? {} : { enabled }),
            ...(locked.issuer ? {} : { issuer: text("issuer") }),
            ...(locked.client_id ? {} : { client_id: text("client_id") }),
            ...(locked.client_secret
              ? {}
              : removeSecret
                ? { remove_secret: true }
                : secret
                  ? { client_secret: secret }
                  : {}),
            ...(locked.display_name ? {} : { display_name: text("display_name") }),
            ...(locked.redirect_url ? {} : { redirect_url: text("redirect_url") }),
            ...(locked.auto_create ? {} : { auto_create: autoCreate }),
          },
        });
      }}
    >
      <label className="flex items-center gap-2 text-sm">
        <Checkbox
          checked={enabled}
          disabled={locked.enabled}
          onCheckedChange={(v) => setEnabled(v === true)}
        />
        Offer single sign-on
      </label>
      {locked.enabled ? fromEnv("OIDC_ENABLED") : null}
      <Field label="Issuer URL" htmlFor="oidc-issuer">
        <Input
          id="oidc-issuer"
          name="issuer"
          type="url"
          readOnly={locked.issuer}
          defaultValue={data.issuer ?? ""}
          placeholder="https://id.example.com/realms/home"
        />
        {locked.issuer ? fromEnv("OIDC_ISSUER") : null}
      </Field>
      <Field label="Client id" htmlFor="oidc-client-id">
        <Input
          id="oidc-client-id"
          name="client_id"
          autoComplete="off"
          readOnly={locked.client_id}
          defaultValue={data.client_id ?? ""}
        />
        {locked.client_id ? fromEnv("OIDC_CLIENT_ID") : null}
      </Field>
      <Field label="Client secret" htmlFor="oidc-client-secret">
        <Input
          id="oidc-client-secret"
          name="client_secret"
          type="password"
          autoComplete="new-password"
          readOnly={locked.client_secret}
          disabled={removeSecret}
          placeholder={
            locked.client_secret
              ? "From the environment"
              : data.has_secret
                ? "Saved. Type to replace"
                : "None (a public client)"
          }
        />
        {locked.client_secret ? (
          fromEnv("OIDC_CLIENT_SECRET")
        ) : data.has_secret ? (
          <label className="flex items-center gap-2 text-sm">
            <Checkbox checked={removeSecret} onCheckedChange={(v) => setRemoveSecret(v === true)} />
            Remove the saved secret
          </label>
        ) : null}
        <p className="font-mono text-[11px] text-muted-foreground">
          Stored as given and never shown again. A new issuer needs its secret entered again.
        </p>
      </Field>
      <Field label="Button name" htmlFor="oidc-display-name">
        <Input
          id="oidc-display-name"
          name="display_name"
          readOnly={locked.display_name}
          defaultValue={data.display_name ?? ""}
          placeholder="single sign-on"
        />
        {locked.display_name ? fromEnv("OIDC_DISPLAY_NAME") : null}
      </Field>
      <Field label="Redirect URL (optional)" htmlFor="oidc-redirect-url">
        <Input
          id="oidc-redirect-url"
          name="redirect_url"
          type="url"
          readOnly={locked.redirect_url}
          defaultValue={data.redirect_url ?? ""}
          placeholder="https://woodmouse.example.com/api/auth/oidc/callback"
        />
        {locked.redirect_url ? fromEnv("OIDC_REDIRECT_URL") : null}
      </Field>
      <label className="flex items-center gap-2 text-sm">
        <Checkbox
          checked={autoCreate}
          disabled={locked.auto_create}
          onCheckedChange={(v) => setAutoCreate(v === true)}
        />
        Create an account for someone new whose provider confirms their email
      </label>
      {locked.auto_create ? fromEnv("OIDC_AUTO_CREATE") : null}
      {save.isError ? (
        <p role="alert" className="text-sm text-destructive">
          {errorText(save.error)}
        </p>
      ) : null}
      <div>
        <Button type="submit" disabled={save.isPending}>
          {save.isPending ? "Checking the provider..." : "Save"}
        </Button>
      </div>
    </form>
  );
}

/** The server's reason (it says what is wrong with the provider or a field). */
function errorText(error: unknown): string {
  const message =
    typeof error === "object" && error !== null && "error" in error
      ? (error as { error: unknown }).error
      : null;
  return typeof message === "string" ? `Couldn't save: ${message}` : "Couldn't save.";
}
