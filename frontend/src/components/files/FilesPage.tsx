import { useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, getRouteApi } from "@tanstack/react-router";
import { toast } from "sonner";
import type { FileView } from "woodmouse-client";
import {
  cleanupFilesMutation,
  deleteFileMutation,
  fileUsageOptions,
  listFileVersionsOptions,
  listFilesOptions,
  unusedFilesOptions,
} from "woodmouse-client/react-query";

import { FileThumb } from "./FileThumb";
import { fileContentUrl, fileVersionContentUrl } from "../../lib/api";
import { formatBytes, uploadFileVersion } from "../../lib/upload";
import { useFormatters } from "../../lib/useFormatters";
import { userLabel } from "../../lib/user";
import { useWorkspaceRole } from "../../lib/useWorkspaceRole";
import { cn } from "../../lib/utils";
import { ConfirmDialog } from "../ConfirmDialog";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { useUploader } from "./useUploader";

const route = getRouteApi("/w/$workspaceId/files");

/** The workspace's file library: everything uploaded, where it is used, and admin cleanup. */
export function FilesPage({ workspaceId }: { workspaceId: string }) {
  const queryClient = useQueryClient();
  const { when } = useFormatters();
  const { canWrite, isAdmin } = useWorkspaceRole(workspaceId);
  // The search and the unused-only filter live in the URL, so a link shows the same files.
  const search = route.useSearch();
  const navigate = route.useNavigate();
  const query = search.q ?? "";
  const onlyUnused = search.unused === true;
  const setSearch = (patch: { q?: string; unused?: boolean }) =>
    void navigate({ search: (prev) => ({ ...prev, ...patch }), replace: true });
  const [expanded, setExpanded] = useState<{ id: string; panel: "usage" | "versions" } | null>(
    null,
  );
  const [deleting, setDeleting] = useState<FileView | null>(null);
  const versionInput = useRef<HTMLInputElement>(null);
  const versionTarget = useRef<FileView | null>(null);
  const [confirmCleanup, setConfirmCleanup] = useState(false);
  const [over, setOver] = useState(false);
  const input = useRef<HTMLInputElement>(null);

  const { data: files } = useQuery(
    listFilesOptions({
      path: { id: workspaceId },
      query: { q: query.trim() || undefined, unused: onlyUnused || undefined },
    }),
  );
  const { data: unused } = useQuery({
    ...unusedFilesOptions({ path: { id: workspaceId } }),
    enabled: isAdmin,
  });
  const { uploadFiles, isUploading, dialog } = useUploader(workspaceId);
  const remove = useMutation({
    ...deleteFileMutation(),
  });
  const addVersion = async (file: FileView, upload: File) => {
    const result = await uploadFileVersion(file.id, upload);
    if (result.status === "created") {
      void queryClient.invalidateQueries();
      toast.success(`Uploaded a new version of ${file.name}.`);
    } else if (result.status === "duplicate") {
      toast.info("That is the same content as the current version.");
    } else if (result.status === "too_large") {
      toast.error(`${upload.name} is larger than the upload limit.`);
    } else {
      toast.error(`Couldn't upload ${upload.name}. Try again.`);
    }
  };
  const cleanup = useMutation({
    ...cleanupFilesMutation(),
    onSuccess: (result) => {
      void queryClient.invalidateQueries();
      toast.success(
        `Deleted ${result.deleted} ${result.deleted === 1 ? "file" : "files"}, freed ${formatBytes(result.freed_bytes)}.`,
      );
    },
  });

  const totalBytes = files?.reduce((sum, f) => sum + f.size, 0) ?? 0;

  return (
    <div
      className={cn("min-h-full", over && "bg-accent/40")}
      onDragOver={(e) => {
        if (!canWrite || !e.dataTransfer.types.includes("Files")) return;
        e.preventDefault();
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        setOver(false);
        if (!canWrite || e.dataTransfer.files.length === 0) return;
        e.preventDefault();
        void uploadFiles([...e.dataTransfer.files]);
      }}
    >
      <header className="flex flex-wrap items-end justify-between gap-3 border-b border-border px-4 pt-5 pb-4 sm:px-6">
        <div>
          <p className="label-mono text-muted-foreground">Workspace</p>
          <h1 className="font-display text-2xl font-bold">Files</h1>
        </div>
        <div className="flex gap-2">
          {isAdmin ? (
            <Button
              type="button"
              variant="outline"
              onClick={() => setConfirmCleanup(true)}
              disabled={!unused || unused.count === 0}
            >
              Clean up unused…
            </Button>
          ) : null}
          {canWrite ? (
            <>
              <input
                ref={input}
                type="file"
                multiple
                className="sr-only"
                aria-label="Upload files"
                onChange={(e) => {
                  void uploadFiles([...(e.target.files ?? [])]);
                  e.target.value = "";
                }}
              />
              <Button type="button" disabled={isUploading} onClick={() => input.current?.click()}>
                {isUploading ? "Uploading…" : "Upload"}
              </Button>
            </>
          ) : null}
        </div>
      </header>

      <div className="flex flex-wrap items-center gap-3 border-b border-border px-4 py-2.5 sm:px-6">
        <Input
          aria-label="Search files"
          placeholder="Search files…"
          className="h-8 max-w-56"
          value={query}
          onChange={(e) => setSearch({ q: e.target.value || undefined })}
        />
        <Button
          type="button"
          variant={onlyUnused ? "secondary" : "outline"}
          size="sm"
          aria-pressed={onlyUnused}
          onClick={() => setSearch({ unused: onlyUnused ? undefined : true })}
        >
          Unused only
        </Button>
        <span className="label-mono ml-auto text-muted-foreground">
          {files?.length ?? 0} {files?.length === 1 ? "file" : "files"} · {formatBytes(totalBytes)}
        </span>
      </div>

      {files && files.length === 0 ? (
        <p className="p-6 text-sm text-muted-foreground">
          {query || onlyUnused
            ? "No files match."
            : canWrite
              ? "No files yet. Upload one, or drop files anywhere on this page."
              : "No files yet."}
        </p>
      ) : null}

      <ul>
        {files?.map((file) => (
          <li key={file.id} className="border-b border-border bg-card">
            <div className="flex flex-wrap items-center gap-3 px-4 py-2.5 sm:px-6">
              <FileThumb file={file} />
              <div className="min-w-0 flex-1 basis-48">
                <p className="truncate text-sm font-medium">{file.name}</p>
                <p className="label-mono text-muted-foreground">
                  {formatBytes(file.size)} · {file.used_in}{" "}
                  {file.used_in === 1 ? "reference" : "references"} · {when(file.created_at)}
                  {file.uploaded_by ? ` · ${userLabel(file.uploaded_by)}` : ""}
                </p>
              </div>
              {file.used_in === 0 ? (
                <span className="label-mono text-signal">Unused</span>
              ) : (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  aria-expanded={expanded?.id === file.id && expanded.panel === "usage"}
                  onClick={() =>
                    setExpanded(
                      expanded?.id === file.id && expanded.panel === "usage"
                        ? null
                        : { id: file.id, panel: "usage" },
                    )
                  }
                >
                  Where used
                </Button>
              )}
              <Button
                type="button"
                variant="ghost"
                size="sm"
                aria-expanded={expanded?.id === file.id && expanded.panel === "versions"}
                onClick={() =>
                  setExpanded(
                    expanded?.id === file.id && expanded.panel === "versions"
                      ? null
                      : { id: file.id, panel: "versions" },
                  )
                }
              >
                Versions · {file.versions}
              </Button>
              {canWrite ? (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    versionTarget.current = file;
                    versionInput.current?.click();
                  }}
                >
                  Upload new version
                </Button>
              ) : null}
              <Button variant="outline" size="sm" asChild>
                <a href={fileContentUrl(file.id)} download={file.name}>
                  Download
                </a>
              </Button>
              {canWrite ? (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="text-destructive"
                  onClick={() => setDeleting(file)}
                >
                  Delete
                </Button>
              ) : null}
            </div>
            {expanded?.id === file.id && expanded.panel === "usage" ? (
              <FileUsage fileId={file.id} workspaceId={workspaceId} />
            ) : null}
            {expanded?.id === file.id && expanded.panel === "versions" ? (
              <FileVersions fileId={file.id} />
            ) : null}
          </li>
        ))}
      </ul>
      {dialog}
      <input
        ref={versionInput}
        type="file"
        className="sr-only"
        aria-label="Upload new version"
        onChange={(e) => {
          const picked = e.target.files?.[0];
          const target = versionTarget.current;
          if (picked && target) void addVersion(target, picked);
          e.target.value = "";
        }}
      />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={`Delete "${deleting?.name ?? ""}"?`}
        description="It leaves the library and everything it is attached to. You can restore it from the trash."
        confirmLabel="Delete file"
        onConfirm={() => deleting && remove.mutate({ path: { id: deleting.id } })}
      />
      <ConfirmDialog
        open={confirmCleanup}
        onOpenChange={setConfirmCleanup}
        title="Clean up unused files?"
        description={`${unused?.count ?? 0} ${unused?.count === 1 ? "file" : "files"} (${formatBytes(unused?.total_bytes ?? 0)}) aren't attached to anything, including trashed tasks. Deleting them permanently can't be undone.`}
        confirmLabel={`Delete ${unused?.count ?? 0} ${unused?.count === 1 ? "file" : "files"}`}
        onConfirm={() => cleanup.mutate({ path: { id: workspaceId } })}
      />
    </div>
  );
}

