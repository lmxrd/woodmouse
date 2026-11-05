# Woodmouse 🐭

A self-hosted project manager for a household or a small team. Projects with
boards, lists, a calendar and a timeline, plus a wiki you edit together live.
One program, one SQLite file, nothing else to run.

- **Projects** with a board, a list view, a calendar and a timeline, and
  subtasks, sprints, labels, comments and attachments.
- **A wiki** per workspace, edited live by everyone who has a page open, with
  version history.
- **Files** with versions, and a trash: nothing is permanently deleted.
- **Notifications**, optionally by email, and **calendar feeds** you can
  subscribe to from any calendar app.
- **Import and export** of whole workspaces, search, themes, and a layout that
  works on phones.

## Quick start

You need [Docker](https://docs.docker.com/get-docker/). Start Woodmouse with
one command:

```bash
docker run -d --name woodmouse -p 8000:8000 -v woodmouse-data:/data ghcr.io/lmxrd/woodmouse
```

Then open <http://localhost:8000>.

1. **Create your account.** The first account is the site's super admin.
2. **Create a workspace** for your projects, files and wiki.
3. **Invite people** under **Settings → Members**. Copy the invite link and
   send it to them; it works once and expires after seven days, and they choose
   their own password.

Your data lives in the `woodmouse-data` volume, so you can stop, update and
recreate the container and keep it.

### With Docker Compose

Save this as `compose.yaml` and run `docker compose up -d`:

```yaml
services:
  woodmouse:
    image: ghcr.io/lmxrd/woodmouse
    restart: unless-stopped
    ports:
      - "8000:8000"
    volumes:
      - woodmouse-data:/data
    environment:
      # The address people use to reach it, for links in emails and calendars.
      # PUBLIC_URL: "https://tasks.example.com"

volumes:
  woodmouse-data:
```

### Without Docker

Each release has a single Linux program for `amd64` and `arm64`, which runs on
any Ubuntu or Debian. Download the one for your machine from the
[releases](https://github.com/lmxrd/woodmouse/releases) (a checksum file sits
next to it), make it executable and run it:

```bash
chmod +x woodmouse-linux-amd64
./woodmouse-linux-amd64
```

It listens on port 8000 and keeps its database (`woodmouse.db`) and uploaded
files (`data/`) in the folder you start it from.

## Self-hosting

### Configuration

Everything works without configuration. These environment variables are all
optional:

| Variable                   | Default                 | Meaning                                                                                  |
| -------------------------- | ----------------------- | ---------------------------------------------------------------------------------------- |
| `PORT`                     | `8000`                  | The port to listen on                                                                    |
| `PUBLIC_URL`               | from the request        | Your address, like `https://tasks.example.com`, for links in calendar events and emails  |
| `DATABASE_URL`             | `sqlite:woodmouse.db`   | The SQLite database (the Docker image uses `sqlite:/data/woodmouse.db`)                  |
| `DATA_DIR`                 | `data`                  | Where uploaded files live (the Docker image uses `/data`)                                |
| `MAX_UPLOAD_MB`            | `25`                    | The largest accepted upload                                                              |
| `WOODMOUSE_SECURE_COOKIES` | unset                   | Set to `1` when served over HTTPS: login cookies get `Secure` and the `__Host-` prefix   |
| `WOODMOUSE_SWAGGER_UI`     | unset                   | Set to `1` to browse the API at `/swagger-ui`                                            |
| `SMTP_*`                   | unset                   | Email, see below                                                                         |
| `OIDC_*`                   | unset                   | Single sign-on, see below                                                                |
| `TOTP_REQUIRED`            | unset                   | `1` to require two-factor sign-in: anyone without it sets it up at their next sign-in    |


### Backups

The whole state is two things: the database and the uploaded files. Back up both
together. The simplest way is to stop Woodmouse (it finishes writing the database
on a clean stop) and copy `woodmouse.db` and the `data` folder. In Docker, stop the
container and archive the volume:

```bash
docker stop woodmouse
docker run --rm -v woodmouse-data:/data -v "$PWD":/backup alpine tar czf /backup/woodmouse-backup.tgz -C /data .
docker start woodmouse
```

To copy the database while the server runs, use SQLite rather than `cp`:
`sqlite3 woodmouse.db "VACUUM INTO 'backup.db'"`.

`woodmouse check` reports rows that point at something that no longer exists.

### Behind a reverse proxy

- **Pass WebSockets through.** Live editing uses one (`/api/pages/<id>/live`).
  In nginx add `proxy_set_header Upgrade $http_upgrade;` and
  `proxy_set_header Connection "upgrade";`; Caddy and Traefik do it by default.
  Without it, pages still work with a Save button instead of live editing.
- **Run one instance.** Live documents are kept in memory.
- **Use HTTPS**, and set `WOODMOUSE_SECURE_COOKIES=1` and `PUBLIC_URL`. The
  session cookie is then `__Host-woodmouse_session`, which no other subdomain
  can set. Turning the setting on or off signs everyone out once.

### Email

Email is off until you set `SMTP_HOST`, `SMTP_PORT`, `SMTP_USER`,
`SMTP_PASSWORD`, `SMTP_FROM` and `SMTP_TLS` (`starttls`, `tls` or `none`), or
fill in the same fields on the super admin's **Site admin → Email** page, where
a field overrides the environment and there is a test button. With `PUBLIC_URL`
set, invite links are emailed too. Without email, admins copy the invite link
and share it by hand.

### Single sign-on

Besides passwords, people can sign in through any OpenID Connect provider
(Keycloak, Authentik, Authelia, Google, ...). Set it up on the super admin's
**Site admin → Single sign-on** page, or with these variables. A variable that
is set wins over its field on that page, which then cannot be changed there.

| Variable             | Meaning                                                                                       |
| -------------------- | --------------------------------------------------------------------------------------------- |
| `OIDC_ISSUER`        | The provider's issuer URL (https), like `https://id.example.com/realms/home`                  |
| `OIDC_CLIENT_ID`     | The client registered at the provider                                                         |
| `OIDC_CLIENT_SECRET` | Its secret (leave unset for a public client)                                                  |
| `OIDC_ENABLED`       | `1`/`0` (also `true`/`false`, `yes`/`no`, `on`/`off`); on by default when `OIDC_ISSUER` is set |
| `OIDC_DISPLAY_NAME`  | The provider's name on the login button                                                       |
| `OIDC_AUTO_CREATE`   | `1` to create an account for someone new whose provider confirms their email                  |
| `OIDC_REDIRECT_URL`  | The callback URL, when the one from `PUBLIC_URL` is wrong                                     |
| `OIDC_ONLY`          | `1` to turn password sign-in off: single sign-on is the only way in (see below)               |

A yes/no variable with any other value stops the server from starting.

- **Register the callback URL** at the provider:
  `<PUBLIC_URL>/api/auth/oidc/callback`. The settings page shows it.
- **Over HTTPS, set `WOODMOUSE_SECURE_COOKIES=1`**; over plain HTTP, leave it
  unset. When the callback URL and that setting disagree, single sign-on is
  refused (the login page says it is misconfigured and the server log says why).
- **How accounts are found.** The first time, by the email address, and only if
  the provider says it is verified; from then on by the provider's user id, so
  a changed email does not matter. Microsoft Entra ID never says an email is
  verified, so its users cannot be matched to existing accounts by email.
  **Only use a provider you trust to verify emails.** Any existing account,
  the super admin's included, is linked to whoever first signs in with its
  email. Two consequences:
  - If the provider lets people claim an address it reports as verified (for
    example self-registration with "trust email" in Keycloak), they can take
    over the account with that address.
  - People can change their own email in Woodmouse without confirming it. If
    someone sets theirs to a colleague's address before that colleague's first
    single sign-on, the colleague is linked to their account.
- **Automatic accounts.** With `OIDC_AUTO_CREATE`, anyone the provider lets in
  gets an account. It has no password, no admin rights and no workspaces until
  an admin adds them. With a public provider (Google), that means anyone.
- **Passwords keep working.** The first account is always made with a password.
  Someone who signs in only through single sign-on can set a password under
  **Settings → Password & two-factor**.
- **Single sign-on only.** With `OIDC_ONLY=1`, signing in with a password, the
  two-factor code step, choosing a password from an invite link and changing a
  password are all refused (403), and the login page shows only the provider's
  button. `OIDC_ISSUER` and `OIDC_CLIENT_ID` must then be set as variables too,
  and single sign-on cannot be turned off on the settings page; otherwise the
  server does not start. The first account is still created on the login page
  (with a password that cannot be used while this is set): use the email your
  provider confirms for you, since that is how your provider account is linked
  to it. Invited people open their invite link and sign in with the provider
  using the invited email; that links their account (or, with
  `OIDC_AUTO_CREATE`, anyone the provider lets in gets an account).
- Saving the settings checks the provider first and refuses what it cannot
  reach. They apply at once, without a restart. The client secret is stored in
  the database as typed and is never shown again.

### Two-factor sign-in

Anyone can also require a code from an authenticator app at every sign-in,
password or single sign-on (any TOTP app: Aegis, 1Password, Google Authenticator, ...)
under **Settings → Password & two-factor**: scan the QR code, enter the code it
shows, and keep the ten recovery codes, each of which signs in once in place of
a code. Turning it off or making new recovery codes needs a code.
Someone who lost both their app and their recovery codes asks the super admin,
who can reset it under **Site admin → Users**.

With `TOTP_REQUIRED=1`, every sign-in (password or single sign-on) of someone
without two-factor sets it up before going in, and nobody can turn it off.
Sessions that already exist are left alone. After the super admin resets
someone's two-factor, they set it up again at their next sign-in.

### Docker notes

The image runs as a non-root user (uid 100) and asks its own `/api/health`
endpoint for its health check. If you mount a folder from the host instead of a
named volume, make it writable by that user.

### The API

Every route lives under `/api`. Run with `WOODMOUSE_SWAGGER_UI=1` to browse the
API, or load the `openapi.json` attached to each release into any OpenAPI tool.

Scripts and integrations use an **API key** from **Settings → Account**, sent
as `Authorization: Bearer wm_...`. It acts as you, capped at its permission:
**Read** only reads, **Read & write** also changes things but never
administers (site or workspace), and **Admin** (for site and global admins
only) does what you may. A key can never manage API keys, two-factor sign-in,
your password, email or sessions. It stops working when it is revoked, when it
expires, or when your account is disabled. The key is shown once, when made.

## Contributing

To run it from source or change it, see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[Hippocratic License 3.0](LICENSE).
