/** What to call a user: their display name, or their email when they have not set one. */
export function userLabel(user: { name: string; email: string }): string {
  return user.name.trim() || user.email;
}
