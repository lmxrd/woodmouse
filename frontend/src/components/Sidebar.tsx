import { Link, useNavigate } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  createProjectMutation,
  listProjectsOptions,
  listWorkspacesOptions,
  logoutMutation,
  meOptions,
} from "woodmouse-client/react-query";

import { cn } from "../lib/utils";
import { InlineAdd } from "./InlineAdd";
import { UserAvatar } from "./UserAvatar";
import { Button } from "./ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";

export function Sidebar({
  workspaceId,
  className,
  onNavigate,
}: {
  workspaceId: string;
  className?: string;
  /** Called after the user picks a project or workspace (lets the mobile drawer close). */
  onNavigate?: () => void;
}) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const { data: me } = useQuery(meOptions());
  const { data: workspaces } = useQuery(listWorkspacesOptions());
  const { data: projects } = useQuery(listProjectsOptions({ path: { workspace_id: workspaceId } }));
  const workspace = workspaces?.find((w) => w.id === workspaceId);

  const createProject = useMutation({
    ...createProjectMutation(),
    onSuccess: async (project) => {
      await queryClient.invalidateQueries();
      void navigate({
        to: "/w/$workspaceId/p/$projectId",
        params: { workspaceId, projectId: project.id },
      });
    },
  });
  const logout = useMutation({
    ...logoutMutation(),
    onSuccess: () => {
      queryClient.clear();
      void navigate({ to: "/login" });
    },
  });

  return (
    <aside
      className={cn("flex min-w-0 flex-col gap-5 border-r border-border bg-card p-4", className)}
    >
      <DropdownMenu>
        <DropdownMenuTrigger className="flex items-center gap-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring">
          <span className="grid size-8 place-items-center bg-primary font-display font-bold text-primary-foreground">
            W
          </span>
          <span className="flex min-w-0 flex-col leading-tight">
            <span className="truncate font-display text-[15px] font-bold">
              {workspace?.name ?? "Workspace"}
            </span>
            <span className="label-mono text-[10px] text-muted-foreground">Switch ▾</span>
          </span>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start">
          <DropdownMenuLabel className="label-mono text-muted-foreground">
            Workspaces
          </DropdownMenuLabel>
          {workspaces?.map((w) => (
            <DropdownMenuItem
              key={w.id}
              onSelect={() =>
                void navigate({ to: "/w/$workspaceId", params: { workspaceId: w.id } })
              }
            >
              {w.name}
            </DropdownMenuItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>

      <Link
        to="/w/$workspaceId/my-tasks"
        params={{ workspaceId }}
        onClick={onNavigate}
        className="px-2 py-1 text-sm text-muted-foreground hover:text-foreground"
        activeProps={{ className: "bg-accent font-medium !text-foreground" }}
      >
        My tasks
      </Link>

      <nav className="flex flex-col text-sm" aria-label="Projects">
        <p className="label-mono mb-1.5 text-muted-foreground">Projects</p>
        {projects?.map((project) => (
          <Link
            key={project.id}
            to="/w/$workspaceId/p/$projectId"
            params={{ workspaceId, projectId: project.id }}
            onClick={onNavigate}
            className="truncate px-2 py-1 text-muted-foreground hover:text-foreground"
            activeProps={{ className: "bg-accent font-medium !text-foreground" }}
          >
            {project.name}
          </Link>
        ))}
        <InlineAdd
          className="mt-2"
          label="New project"
          placeholder="Project name"
          onSubmit={(name) =>
            createProject.mutate({ path: { workspace_id: workspaceId }, body: { name } })
          }
        />
      </nav>

      <Link
        to="/w/$workspaceId/settings"
        params={{ workspaceId }}
        onClick={onNavigate}
        className="px-2 py-1 text-sm text-muted-foreground hover:text-foreground"
        activeProps={{ className: "bg-accent font-medium !text-foreground" }}
      >
        Settings
      </Link>

      <div className="mt-auto flex items-center gap-2 text-sm text-muted-foreground">
        <UserAvatar name={me?.name ?? ""} size="md" />
        <span className="min-w-0 flex-1 truncate">{me?.name}</span>
        <Button variant="ghost" size="xs" onClick={() => logout.mutate({})}>
          Log out
        </Button>
      </div>
    </aside>
  );
}
