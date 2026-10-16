import { useQuery } from "@tanstack/react-query";
import { meOptions } from "woodmouse-client/react-query";

import type { WeekStart } from "./calendar";
import { dayEpoch, instant } from "./time";
import {
  DEFAULT_PREFS,
  formatDate,
  formatDateShort,
  formatDateTime,
  formatWhen,
  type DisplayPrefs,
} from "./format";

/** The date and time formatters bound to the signed-in user's preferences. */
export function useFormatters() {
  const { data: me } = useQuery(meOptions());
  const prefs: DisplayPrefs = me
    ? { date_format: me.date_format, time_format: me.time_format }
    : DEFAULT_PREFS;
  const weekStart: WeekStart = me?.week_start === "sunday" ? "sunday" : "monday";
  return {
    prefs,
    weekStart,
    /** A day as the calendar holds it: the epoch second of its local midnight. */
    localDay: (epochDay: number) => formatDate(epochDay, prefs),
    /** A day from the API (a task's start or due date). */
    date: (day: string) => formatDate(dayEpoch(day), prefs),
    dateShort: (day: string) => formatDateShort(dayEpoch(day), prefs),
    /** The local date of an instant from the API. */
    dateOf: (at: string) => formatDate(instant(at), prefs),
    /** An instant from the API, in the viewer's local time. */
    dateTime: (at: string) => formatDateTime(instant(at), prefs),
    when: (at: string) => formatWhen(instant(at), prefs),
  };
}
