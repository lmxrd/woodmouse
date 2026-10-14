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

export const Route = createFileRoute("/login")({
  component: LoginPage,
});

function LoginPage() {
  const { data: status, isPending } = useQuery(authStatusOptions());
  const bootstrap = status?.bootstrap_required ?? false;

  return (
    <section className="mx-auto max-w-sm space-y-6">
      <h1 className="text-2xl font-bold tracking-tight">
        {bootstrap ? "Set up woodmouse" : "Log in"}
      </h1>
      <p className="text-sm text-gray-600">
        {bootstrap
          ? "Create the first account. It becomes the system super admin; afterwards accounts are created by admins."
          : "Sign in to your workspaces."}
      </p>
      {isPending ? <p className="text-gray-500">Loading...</p> : null}
      {status && (bootstrap ? <BootstrapForm /> : <LoginForm />)}
    </section>
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
      className="space-y-4"
      onSubmit={(e) => {
        e.preventDefault();
        login.mutate({ body: { email, password } });
      }}
    >
      <Field label="Email">
        <input
          type="email"
          required
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          className={inputClass}
        />
      </Field>
      <Field label="Password">
        <input
          type="password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className={inputClass}
        />
      </Field>
      <button type="submit" disabled={login.isPending} className={buttonClass}>
        {login.isPending ? "Signing in..." : "Log in"}
      </button>
      {login.isError ? <ErrorNote /> : null}
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
      className="space-y-4"
      onSubmit={(e) => {
        e.preventDefault();
        bootstrapAccount.mutate({ body: { name, email, password } });
      }}
    >
      <Field label="Your name">
        <input
          required
          value={name}
          onChange={(e) => setName(e.target.value)}
          className={inputClass}
        />
      </Field>
      <Field label="Email">
        <input
          type="email"
          required
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          className={inputClass}
        />
      </Field>
      <Field label="Password (min 8 characters)">
        <input
          type="password"
          required
          minLength={8}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className={inputClass}
        />
      </Field>
      <button type="submit" disabled={bootstrapAccount.isPending} className={buttonClass}>
        {bootstrapAccount.isPending ? "Creating..." : "Create admin account"}
      </button>
      {bootstrapAccount.isError ? <ErrorNote /> : null}
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

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block space-y-1">
      <span className="text-sm font-medium text-gray-700">{label}</span>
      {children}
    </label>
  );
}

const inputClass =
  "w-full rounded-md border border-gray-300 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none";
const buttonClass =
  "w-full rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50";

function ErrorNote() {
  return <p className="text-sm text-red-600">That did not work. Check your credentials.</p>;
}
