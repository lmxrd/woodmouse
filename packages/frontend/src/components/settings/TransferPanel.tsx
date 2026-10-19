import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { ImportReport, UserChoice } from "woodmouse-client";
import { listMembersOptions, listProjectsOptions } from "woodmouse-client/react-query";

import {
  canImport,
  exportCsvUrl,
  exportJsonUrl,
  importFormat,
  importText,
  type UserMap,
} from "../../lib/transfer";
import { userLabel } from "../../lib/user";
import { useWorkspaceRole } from "../../lib/useWorkspaceRole";
import { Field } from "../common/Field";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { InviteLinkBox } from "./InviteLinkBox";

const PARTS = [
  { id: "projects", label: "Projects, lists, sprints, tasks and comments" },
  { id: "labels", label: "Labels" },
  { id: "wiki", label: "Wiki pages" },
  { id: "members", label: "Member list (for reference; admins only)" },
] as const;

/** Export chosen parts of the workspace or its tasks as CSV; import either back. */
export function TransferPanel({ workspaceId }: { workspaceId: string }) {
  return (
    <div className="mx-auto flex max-w-2xl flex-col gap-6 p-4 sm:p-6">
      <ExportSection workspaceId={workspaceId} />
      <ImportSection workspaceId={workspaceId} />
    </div>
  );
}

function ExportSection({ workspaceId }: { workspaceId: string }) {
  const { isAdmin } = useWorkspaceRole(workspaceId);
  const { data: projects } = useQuery(listProjectsOptions({ path: { workspace_id: workspaceId } }));
  const [parts, setParts] = useState<Set<string>>(new Set(["projects", "labels", "wiki"]));
  // Empty means every project.
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const toggle = (set: Set<string>, value: string) => {
    const next = new Set(set);
    if (next.has(value)) next.delete(value);
    else next.add(value);
    return next;
  };

  return (
    <section className="flex flex-col gap-3 border border-border bg-card p-5">
      <h2 className="label-mono text-muted-foreground">Export</h2>
      <fieldset className="flex flex-col gap-1.5">
        <legend className="mb-1 text-sm">What to include</legend>
        {PARTS.filter((p) => p.id !== "members" || isAdmin).map((p) => (
          <label key={p.id} className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={parts.has(p.id)}
              onChange={() => setParts(toggle(parts, p.id))}
            />
            {p.label}
          </label>
        ))}
      </fieldset>
      {projects && projects.length > 1 ? (
        <fieldset className="flex flex-col gap-1.5">
          <legend className="mb-1 text-sm">
            Projects <span className="text-muted-foreground">(none ticked means all)</span>
          </legend>
          {projects.map((p) => (
            <label key={p.id} className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={chosen.has(p.id)}
                onChange={() => setChosen(toggle(chosen, p.id))}
              />
              {p.name}
            </label>
          ))}
        </fieldset>
      ) : null}
      <div className="flex flex-wrap gap-2">
        <Button
          disabled={parts.size === 0}
          onClick={() =>
            window.location.assign(exportJsonUrl(workspaceId, [...parts], [...chosen]))
          }
        >
          Download JSON
        </Button>
        <Button
          variant="outline"
          onClick={() => window.location.assign(exportCsvUrl(workspaceId, [...chosen]))}
        >
          Download tasks as CSV
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        The CSV has one row per task with its project, list, sprint, parent, dates, estimates,
        labels and assignee emails, so importing it sets everything up again.
      </p>
    </section>
  );
}

