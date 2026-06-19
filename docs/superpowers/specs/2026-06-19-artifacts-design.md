# Milestone 4 — Markdown Artifacts

## Goal

Make durable documents first-class — the "research/planning" surface. Done when a
user can create, edit, and reopen markdown artifacts inside a workspace (optionally
scoped to a project). Unblocks M11 (dispatch an artifact into coding tasks).

## Decisions

- **Editor**: a plain markdown **source** editor (`.re-textarea`) with an
  **Edit / Preview** toggle (`.re-segmented`). Preview renders via a single
  `renderMarkdown(src)` util — markdown-it `{ html: false, linkify: true }` →
  **DOMPurify** → `v-html`. Not a rich WYSIWYG editor.
- **Saving**: explicit **Save** (enabled only when dirty) + an "Unsaved"
  indicator; switching artifacts while dirty prompts to discard. No autosave.
- **Scoping**: an artifact belongs to a workspace (NOT NULL, ON DELETE CASCADE),
  optionally a project (nullable, ON DELETE SET NULL) — mirrors sources/sessions.

This spec already folds in a multi-discipline design review (0 critical, 12
important, 17 minor); the hardening below comes from that review.

## Data model

Migration `0008_artifacts.sql`:

```sql
CREATE TABLE artifacts (
    id           TEXT PRIMARY KEY NOT NULL,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id   TEXT REFERENCES projects(id) ON DELETE SET NULL,
    title        TEXT NOT NULL,
    content      TEXT NOT NULL DEFAULT '',
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
CREATE INDEX idx_artifacts_workspace ON artifacts(workspace_id);
CREATE INDEX idx_artifacts_project ON artifacts(project_id);
```

`NOT NULL` on the timestamps + `content` matches every prior table (and `from_row`
reads them into non-`Option` `String`s). Registered in `db/mod.rs` `MIGRATIONS` as
version `8`.

## Backend

`models/artifact.rs` — `Artifact { id, workspace_id, project_id: Option<String>,
title, content, created_at, updated_at }` with:
- `create(conn, workspace_id, project_id: Option<&str>, title) -> Artifact` —
  **generates its own id** via `new_id()` (like session/project/repository, not
  the external-id `review::create`); `content` starts `''`, status of timestamps
  via `now_rfc3339()`.
- `get(conn, id) -> Option<Artifact>`
- `list(conn, workspace_id) -> Vec<Artifact>` — full rows (content included),
  ordered `created_at DESC`. Named `list` to match session/project/repository.
- `update(conn, id, title, content) -> Option<Artifact>`
- `delete(conn, id) -> bool`

`commands/artifacts.rs` (standard lock discipline; registered in `lib.rs`):
- `list_artifacts(state, workspace_id) -> Vec<Artifact>` — returns full rows, so
  **no `get_artifact`** command is needed (the view reads from the loaded list,
  matching sessions/reviews).
