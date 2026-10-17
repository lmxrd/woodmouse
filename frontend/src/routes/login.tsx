import { useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { PublicUser } from "woodmouse-client";
import { authStatusOptions, bootstrapMutation, loginMutation } from "woodmouse-client/react-query";

import { AuthField as Field, AuthSheet, ErrorNote } from "../components/layout/AuthSheet";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { finishLogin } from "../lib/auth";

export const Route = createFileRoute("/login")({
  component: LoginPage,
});

function LoginPage() {
  const { data: status, isPending } = useQuery(authStatusOptions());
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
      {status && (bootstrap ? <BootstrapForm /> : <LoginForm />)}
    </AuthSheet>
  );
}

function LoginForm() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const login = useMutation({
    ...loginMutation(),
    meta: { inline: true, invalidate: false },
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
        No account? Ask a workspace admin to invite you.
      </p>
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