function ImportSection({ workspaceId }: { workspaceId: string }) {
  const queryClient = useQueryClient();
  const { canWrite, isAdmin } = useWorkspaceRole(workspaceId);
  const { data: members } = useQuery(listMembersOptions({ path: { id: workspaceId } }));
  const [file, setFile] = useState<{ name: string; text: string; format: "csv" | "json" } | null>(
    null,
  );
  const [project, setProject] = useState("");
  const [userMap, setUserMap] = useState<UserMap>({});
  // Everyone the file names who is not a member, kept after they get a choice so it can be changed.
  const [people, setPeople] = useState<{ email: string; references: number }[]>([]);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async (
    dryRun: boolean,
    next: { file?: typeof file; project?: string; userMap?: UserMap } = {},
  ) => {
    const f = next.file ?? file;
    if (!f) return;
    setBusy(true);
    setError(null);
    const result = await importText(workspaceId, f.format, f.text, {
      dryRun,
      project: (next.project ?? project).trim() || undefined,
      userMap: next.userMap ?? userMap,
    });
    setBusy(false);
    if (!result.ok) {
      setReport(null);
      return setError(result.message);
    }
    setReport(result.report);
    if (result.report.unmapped_users.length > 0) {
      setPeople((old) => [
        ...old,
        ...result.report.unmapped_users.filter((p) => !old.some((o) => o.email === p.email)),
      ]);
    }
    if (!dryRun && result.report.applied) void queryClient.invalidateQueries();
  };
  const imported = report?.applied === true;

  return (
    <section className="flex flex-col gap-3 border border-border bg-card p-5">
      <h2 className="label-mono text-muted-foreground">Import</h2>
      {!canWrite ? (
        <p className="text-sm text-muted-foreground">Importing needs write access.</p>
      ) : (
        <>
          <p className="text-sm text-muted-foreground">
            A Woodmouse JSON export or a tasks CSV. It is imported completely or not at all: the
            file is checked first, and anything that cannot be imported as it is must be fixed in
            the file. Projects, lists, sprints and labels that do not exist are created; existing
            ones are matched by name.
          </p>
          <Field label="File (.json or .csv)" htmlFor="import-file">
            <Input
              id="import-file"
              type="file"
              accept=".json,.csv,application/json,text/csv"
              onChange={async (e) => {
                const picked = e.target.files?.[0];
                setReport(null);
                setError(null);
                setUserMap({});
                setPeople([]);
                if (!picked) return setFile(null);
                const format = importFormat(picked.name);
                if (!format) {
                  setFile(null);
                  return setError("Choose a .json or .csv file.");
                }
                const next = { name: picked.name, text: await picked.text(), format };
                setFile(next);
                void run(true, { file: next, userMap: {} });
              }}
            />
          </Field>
          {file?.format === "csv" ? (
            <Field label="Project for rows without one (optional)" htmlFor="import-project">
              <Input
                id="import-project"
                value={project}
                onChange={(e) => setProject(e.target.value)}
                onBlur={() => void run(true)}
                placeholder="Project name"
              />
            </Field>
          ) : null}
          {report && !imported && people.length > 0 ? (
            <UnmappedUsers
              people={people}
              members={(members ?? []).map((m) => ({ id: m.user.id, name: userLabel(m.user) }))}
              canCreate={isAdmin}
              userMap={userMap}
              onChange={(next) => {
                setUserMap(next);
                void run(true, { userMap: next });
              }}
            />
          ) : null}
          <div className="flex gap-2">
            <Button
              disabled={!file || !canImport(report) || busy || imported}
              onClick={() => void run(false)}
            >
              Import
            </Button>
            {file && !imported ? (
              <Button variant="outline" disabled={busy} onClick={() => void run(true)}>
                Check again
              </Button>
            ) : null}
          </div>
          {error ? (
            <p role="alert" className="text-sm text-destructive">
              {error}
            </p>
          ) : null}
          {report ? <ReportView report={report} /> : null}
          {imported && report && report.invites.length > 0 ? (
            <div className="flex flex-col gap-2">
              <p className="text-sm">
                These people were created. Share their one-time invite links so they can choose a
                password:
              </p>
              {report.invites.map((i) => (
                <InviteLinkBox
                  key={i.email}
                  name={i.email}
                  token={i.invite_token}
                  onDismiss={() =>
                    setReport({ ...report, invites: report.invites.filter((x) => x !== i) })
                  }
                />
              ))}
            </div>
          ) : null}
        </>
      )}
    </section>
  );
}