- `create_artifact(state, workspace_id, project_id: Option<String>, title) ->
  Artifact` — validation parity with `create_session`/`create_repository_source`:
  `title.trim()` non-empty (rejects whitespace-only), the workspace must exist,
  and a provided `project_id` must belong to that workspace
  (`project.workspace_id == workspace_id`, else "Project belongs to a different
  workspace"). The `project_id` FK alone does not enforce the workspace boundary.
- `update_artifact(state, id, title, content) -> Option<Artifact>` — same
  `title.trim()` non-empty check.
- `delete_artifact(state, id) -> bool`

## Frontend

New deps: `markdown-it` (pin `^14`), `dompurify`; devDeps `@types/markdown-it`,
`@types/dompurify` (strict `vue-tsc`).

- `types/artifact.ts` — `Artifact` (mirrors the row; `project_id: string | null`).
- `api/artifacts.ts` — `listArtifacts`, `createArtifact`, `updateArtifact`,
  `deleteArtifact`.
- `utils/markdown.ts` — `renderMarkdown(src: string): string`. Owns a single
  module-level `MarkdownIt({ html: false, linkify: true })`. Renders, then
  `DOMPurify.sanitize(html)`; a DOMPurify `afterSanitizeAttributes` hook adds
  `rel="noopener noreferrer nofollow"` (and `target="_blank"`) to anchors. This is
  the **only** place markdown becomes HTML, and the only `v-html` sink in the app.
- `stores/artifacts.ts` — Pinia setup store: `list`, `loading`, `error`;
  `load(workspaceId)` (monotonic-token guard like the other stores), `create`,
  `update`, `remove`, and **`detachProject(projectId)`** (null out `project_id` of
  matching artifacts in the live list, mirroring `sessions.detachProject`).
- `components/ArtifactsView.vue` — master/detail (like `ReviewsView`):
  - Left: list of artifacts (`data-testid="artifact-row"`, showing title + the
    project name when scoped) + a **New artifact** form (title input
    `aria-label="New artifact title"` + optional project picker
    `aria-label="Artifact project"`; Create disabled when the title is blank).
  - Right (`data-testid="artifact-editor"`): a **title** input
    (`aria-label="Artifact title"`), an **Edit / Preview** `.re-segmented` toggle,
    the source `.re-textarea` (`aria-label="Markdown source"`) in Edit, the
    rendered `.markdown-body` (`data-testid="artifact-preview"`, `v-html` =
    `renderMarkdown(buffer.content)`) in Preview, a **Save** button (disabled
    unless dirty) with an "Unsaved" indicator, and a **Delete** (confirm via
    `useConfirm`).
  - **Edit buffer + dirty**: a local reactive `buffer { title, content }` seeded
    by a `watch` on the selected artifact id (`immediate`); `dirty = buffer.title
    !== saved.title || buffer.content !== saved.content` (value equality). Never
    mutate the store object. On Save, `store.update(...)` replaces the list object
    → the watcher reseeds the buffer → `dirty` clears.
  - **Discard policy**: switching the selected artifact while dirty routes through
    a `useConfirm` discard prompt (cancel keeps the dirty artifact). Workspace
    switch / nav away while dirty is **not** guarded in M4 (the store reload clears
    selection); documented as a known limitation.
- `components/ProjectsView.vue` — `removeProject` also calls
  `artifacts.detachProject(id)` alongside `sessions.detachProject(id)`.
- `App.vue` — add `"artifacts"` to `ActiveView`, an **Artifacts** nav button
  (after Projects), an `<ArtifactsView v-else-if="activeView === 'artifacts'" />`
  branch, and `artifacts.load(workspaceId)` in the workspace-switch watch.

## Security

- The Preview is the app's **first and only `v-html` sink**, and a Tauri webview
  XSS can reach native `invoke` commands (PTY/fs). Two independent layers on the
  sink: markdown-it `{ html: false }` (raw HTML escaped; default `validateLink`
  rejects `javascript:`/`vbscript:`/`file:`/non-image `data:`) **and**
  `DOMPurify.sanitize` of the rendered HTML. `v-html` binds only to
  `renderMarkdown`'s output.
- All SQL parameterized; title validated (trim + non-empty) on create and update.
- **CSP** (final, e2e-gated task): set `tauri.conf.json` `security.csp` from
  `null` to a real policy
  (`default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';
  img-src 'self' data: asset: http://asset.localhost; font-src 'self' data:;
  connect-src 'self' ipc: http://ipc.localhost; object-src 'none';
  base-uri 'self'`). Validated by the full e2e (a broken CSP blanks the app and
  fails every spec); loosen `style-src`/`connect-src`/`img-src` as needed for
  Vue/xterm/Tauri-IPC. If it proves too fiddly to land safely, fall back to a
  minimally-better policy rather than block the milestone.

## Testing

- **Rust** (`models/artifact.rs` + `commands/artifacts.rs`):
  - model CRUD; cascade on workspace delete; **project delete nulls `project_id`
    but keeps the artifact** (ON DELETE SET NULL); realistic multi-line content
    round-trip (newlines, quotes, backticks, the `''` default).
  - command validation: empty title → Err, whitespace-only title → Err,
    nonexistent workspace → Err, project from a different workspace → Err.
- **e2e** (`e2e/specs/artifacts.e2e.ts`, using the `textOf`/`waitUntil` pattern —
  WebKit `getText` returns `''` for some nodes):
  - Create an artifact (assert `New artifact title` Create is disabled when blank,
    enabled with a title) → it appears as an `artifact-row`.
  - Type markdown (`# Hello` + a `[x](javascript:alert(1))` link + a literal
    `<script>alert(1)</script>`); assert the **"Unsaved"** indicator + Save
    enabled; Save.
  - Toggle **Preview**; assert `artifact-preview h1` contains `Hello`, that the
    `javascript:` link rendered **without** a `javascript:` href, and that **no
    `<script>` element** exists in the preview (XSS regression, covering
    `renderMarkdown` end-to-end since there's no frontend unit runner).
  - Edit again → select another artifact while dirty → assert the discard
    `.re-dialog` appears; cancel keeps the dirty artifact, confirm switches.
  - Reselect the first artifact → its saved content is shown.
  - Delete an artifact (confirm) → row removed.
  - Project-detach: create a project + an artifact scoped to it, delete the
    project from Projects, return to Artifacts → the artifact still exists with no
    project (SET NULL surfaced live).

## Out of scope (later)

- Rich WYSIWYG editing, autosave, artifact linking/embedding, a project filter on
  the list, guarding unsaved edits across workspace-switch/nav-away, and `M11`
  (dispatch artifact → coding tasks — the milestone this unblocks).
