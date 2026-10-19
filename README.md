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
| `WOODMOUSE_SECURE_COOKIES` | unset                   | Set to `1` when served over HTTPS, so login cookies are marked `Secure`                  |
| `WOODMOUSE_SWAGGER_UI`     | unset                   | Set to `1` to browse the API at `/swagger-ui`                                            |
| `SMTP_*`                   | unset                   | Email, see below                                                                         |


### Behind a reverse proxy

- **Pass WebSockets through.** Live editing uses one (`/api/pages/<id>/live`).
  In nginx add `proxy_set_header Upgrade $http_upgrade;` and
  `proxy_set_header Connection "upgrade";`; Caddy and Traefik do it by default.
  Without it, pages still work with a Save button instead of live editing.
- **Run one instance.** Live documents are kept in memory.
- **Use HTTPS**, and set `WOODMOUSE_SECURE_COOKIES=1` and `PUBLIC_URL`.

### Email

Email is off until you set `SMTP_HOST`, `SMTP_PORT`, `SMTP_USER`,
`SMTP_PASSWORD`, `SMTP_FROM` and `SMTP_TLS` (`starttls`, `tls` or `none`), or
fill in the same fields on the super admin's **Site admin → Email** page, where
a field overrides the environment and there is a test button. With `PUBLIC_URL`
set, invite links are emailed too. Without email, admins copy the invite link
and share it by hand.

### Docker notes

The image runs as a non-root user (uid 100) and asks its own `/api/health`
endpoint for its health check. If you mount a folder from the host instead of a
named volume, make it writable by that user.

### The API

Every route lives under `/api`. Run with `WOODMOUSE_SWAGGER_UI=1` to browse the
API, or load the `openapi.json` attached to each release into any OpenAPI tool.

## Contributing

To run it from source or change it, see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[Hippocratic License 3.0](LICENSE).