/** Everyone the file names who is not a member: map them to a member, create them, or leave them out. */
function UnmappedUsers({
  people,
  members,
  canCreate,
  userMap,
  onChange,
}: {
  people: { email: string; references: number }[];
  members: { id: string; name: string }[];
  canCreate: boolean;
  userMap: UserMap;
  onChange: (next: UserMap) => void;
}) {
  const set = (email: string, choice: UserChoice | undefined) => {
    const next = { ...userMap };
    if (choice) next[email] = choice;
    else delete next[email];
    onChange(next);
  };
  const action = (email: string) => userMap[email]?.action ?? "none";
  return (
    <div className="flex flex-col gap-2 border border-border p-3">
      <p className="text-sm font-medium">
        {people.length === 1 ? "1 person is" : `${people.length} people are`} not in this workspace
      </p>
      <p className="text-sm text-muted-foreground">
        Choose what to do about each: use someone who is a member, create them
        {canCreate ? "" : " (needs a workspace admin)"}, or leave them out (their tasks stay
        unassigned and you become the author of their comments).
      </p>
      {people.map((p) => {
        const choice = userMap[p.email];
        return (
          <div key={p.email} className="flex flex-wrap items-center gap-2">
            <span className="min-w-0 flex-1 basis-48 truncate text-sm">
              {p.email}{" "}
              <span className="text-muted-foreground">
                · {p.references} {p.references === 1 ? "mention" : "mentions"}
              </span>
            </span>
            <Select
              value={action(p.email)}
              onValueChange={(a) => {
                if (a === "skip") set(p.email, { action: "skip" });
                else if (a === "create") set(p.email, { action: "create", role: "write" });
                else if (a === "map" && members[0]) {
                  set(p.email, { action: "map", user_id: members[0].id });
                }
              }}
            >
              <SelectTrigger size="sm" aria-label={`What to do about ${p.email}`} className="w-44">
                <SelectValue placeholder="Choose…" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="map">Use a member</SelectItem>
                {canCreate ? <SelectItem value="create">Create the person</SelectItem> : null}
                <SelectItem value="skip">Leave out</SelectItem>
              </SelectContent>
            </Select>
            {choice?.action === "map" ? (
              <Select
                value={choice.user_id}
                onValueChange={(id) => set(p.email, { action: "map", user_id: id })}
              >
                <SelectTrigger size="sm" aria-label={`Member for ${p.email}`} className="w-44">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {members.map((m) => (
                    <SelectItem key={m.id} value={m.id}>
                      {m.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : null}
            {choice?.action === "create" ? (
              <Select
                value={choice.role}
                onValueChange={(role) =>
                  set(p.email, { action: "create", role: role as "read" | "write" | "admin" })
                }
              >
                <SelectTrigger size="sm" aria-label={`Role for ${p.email}`} className="w-28">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="read">Read</SelectItem>
                  <SelectItem value="write">Write</SelectItem>
                  <SelectItem value="admin">Admin</SelectItem>
                </SelectContent>
              </Select>
            ) : null}
          </div>
        );
      })}
    </div>
  );
}

function ReportView({ report }: { report: ImportReport }) {
  const counts: [string, number][] = [
    ["projects", report.projects_created],
    ["lists", report.lists_created],
    ["sprints", report.sprints_created],
    ["labels", report.labels_created],
    ["tasks", report.tasks_created],
    ["comments", report.comments_created],
    ["pages", report.pages_created],
  ];
  const summary =
    counts
      .filter(([, n]) => n > 0)
      .map(([name, n]) => `${n} ${name}`)
      .join(", ") || "nothing";
  const blocked = report.problems.length > 0 || report.unmapped_users.length > 0;
  return (
    <div role="status" className="flex flex-col gap-2 border border-primary bg-accent p-3 text-sm">
      <p className="font-medium">
        {report.applied
          ? `Imported ${summary}.`
          : blocked
            ? "Not ready to import yet."
            : `Ready: this would create ${summary}.`}
      </p>
      {report.problems.length > 0 ? (
        <div>
          <p className="label-mono text-destructive">
            Fix these in the file ({report.problems.length})
          </p>
          <ul className="list-disc pl-5">
            {report.problems.slice(0, 30).map((s, i) => (
              <li key={i}>{s}</li>
            ))}
          </ul>
        </div>
      ) : null}
      {report.warnings.length > 0 ? (
        <div>
          <p className="label-mono text-muted-foreground">
            Worth knowing ({report.warnings.length})
          </p>
          <ul className="list-disc pl-5">
            {report.warnings.slice(0, 20).map((s, i) => (
              <li key={i}>{s}</li>
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  );
}
