# TODO

## Import / export

- [ ] Import/export (CSV/JSON import/export; Markdown export already ships)

## Email & notifications

- [ ] SMTP/email support
- [ ] Email digests
- [ ] In-app notifications

## Activity feeds

- [ ] Activity feeds scoped per project and per task (a workspace-wide feed already ships)

## Tasks

- [ ] Estimated and actual work time for tasks
- [ ] Sprints, created from the List view
- [ ] Create new tasks from the List view
- [ ] Create new tasks from the Timeline view

## Site admin

- [x] All workspaces on the site admin page; deleted workspaces behind a toggle; add new workspaces with a button
- [x] Edit a workspace from the admin page with its admin settings (users, theme, etc.)
- [x] User editing (and deactivation) and creation from the site admin panel

## Settings

- [x] Pressing the user in the sidebar footer goes to a settings page with its own sidebar, navigating to account settings, and to the super admin settings page for super admins

## Files page

- [x] Per file entry: reference count next to the file size
- [ ] Right side buttons: upload new version, download a version, delete file
- [x] Downloading a file only happens via the button, never by pressing the title

## Backend

- [x] Make sure the database closes properly when the application exits, so SQLite writes the WAL and shm files and deletes them without corruption
- [x] Create a seed command that populates the database with mock data

## Frontend polish

- [x] Clean up UI transitions when navigating
