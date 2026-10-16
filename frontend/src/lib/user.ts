/** What to call a user: their display name or else their email, marked when the account is disabled. */
export function userLabel(user: { name: string; email: string; active?: boolean }): string {
  const label = user.name.trim() || user.email;
  return user.active === false ? `${label} (disabled)` : label;
}
