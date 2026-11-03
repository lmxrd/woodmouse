/**
 * Single sign-on. The browser itself goes to `SSO_LOGIN_URL` (a plain link, not an SDK call: it has to
 * leave for the provider), and comes back to `/login?error=<code>` when it did not work.
 */
export const SSO_LOGIN_URL = "/api/auth/oidc/login";

/**
 * What each `error` code of a failed single sign-on means. The codes are the backend's `SsoError`
 * (`backend/api/src/routes/oidc.rs`); keep the two lists in step.
 */
const SSO_ERRORS: Record<string, string> = {
  unavailable:
    "Single sign-on is not available right now. Try again in a minute, or use your password.",
  misconfigured:
    "Single sign-on is not set up correctly. Ask the site admin to check the server log.",
  state: "The sign-in took too long or was started in another browser. Try again.",
  denied: "The sign-in was cancelled or refused by the provider.",
  failed: "The provider's answer could not be checked. Try again.",
  email: "No account is linked, and the provider did not confirm an email address to match one by.",
  unknown: "There is no account for that email. Ask an admin to invite you.",
  linked: "The account with that email is already linked to another sign-in at this provider.",
  disabled: "That account is disabled.",
  setup: "Woodmouse is not set up yet. Create the first account with a password.",
};

/** The message for a `?error=` code; `null` for none. An unknown code gets a general message. */
export function ssoErrorMessage(code: string | undefined): string | null {
  if (!code) return null;
  // Own keys only: `__proto__` or `constructor` in the URL must not reach inherited ones.
  return Object.hasOwn(SSO_ERRORS, code)
    ? SSO_ERRORS[code]
    : "Single sign-on did not work. Try again.";
}
