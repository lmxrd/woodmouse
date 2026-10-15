import { useState } from "react";
import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { acceptInviteMutation, getInviteOptions } from "woodmouse-client/react-query";

import { AuthField, AuthSheet, ErrorNote } from "../components/AuthSheet";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { finishLogin } from "../lib/auth";

/** Public: where an invite link lands. The invitee sets a password and is logged in. */
export const Route = createFileRoute("/invite/$token")({
  component: InvitePage,
});

function InvitePage() {
  const { token } = Route.useParams();
  const {
    data: invite,
    isPending,
    isError,
  } = useQuery({
    ...getInviteOptions({ path: { token } }),
    retry: false, // a dead link will not come back
  });

  if (isError) {
    return (
      <AuthSheet
        tag="Invite"
        title="This link doesn't work"
        description="It may have been used already or expired. Ask the person who invited you for a new one."
      >
        <Button asChild variant="outline">
          <Link to="/login">Go to log in</Link>
        </Button>
      </AuthSheet>
    );
  }
  if (isPending) {
    return (
      <AuthSheet tag="Invite" title="Welcome">
        <p className="text-sm text-muted-foreground">Loading...</p>
      </AuthSheet>
    );
  }

  return (
    <AuthSheet
      tag="Invite"
      title={`Welcome, ${invite.name}`}
      description={`Choose a password for ${invite.email} to join Woodmouse.`}
    >
      <SetPasswordForm token={token} />
    </AuthSheet>
  );
}

function SetPasswordForm({ token }: { token: string }) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const accept = useMutation({
    ...acceptInviteMutation(),
    onSuccess: (user) => finishLogin(queryClient, navigate, user),
  });
  const [password, setPassword] = useState("");

  return (
    <form
      className="flex flex-col gap-5"
      onSubmit={(e) => {
        e.preventDefault();
        accept.mutate({ path: { token }, body: { password } });
      }}
    >
      {accept.isError ? (
        <ErrorNote message="Couldn't set the password. Use at least 8 characters and try again." />
      ) : null}
      <AuthField id="password" label="Password" hint="min 8 characters">
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
      </AuthField>
      <Button type="submit" size="lg" disabled={accept.isPending}>
        {accept.isPending ? "Saving..." : "Set password and continue"}
      </Button>
    </form>
  );
}
