import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { QRCodeSVG } from "qrcode.react";
import type { TotpSetup } from "woodmouse-sdk";
import {
  authStatusOptions,
  meOptions,
  meQueryKey,
  totpDisableMutation,
  totpEnableMutation,
  totpRecoveryCodesMutation,
  totpReplaceConfirmMutation,
  totpReplaceMutation,
  totpSetupMutation,
} from "woodmouse-sdk/react-query";

import { Field } from "../common/field";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Section } from "./account-page";

/**
 * Two-factor sign-in: a code from an authenticator app at every sign-in, password or single sign-on. Off:
 * set it up (scan, confirm a code, keep the recovery codes). On: new recovery codes, or turn it off (not
 * where the site requires it); both need a code.
 */
export function TwoFactorSection() {
  const { data: me } = useQuery(meOptions());
  const { data: status } = useQuery(authStatusOptions());
  // Recovery codes just made: shown once, until dismissed.
  const [codes, setCodes] = useState<string[] | null>(null);
  if (!me) return null;

  return (
    <Section title="Two-factor sign-in">
      {codes ? (
        <RecoveryCodes codes={codes} onDone={() => setCodes(null)} />
      ) : me.totp_enabled ? (
        <Enabled required={status?.totp_required ?? false} onNewCodes={setCodes} />
      ) : (
        <Setup onEnabled={setCodes} />
      )}
    </Section>
  );
}

function useRefreshMe() {
  const queryClient = useQueryClient();
  return () => queryClient.invalidateQueries({ queryKey: meQueryKey() });
}

function Setup({ onEnabled }: { onEnabled: (codes: string[]) => void }) {
  const refresh = useRefreshMe();
  const [setup, setSetup] = useState<TotpSetup | null>(null);
  const [code, setCode] = useState("");
  const start = useMutation({
    ...totpSetupMutation(),
    meta: { inline: true, invalidate: false },
    onSuccess: setSetup,
  });
  const enable = useMutation({
    ...totpEnableMutation(),
    meta: { inline: true },
    onSuccess: (data) => {
      onEnabled(data.codes);
      void refresh();
    },
  });

  if (!setup) {
    return (
      <div className="flex flex-col gap-3">
        <p className="text-sm text-muted-foreground">
          Off. Turn it on to also ask for a code from an authenticator app when you sign in.
        </p>
        <div className="flex items-center gap-3">
          <Button onClick={() => start.mutate({})} disabled={start.isPending}>
            Set up two-factor sign-in
          </Button>
          {start.isError ? (
            <span role="alert" className="text-sm text-destructive">
              Couldn't start the setup.
            </span>
          ) : null}
        </div>
      </div>
    );
  }

  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        enable.mutate({ body: { code: code.trim() } });
      }}
    >
      <p className="text-sm text-muted-foreground">
        Scan this with your authenticator app, or type the key into it. Then enter the code it
        shows.
      </p>
      <SetupQr setup={setup} />
      <Field label="Code from the app" htmlFor="totp-confirm">
        <Input
          id="totp-confirm"
          required
          inputMode="numeric"
          autoComplete="one-time-code"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          className="w-40 font-mono"
        />
      </Field>
      <div className="flex items-center gap-3">
        <Button type="submit" disabled={enable.isPending}>
          Turn on
        </Button>
        <Button type="button" variant="ghost" onClick={() => setSetup(null)}>
          Cancel
        </Button>
        {enable.isError ? (
          <span role="alert" className="text-sm text-destructive">
            That code did not work. Check the app's clock and try the next code.
          </span>
        ) : null}
      </div>
    </form>
  );
}

/** The QR code an authenticator app scans, and the key to type in instead. */
export function SetupQr({ setup }: { setup: TotpSetup }) {
  return (
    <div className="flex flex-wrap items-start gap-4">
      {/* White behind the code whatever the theme: scanners want dark on light. */}
      <div className="border border-border bg-white p-3">
        <QRCodeSVG value={setup.otpauth_uri} size={160} bgColor="#ffffff" fgColor="#000000" />
      </div>
      <Field label="Key" htmlFor="totp-secret">
        <Input
          id="totp-secret"
          readOnly
          value={setup.secret}
          onFocus={(e) => e.target.select()}
          className="w-80 max-w-full font-mono"
        />
      </Field>
    </div>
  );
}

