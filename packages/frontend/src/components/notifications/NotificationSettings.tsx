import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  getNotificationSettingsOptions,
  getNotificationSettingsQueryKey,
  meOptions,
  updateNotificationSettingsMutation,
} from "woodmouse-sdk/react-query";

import { Field } from "../common/Field";
import { Section } from "../settings/AccountPage";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";

const HOURS = Array.from({ length: 24 }, (_, h) => h);
const hourLabel = (h: number) => `${String(h).padStart(2, "0")}:00`;

/** How the person is told: notifications in the app always; optionally email, instantly or as a daily summary. */
export function NotificationSettings() {
  const queryClient = useQueryClient();
  const { data: me } = useQuery(meOptions());
  const { data: settings } = useQuery(getNotificationSettingsOptions());
  const update = useMutation({
    ...updateNotificationSettingsMutation(),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: getNotificationSettingsQueryKey() }),
  });
  if (!settings) return null;

  return (
    <Section title="Notifications">
      <p className="text-sm text-muted-foreground">
        You are told in the app when you are assigned a task, when someone comments on a task you
        are on or have commented on, when a page mentions a task you are on, when something you
        watch changes, and when your tasks are due tomorrow or overdue.
      </p>
      {!settings.email_available ? (
        <p className="border border-dashed border-border px-3 py-2 text-sm text-muted-foreground">
          Email is not set up on this server, so notifications only appear in the app.
        </p>
      ) : null}
      <label className="flex items-start gap-3 text-sm">
        <input
          type="checkbox"
          className="mt-1"
          checked={settings.email_instant}
          disabled={!settings.email_available}
          onChange={(e) => update.mutate({ body: { email_instant: e.target.checked } })}
        />
        <span>
          Email me each notification as it happens
          <span className="block text-muted-foreground">To {me?.email}.</span>
        </span>
      </label>
      <label className="flex items-start gap-3 text-sm">
        <input
          type="checkbox"
          className="mt-1"
          checked={settings.digest}
          disabled={!settings.email_available}
          onChange={(e) => update.mutate({ body: { digest: e.target.checked } })}
        />
        <span>
          Email me a daily summary
          <span className="block text-muted-foreground">
            What you have not read, and what is due today or overdue.
          </span>
        </span>
      </label>
      {settings.digest ? (
        <Field label={`Send it at (${me?.timezone ?? "UTC"})`}>
          <Select
            value={String(settings.digest_hour)}
            onValueChange={(h) => update.mutate({ body: { digest_hour: Number(h) } })}
          >
            <SelectTrigger aria-label="Summary hour" className="w-32">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {HOURS.map((h) => (
                <SelectItem key={h} value={String(h)}>
                  {hourLabel(h)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
      ) : null}
    </Section>
  );
}
