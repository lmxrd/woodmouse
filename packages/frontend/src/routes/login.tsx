import { useEffect, useRef, useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { PublicUser } from "woodmouse-sdk";
import {
  authStatusOptions,
  bootstrapMutation,
  loginMutation,
  loginTotpEnableMutation,
  loginTotpMutation,
  loginTotpSetupMutation,
} from "woodmouse-sdk/react-query";

import { AuthField as Field, AuthSheet, ErrorNote } from "../components/layout/AuthSheet";
import { Button, buttonVariants } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { RecoveryCodes, SetupQr } from "../components/settings/TwoFactorSection";
import { finishLogin } from "../lib/auth";
import { SSO_LOGIN_URL, ssoErrorMessage } from "../lib/sso";

/** The second step of a sign-in: a two-factor code, or setting two-factor up where it is required. */
type Step = "totp" | "totp-setup";
type Search = { error?: string; step?: Step };

export const Route = createFileRoute("/login")({
  // `error` is set by the single sign-on callback when it did not work; `step` by the callback or the
  // password form when a second step is needed (the challenge itself is in a cookie).
  validateSearch: (raw: Record<string, unknown>): Search => ({
    error: typeof raw.error === "string" && raw.error ? raw.error : undefined,
    step: raw.step === "totp" || raw.step === "totp-setup" ? raw.step : undefined,
  }),
  component: LoginPage,
});

function LoginPage() {
  const { data: status, isPending } = useQuery(authStatusOptions());
  const { error, step } = Route.useSearch();
  const navigate = useNavigate();
  const bootstrap = status?.bootstrap_required ?? false;
  const restart = () => void navigate({ to: "/login", search: {}, replace: true });

  if (step) {
    return (
      <AuthSheet
        tag="Sign in"
        title={step === "totp" ? "Two-factor code" : "Set up two-factor"}
        description={
          step === "totp"
            ? undefined
            : "This site requires a code from an authenticator app at every sign-in. Set it up to continue."
        }
      >
        {step === "totp" ? <CodeForm onRestart={restart} /> : <EnrollForm onRestart={restart} />}
      </AuthSheet>
    );
  }

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
  const login = useMutation({
    ...loginMutation(),
    meta: { inline: true, invalidate: false },
    onSuccess: (answer) => {
      // The password was right; a second step comes first (the challenge is in a cookie).
      if ("totp_setup_required" in answer) {
        const step: Step = answer.totp_setup_required ? "totp-setup" : "totp";
        void navigate({ to: "/login", search: { step }, replace: true });
      } else finishLogin(queryClient, navigate, answer);
    },
  });

  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");

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
          <a href={SSO_LOGIN_URL} className={buttonVariants({ variant: "outline", size: "lg" })}>
            Sign in with {sso}
          </a>
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
      <a href={SSO_LOGIN_URL} className={buttonVariants({ size: "lg" })}>
        Sign in with {sso}
      </a>
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

/**
 * The second step where two-factor is required and not set up yet: scan, confirm a code, keep the
 * recovery codes, then into the app.
 */
function EnrollForm({ onRestart }: { onRestart: () => void }) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [code, setCode] = useState("");
  const setup = useMutation({
    ...loginTotpSetupMutation(),
    meta: { inline: true, invalidate: false },
  });
  const enable = useMutation({
    ...loginTotpEnableMutation(),
    meta: { inline: true, invalidate: false },
  });
  // Once only: a second call (React runs effects twice in development) would replace the secret shown.
  const started = useRef(false);
  const startSetup = setup.mutate;
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    startSetup({});
  }, [startSetup]);

  if (enable.data) {
    const { user, codes } = enable.data;
    return (
      <RecoveryCodes
        codes={codes}
        doneLabel="Continue"
        onDone={() => finishLogin(queryClient, navigate, user)}
      />
    );
  }
  if (setup.isError) {
    return (
      <div className="flex flex-col gap-5">
        <ErrorNote message="This sign-in has expired. Start again." />
        <Button type="button" onClick={onRestart}>
          Start again
        </Button>
      </div>
    );
  }
  if (!setup.data) return <p className="text-sm text-muted-foreground">Loading...</p>;

  return (
    <form
      className="flex flex-col gap-5"
      onSubmit={(e) => {
        e.preventDefault();
        enable.mutate({ body: { code: code.trim() } });
      }}
    >
      <SetupQr setup={setup.data} />
      {enable.isError ? (
        <ErrorNote message="That code did not work. Check the app's clock and try the next code." />
      ) : null}
      <Field id="enroll-code" label="Code from the app">
        <Input
          id="enroll-code"
          required
          inputMode="numeric"
          autoComplete="one-time-code"
          aria-invalid={enable.isError}
          value={code}
          onChange={(e) => setCode(e.target.value)}
          className="h-10 font-mono"
        />
      </Field>
      <Button type="submit" size="lg" disabled={enable.isPending}>
        {enable.isPending ? "Checking..." : "Turn on and sign in"}
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
