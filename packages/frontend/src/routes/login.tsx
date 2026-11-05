import { useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { PublicUser } from "woodmouse-sdk";
import {
  authStatusOptions,
  bootstrapMutation,
  loginMutation,
  loginTotpMutation,
} from "woodmouse-sdk/react-query";

import { AuthField as Field, AuthSheet, ErrorNote } from "../components/layout/AuthSheet";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { finishLogin } from "../lib/auth";
import { SSO_LOGIN_URL, ssoErrorMessage } from "../lib/sso";

type Search = { error?: string };

export const Route = createFileRoute("/login")({
  // `error` is set by the single sign-on callback when it did not work.
  validateSearch: (raw: Record<string, unknown>): Search => ({
    error: typeof raw.error === "string" && raw.error ? raw.error : undefined,
  }),
  component: LoginPage,
});

function LoginPage() {
  const { data: status, isPending } = useQuery(authStatusOptions());
  const { error } = Route.useSearch();
  const bootstrap = status?.bootstrap_required ?? false;

  return (
    <AuthSheet
      tag={bootstrap ? "First run" : "Sign in"}
      title={bootstrap ? "Set up Woodmouse" : "Log in"}
      description={
        bootstrap
          ? "Create the first account. It becomes the system super admin. After this, admins create every other account."
          : "Sign in to your workspaces."
      }
    >
      {isPending ? <p className="text-sm text-muted-foreground">Loading...</p> : null}
      {status && !bootstrap && ssoErrorMessage(error) ? (
        <ErrorNote message={ssoErrorMessage(error) ?? ""} />
      ) : null}
      {status &&
        (bootstrap ? (
          <BootstrapForm />
        ) : status.password_login ? (
          <LoginForm sso={status.sso ?? null} />
        ) : (
          <SsoOnly sso={status.sso ?? "single sign-on"} />
        ))}
    </AuthSheet>
  );
}

function LoginForm({ sso }: { sso: string | null }) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  // Set once the password was right and a two-factor code is needed too.
  const [needsCode, setNeedsCode] = useState(false);
  const login = useMutation({
    ...loginMutation(),
    meta: { inline: true, invalidate: false },
    onSuccess: (answer) => {
      if ("totp_required" in answer) setNeedsCode(true);
      else finishLogin(queryClient, navigate, answer);
    },
  });

  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");

  if (needsCode) return <CodeForm onRestart={() => setNeedsCode(false)} />;

  return (
    <form
      className="flex flex-col gap-5"
      onSubmit={(e) => {
        e.preventDefault();
        // A single sign-on error no longer applies once they try their password.
        void navigate({ to: "/login", search: {}, replace: true });
        login.mutate({ body: { email, password } });
      }}
    >
      {sso ? (
        <>
          <Button asChild variant="outline" size="lg">
            <a href={SSO_LOGIN_URL}>Sign in with {sso}</a>
          </Button>
          <p className="label-mono text-center text-muted-foreground">or with your password</p>
        </>
      ) : null}
      {login.isError ? (
        <ErrorNote message="That did not work. Check your email and password." />
      ) : null}
      <Field id="email" label="Email">
        <Input
          id="email"
          type="email"
          required
          autoComplete="email"
          placeholder="you@home.example"
          aria-invalid={login.isError}
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          className="h-10"
        />
      </Field>
      <Field id="password" label="Password">
        <Input
          id="password"
          type="password"
          required
          autoComplete="current-password"
          aria-invalid={login.isError}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className="h-10"
        />
      </Field>
      <Button type="submit" size="lg" disabled={login.isPending}>
        {login.isPending ? "Signing in..." : "Log in"}
      </Button>
      <p className="text-sm text-muted-foreground">
        No account? Ask a workspace admin to invite you.
      </p>
    </form>
  );
}

/** Single sign-on is the only way in (`OIDC_ONLY`): no password form. */
function SsoOnly({ sso }: { sso: string }) {
  return (
    <div className="flex flex-col gap-5">
      <Button asChild size="lg">
        <a href={SSO_LOGIN_URL}>Sign in with {sso}</a>
      </Button>
      <p className="text-sm text-muted-foreground">
        No account? Ask a workspace admin to invite you.
      </p>
    </div>
  );
}

/** The second step for someone with two-factor sign-in on: a code from their app, or a recovery code. */
function CodeForm({ onRestart }: { onRestart: () => void }) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [code, setCode] = useState("");
  const verify = useMutation({
    ...loginTotpMutation(),
    meta: { inline: true, invalidate: false },
    onSuccess: (user: PublicUser) => finishLogin(queryClient, navigate, user),
  });

  return (
    <form
      className="flex flex-col gap-5"
      onSubmit={(e) => {
        e.preventDefault();
        verify.mutate({ body: { code: code.trim() } });
      }}
    >
      <p className="text-sm text-muted-foreground">
        Enter the 6-digit code from your authenticator app, or one of your recovery codes.
      </p>
      {verify.isError ? (
        <ErrorNote message="That code did not work. Check it, or start again if more than five minutes have passed." />
      ) : null}
      <Field id="code" label="Code">
        <Input
          id="code"
          required
          autoFocus
          autoComplete="one-time-code"
          inputMode="text"
          aria-invalid={verify.isError}
          value={code}
          onChange={(e) => setCode(e.target.value)}
          className="h-10 font-mono"
        />
      </Field>
      <Button type="submit" size="lg" disabled={verify.isPending}>
        {verify.isPending ? "Checking..." : "Verify"}
      </Button>
      <Button type="button" variant="ghost" onClick={onRestart}>
        Start again
      </Button>
    </form>
  );
}

function BootstrapForm() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const bootstrapAccount = useMutation({
    ...bootstrapMutation(),
    meta: { inline: true, invalidate: false },
    onSuccess: (user: PublicUser) => finishLogin(queryClient, navigate, user),
  });

  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");

  return (
    <form
      className="flex flex-col gap-5"
      onSubmit={(e) => {
        e.preventDefault();
        bootstrapAccount.mutate({ body: { name, email, password } });
      }}
    >
      {bootstrapAccount.isError ? (
        <ErrorNote message="Could not create the account. Check the details and try again." />
      ) : null}
      <Field id="name" label="Your name (optional)">
        <Input
          id="name"
          autoComplete="name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="h-10"
        />
      </Field>
      <Field id="email" label="Email">
        <Input
          id="email"
          type="email"
          required
          autoComplete="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          className="h-10"
        />
      </Field>
      <Field id="password" label="Password" hint="min 8 characters">
        <Input
          id="password"
          type="password"
          required
          minLength={8}
          autoComplete="new-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className="h-10"
        />
      </Field>
      <Button type="submit" size="lg" disabled={bootstrapAccount.isPending}>
        {bootstrapAccount.isPending ? "Creating..." : "Create admin account"}
      </Button>
    </form>
  );
}
