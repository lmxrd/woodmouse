import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import type { PublicUser } from "woodmouse-client";
import {
  changePasswordMutation,
  meOptions,
  meQueryKey,
  updateMeMutation,
} from "woodmouse-client/react-query";

import { DATE_FORMATS, TIME_FORMATS, formatDateTime } from "../lib/format";
import { CalendarLinks } from "./calendarlinks/CalendarLinks";
import { Field } from "./Field";
import { SaveStatus } from "./SaveStatus";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";

const TIME_LABEL = { "24h": "24 hour", "12h": "12 hour" } as const;

/** The signed-in user's own settings: profile, password, and how dates and times are shown. */
export function AccountPage() {
  const { data: me } = useQuery(meOptions());
  if (!me) return null;
  return (
    <main className="mx-auto flex max-w-2xl flex-col gap-8 px-4 py-8 sm:px-6">
      <header className="flex flex-col gap-1">
        <Link to="/" className="label-mono text-muted-foreground hover:text-foreground">
          ← Back
        </Link>
        <h1 className="font-display text-3xl font-bold">Account</h1>
      </header>
      <ProfileSection key={me.id + me.email + me.name} me={me} />
      <DisplaySection me={me} />
      <Section title="Calendar links">
        <CalendarLinks scopes={["user", "project", "workspace"]} />
      </Section>
      <PasswordSection />
    </main>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-4 border border-border bg-card p-5">
      <h2 className="label-mono text-muted-foreground">{title}</h2>
      {children}
    </section>
  );
}

function useUpdateMe() {
  const queryClient = useQueryClient();
  return useMutation({
    ...updateMeMutation(),
    meta: { inline: true },
    onSuccess: (user: PublicUser) => queryClient.setQueryData(meQueryKey(), user),
  });
}

function ProfileSection({ me }: { me: PublicUser }) {
  const update = useUpdateMe();
  const [name, setName] = useState(me.name);
  const [email, setEmail] = useState(me.email);
  const dirty = name.trim() !== me.name || email.trim().toLowerCase() !== me.email;

  return (
    <Section title="Profile">
      <form
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          update.mutate({ body: { name: name.trim(), email: email.trim() } });
        }}
      >
        <Field label="Display name (optional)" htmlFor="account-name">
          <Input
            id="account-name"
            value={name}
            placeholder="Shown instead of your email"
            onChange={(e) => setName(e.target.value)}
          />
        </Field>
        <Field label="Email" htmlFor="account-email">
          <Input
            id="account-email"
            type="email"
            required
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
        </Field>
        <div className="flex items-center gap-3">
          <Button type="submit" disabled={!dirty || update.isPending}>
            Save profile
          </Button>
          {update.isError ? (
            <span role="alert" className="text-sm text-destructive">
              Couldn't save. That email may already be in use.
            </span>
          ) : null}
          {update.isSuccess && !dirty ? <SaveStatus status="success" /> : null}
        </div>
      </form>
    </Section>
  );
}

function DisplaySection({ me }: { me: PublicUser }) {
  const update = useUpdateMe();
  const [now] = useState(() => Math.floor(Date.now() / 1000));
  const preview = formatDateTime(now, me);

  return (
    <Section title="Date, time and calendar">
      <div className="flex flex-wrap gap-4">
        <Field label="Date format">
          <Select
            value={me.date_format}
            onValueChange={(date_format) => update.mutate({ body: { date_format } })}
          >
            <SelectTrigger aria-label="Date format" className="w-44">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {DATE_FORMATS.map((f) => (
                <SelectItem key={f} value={f}>
                  {f}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
        <Field label="Clock">
          <Select
            value={me.time_format}
            onValueChange={(time_format) => update.mutate({ body: { time_format } })}
          >
            <SelectTrigger aria-label="Clock" className="w-36">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {TIME_FORMATS.map((f) => (
                <SelectItem key={f} value={f}>
                  {TIME_LABEL[f]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
      </div>
      <Field label="Week starts on">
        <Select
          value={me.week_start}
          onValueChange={(week_start) => update.mutate({ body: { week_start } })}
        >
          <SelectTrigger aria-label="Week starts on" className="w-44">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="monday">Monday</SelectItem>
            <SelectItem value="sunday">Sunday</SelectItem>
          </SelectContent>
        </Select>
      </Field>
      <p className="border border-border bg-background px-3 py-2 text-sm">
        <span className="label-mono mr-2 text-muted-foreground">Preview</span>
        {preview}
        <span className="text-muted-foreground">
          {" "}
          · used for anything older than 7 days; newer items say "2 h ago" or "3 days ago"
        </span>
      </p>
    </Section>
  );
}

function PasswordSection() {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const change = useMutation({
    ...changePasswordMutation(),
    meta: { inline: true },
    onSuccess: () => {
      setCurrent("");
      setNext("");
    },
  });

  return (
    <Section title="Password">
      <form
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          change.mutate({ body: { current_password: current, new_password: next } });
        }}
      >
        <Field label="Current password" htmlFor="account-current">
          <Input
            id="account-current"
            type="password"
            required
            autoComplete="current-password"
            value={current}
            onChange={(e) => setCurrent(e.target.value)}
          />
        </Field>
        <Field label="New password" htmlFor="account-new">
          <Input
            id="account-new"
            type="password"
            required
            minLength={8}
            autoComplete="new-password"
            value={next}
            onChange={(e) => setNext(e.target.value)}
          />
          <p className="font-mono text-[11px] text-muted-foreground">
            min 8 characters · your other sessions are signed out
          </p>
        </Field>
        <div className="flex items-center gap-3">
          <Button type="submit" disabled={change.isPending}>
            Change password
          </Button>
          {change.isError ? (
            <span role="alert" className="text-sm text-destructive">
              Couldn't change it. Check your current password.
            </span>
          ) : null}
          {change.isSuccess ? <SaveStatus status="success" /> : null}
        </div>
      </form>
    </Section>
  );
}
