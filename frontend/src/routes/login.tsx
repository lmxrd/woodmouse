import { useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { PublicUser } from "woodmouse-client";
import {
  authStatusOptions,
  bootstrapMutation,
  loginMutation,
  meQueryKey,
} from "woodmouse-client/react-query";

import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Label } from "../components/ui/label";

export const Route = createFileRoute("/login")({
  component: LoginPage,
});

function LoginPage() {
  const { data: status, isPending } = useQuery(authStatusOptions());
  const bootstrap = status?.bootstrap_required ?? false;

  return (
    <main className="grid min-h-screen place-items-center px-4 py-10">
      <section className="w-full max-w-md border border-border bg-card">
        <header className="flex items-center justify-between border-b-2 border-foreground px-6 py-3">
          <span className="font-display text-lg font-bold">Woodmouse</span>
          <span className="label-mono text-[10px] text-muted-foreground">
            {bootstrap ? "First run" : "Sign in"}
          </span>
        </header>
        <div className="flex flex-col gap-5 p-6">
          <div className="flex flex-col gap-2">
            <h1 className="font-display text-3xl leading-tight font-bold">
              {bootstrap ? "Set up Woodmouse" : "Log in"}
            </h1>
            <p className="max-w-[44ch] text-sm leading-6 text-muted-foreground">
              {bootstrap
                ? "Create the first account. It becomes the system super admin. After this, admins create every other account."
                : "Sign in to your workspaces."}
            </p>
          </div>
          {isPending ? <p className="text-sm text-muted-foreground">Loading...</p> : null}
          {status && (bootstrap ? <BootstrapForm /> : <LoginForm />)}
        </div>
        <footer className="label-mono border-t border-border px-6 py-2.5 text-[10px] text-muted-foreground">
          Self-hosted
        </footer>
      </section>
    </main>
  );
}

function LoginForm() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const login = useMutation({
    ...loginMutation(),
    onSuccess: (user: PublicUser) => finishLogin(queryClient, navigate, user),
  });

  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");

  return (
    <form
      className="flex flex-col gap-5"
      onSubmit={(e) => {
        e.preventDefault();
        login.mutate({ body: { email, password } });
      }}
    >
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
        No account? Ask a workspace admin to create one for you.
      </p>
    </form>
  );
}

function BootstrapForm() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const bootstrapAccount = useMutation({
    ...bootstrapMutation(),
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
      <Field id="name" label="Your name">
        <Input
          id="name"
          required
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

function finishLogin(
  queryClient: ReturnType<typeof useQueryClient>,
  navigate: ReturnType<typeof useNavigate>,
  user: PublicUser,
) {
  queryClient.setQueryData(meQueryKey(), user);
  void navigate({ to: "/" });
}

function Field({
  id,
  label,
  hint,
  children,
}: {
  id: string;
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={id} className="label-mono text-[11px] text-muted-foreground">
        {label}
      </Label>
      {children}
      {hint ? <p className="font-mono text-[11px] text-muted-foreground">{hint}</p> : null}
    </div>
  );
}

function ErrorNote({ message }: { message: string }) {
  return (
    <div role="alert" className="flex gap-3 border border-destructive bg-card px-3 py-2.5 text-sm">
      <span className="font-mono font-medium text-destructive">ERR</span>
      <span>{message}</span>
    </div>
  );
}
