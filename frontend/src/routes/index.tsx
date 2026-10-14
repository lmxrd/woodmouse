import { useState } from "react";
import { createFileRoute, redirect, useNavigate } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { meOptions } from "woodmouse-client/react-query";
import {
  createWorkspaceMutation,
  listWorkspacesOptions,
  logoutMutation,
} from "woodmouse-client/react-query";

export const Route = createFileRoute("/")({
  beforeLoad: async ({ context }) => {
    // Guard: unauthenticated visitors land on the login page.
    await context.queryClient.ensureQueryData(meOptions()).catch(() => {
      throw redirect({ to: "/login" });
    });
  },
  component: HomePage,
});

function HomePage() {
  const { data: me } = useQuery(meOptions());
  const { data: workspaces, isPending } = useQuery(listWorkspacesOptions());
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const createWorkspace = useMutation({
    ...createWorkspaceMutation(),
    onSuccess: () => queryClient.invalidateQueries(),
  });

  const logout = useMutation({
    ...logoutMutation(),
    onSuccess: () => {
      queryClient.clear();
      void navigate({ to: "/login" });
    },
  });

  const [name, setName] = useState("");

  return (
    <section className="space-y-8">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Welcome, {me?.name}</h1>
          <p className="text-sm text-gray-600">{me?.email}</p>
        </div>
        <button
          type="button"
          onClick={() => logout.mutate({})}
          disabled={logout.isPending}
          className="rounded-md border border-gray-300 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-100 disabled:opacity-50"
        >
          Log out
        </button>
      </div>

      <div className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
        <h2 className="text-lg font-semibold">Workspaces</h2>
        {isPending ? <p className="mt-2 text-gray-500">Loading...</p> : null}
        {workspaces && workspaces.length === 0 ? (
          <p className="mt-2 text-sm text-gray-500">
            No workspaces yet. Create your first one below.
          </p>
        ) : null}
        <ul className="mt-3 space-y-2">
          {workspaces?.map((workspace) => (
            <li key={workspace.id} className="rounded-md border border-gray-200 px-3 py-2 text-sm">
              {workspace.name}
            </li>
          ))}
        </ul>

        <form
          className="mt-4 flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (!name.trim()) return;
            createWorkspace.mutate({ body: { name } });
            setName("");
          }}
        >
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="New workspace name"
            className="flex-1 rounded-md border border-gray-300 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none"
          />
          <button
            type="submit"
            disabled={createWorkspace.isPending || !name.trim()}
            className="rounded-md bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
          >
            Create
          </button>
        </form>
        {createWorkspace.isError ? (
          <p className="mt-2 text-sm text-red-600">Could not create the workspace.</p>
        ) : null}
      </div>
    </section>
  );
}