function FileUsage({ fileId, workspaceId }: { fileId: string; workspaceId: string }) {
  const { data: usage } = useQuery(fileUsageOptions({ path: { id: fileId } }));
  return (
    <ul className="border-t border-border bg-muted/40 px-4 py-2 sm:px-6">
      {usage?.map((u) => (
        <li key={`${u.kind}:${u.id}`} className="flex items-center gap-2 py-0.5 text-sm">
          {u.in_trash ? (
            <>
              <span>{u.title}</span>
              <span className="label-mono text-muted-foreground">in trash</span>
            </>
          ) : (
            <>
              {u.kind === "page" ? (
                <Link
                  to="/w/$workspaceId/wiki/$pageId"
                  params={{ workspaceId, pageId: u.id }}
                  className="hover:underline"
                >
                  {u.title}
                </Link>
              ) : (
                <Link
                  to="/w/$workspaceId/t/$taskId"
                  params={{ workspaceId, taskId: u.id }}
                  className="hover:underline"
                >
                  {u.title}
                </Link>
              )}
              {u.kind === "page" ? (
                <span className="label-mono text-muted-foreground">page</span>
              ) : null}
            </>
          )}
        </li>
      ))}
    </ul>
  );
}

/** Every version of a file, newest first, each downloadable. */
function FileVersions({ fileId }: { fileId: string }) {
  const { when } = useFormatters();
  const { data: versions } = useQuery(listFileVersionsOptions({ path: { id: fileId } }));
  return (
    <ul className="border-t border-border bg-muted/40 px-4 py-2 sm:px-6">
      {versions?.map((v) => (
        <li key={v.id} className="flex flex-wrap items-center gap-x-3 py-0.5 text-sm">
          <span className="font-mono">v{v.number}</span>
          <span className="label-mono text-muted-foreground">
            {formatBytes(v.size)} · {when(v.created_at)}
            {v.uploaded_by ? ` · ${userLabel(v.uploaded_by)}` : ""}
            {v.current ? " · current" : ""}
          </span>
          <a
            className="ml-auto hover:underline"
            href={fileVersionContentUrl(fileId, v.id)}
            download={v.name}
          >
            Download
          </a>
        </li>
      ))}
    </ul>
  );
}