function Enabled({
  required,
  onNewCodes,
}: {
  required: boolean;
  onNewCodes: (codes: string[]) => void;
}) {
  const refresh = useRefreshMe();
  const [code, setCode] = useState("");
  const disable = useMutation({
    ...totpDisableMutation(),
    meta: { inline: true },
    onSuccess: () => {
      setCode("");
      void refresh();
    },
  });
  const renew = useMutation({
    ...totpRecoveryCodesMutation(),
    meta: { inline: true, invalidate: false },
    onSuccess: (data) => {
      setCode("");
      onNewCodes(data.codes);
    },
  });
  // A new secret while replacing the app: the old one works until the new one is confirmed.
  const [replacing, setReplacing] = useState<TotpSetup | null>(null);
  const replace = useMutation({
    ...totpReplaceMutation(),
    meta: { inline: true, invalidate: false },
    onSuccess: (data) => {
      setCode("");
      setReplacing(data);
    },
  });
  const failed = disable.isError || renew.isError || replace.isError;

  if (replacing) return <ReplaceConfirm setup={replacing} onDone={() => setReplacing(null)} />;

  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm text-muted-foreground">
        On. Signing in also asks for a code from your authenticator app.
      </p>
      <Field label="Code from the app, or a recovery code" htmlFor="totp-code">
        <Input
          id="totp-code"
          autoComplete="one-time-code"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          className="w-64 font-mono"
        />
      </Field>
      <div className="flex flex-wrap items-center gap-3">
        <Button
          variant="outline"
          disabled={!code.trim() || renew.isPending}
          onClick={() => renew.mutate({ body: { code: code.trim() } })}
        >
          New recovery codes
        </Button>
        <Button
          variant="outline"
          disabled={!code.trim() || replace.isPending}
          onClick={() => replace.mutate({ body: { code: code.trim() } })}
        >
          Replace authenticator
        </Button>
        {/* Where it is required (`TOTP_REQUIRED`) it cannot be turned off. */}
        {required ? null : (
          <Button
            variant="destructive"
            disabled={!code.trim() || disable.isPending}
            onClick={() => disable.mutate({ body: { code: code.trim() } })}
          >
            Turn off
          </Button>
        )}
        {failed ? (
          <span role="alert" className="text-sm text-destructive">
            That code did not work.
          </span>
        ) : null}
      </div>
    </div>
  );
}

/** Replacing the app: scan the new secret, then confirm with a code from the new app. */
function ReplaceConfirm({ setup, onDone }: { setup: TotpSetup; onDone: () => void }) {
  const [code, setCode] = useState("");
  const confirm = useMutation({
    ...totpReplaceConfirmMutation(),
    meta: { inline: true, invalidate: false },
    onSuccess: onDone,
  });

  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        confirm.mutate({ body: { code: code.trim() } });
      }}
    >
      <p className="text-sm text-muted-foreground">
        Scan this with the new authenticator app, or type the key into it. Your current app keeps
        working until you enter a code from the new one.
      </p>
      <SetupQr setup={setup} />
      <Field label="Code from the new app" htmlFor="totp-replace-code">
        <Input
          id="totp-replace-code"
          required
          inputMode="numeric"
          autoComplete="one-time-code"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          className="w-40 font-mono"
        />
      </Field>
      <div className="flex items-center gap-3">
        <Button type="submit" disabled={confirm.isPending}>
          Use the new app
        </Button>
        <Button type="button" variant="ghost" onClick={onDone}>
          Cancel
        </Button>
        {confirm.isError ? (
          <span role="alert" className="text-sm text-destructive">
            That code did not work. Check the new app's clock and try the next code.
          </span>
        ) : null}
      </div>
    </form>
  );
}

export function RecoveryCodes({
  codes,
  onDone,
  doneLabel = "I have saved them",
}: {
  codes: string[];
  onDone: () => void;
  doneLabel?: string;
}) {
  const [copied, setCopied] = useState(false);
  return (
    <div role="status" className="flex flex-col gap-3 border border-primary bg-accent p-3">
      <p className="text-sm">
        Keep these recovery codes somewhere safe. Each one signs you in once if you lose your
        authenticator app. They are not shown again.
      </p>
      <ul className="grid grid-cols-2 gap-1 font-mono text-sm">
        {codes.map((c) => (
          <li key={c}>{c}</li>
        ))}
      </ul>
      <div className="flex gap-2">
        <Button
          type="button"
          onClick={() => {
            void navigator.clipboard.writeText(codes.join("\n")).then(() => setCopied(true));
          }}
        >
          {copied ? "Copied" : "Copy"}
        </Button>
        <Button type="button" variant="ghost" onClick={onDone}>
          {doneLabel}
        </Button>
      </div>
    </div>
  );
}
