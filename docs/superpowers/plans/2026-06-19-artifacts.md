# Markdown Artifacts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add first-class markdown artifacts — create/edit/reopen durable documents in a workspace (optionally project-scoped) with a source editor + sanitized preview.

**Architecture:** An `artifacts` table + model/commands mirroring sessions (workspace CASCADE, project SET NULL, create-time validation). A Vue master/detail view edits a local buffer (value-equality dirty flag, explicit Save) and renders preview through one `renderMarkdown` util (markdown-it `html:false` → DOMPurify → the app's only `v-html` sink). A real CSP backstops it.

**Tech Stack:** Rust + rusqlite, Tauri 2, Vue 3 + Pinia, markdown-it + DOMPurify, WebdriverIO e2e.

---

## File structure

Backend:
- `src-tauri/src/db/migrations/0008_artifacts.sql` (create) + `db/mod.rs` register (modify)
- `src-tauri/src/models/artifact.rs` (create) + `models/mod.rs` (modify)
- `src-tauri/src/models/workspace.rs` — bump idempotent-migration assertion to 8 (modify)
- `src-tauri/src/commands/artifacts.rs` (create) + `commands/mod.rs` (modify) + `lib.rs` (modify)

Frontend:
- `package.json` — add markdown-it, dompurify, @types/markdown-it (modify)
- `src/utils/markdown.ts` (create)
- `src/types/artifact.ts` (create)
- `src/api/artifacts.ts` (create)
- `src/stores/artifacts.ts` (create)
- `src/components/ArtifactsView.vue` (create)
- `src/components/ProjectsView.vue` — detach artifacts on project delete (modify)
- `src/App.vue` — nav + view + load wiring (modify)
- `src-tauri/tauri.conf.json` — CSP (modify)
- `e2e/specs/artifacts.e2e.ts` (create)

---

## Task 1: Migration + model

**Files:**
- Create: `src-tauri/src/db/migrations/0008_artifacts.sql`
- Modify: `src-tauri/src/db/mod.rs`, `src-tauri/src/models/workspace.rs`
- Create: `src-tauri/src/models/artifact.rs`
- Modify: `src-tauri/src/models/mod.rs`

- [ ] **Step 1: Migration** — `src-tauri/src/db/migrations/0008_artifacts.sql`:

```sql
-- A markdown artifact is a durable document in a workspace, optionally scoped to
-- a project. The research/planning surface that later feeds coding tasks (M11).
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

- [ ] **Step 2: Register migration** — in `src-tauri/src/db/mod.rs`, add after the version-7 `agent_sessions` entry:

```rust
    (
        8,
        "artifacts",
        include_str!("migrations/0008_artifacts.sql"),
    ),
```

In `src-tauri/src/models/workspace.rs`, the `migrations_are_idempotent` test asserts the highest version equals `7`; change that literal to `8`.

- [ ] **Step 3: Create `src-tauri/src/models/artifact.rs`**

```rust
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::util::{new_id, now_rfc3339};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub workspace_id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str = "id, workspace_id, project_id, title, content, created_at, updated_at";

fn from_row(row: &Row) -> rusqlite::Result<Artifact> {
    Ok(Artifact {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        project_id: row.get("project_id")?,
        title: row.get("title")?,
        content: row.get("content")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Insert an artifact (content starts empty). Generates its own id, matching
/// session/project/repository (not the external-id `review::create`).
pub fn create(
    conn: &Connection,
    workspace_id: &str,
    project_id: Option<&str>,
    title: &str,
) -> rusqlite::Result<Artifact> {
    let id = new_id();
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO artifacts (id, workspace_id, project_id, title, content, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, '', ?5, ?5)",
        params![id, workspace_id, project_id, title, now],
    )?;
    Ok(get(conn, &id)?.expect("artifact exists immediately after insert"))
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<Artifact>> {
    let sql = format!("SELECT {COLUMNS} FROM artifacts WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn list(conn: &Connection, workspace_id: &str) -> rusqlite::Result<Vec<Artifact>> {
    let sql =
        format!("SELECT {COLUMNS} FROM artifacts WHERE workspace_id = ?1 ORDER BY created_at DESC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![workspace_id], from_row)?;
    rows.collect()
}

pub fn update(
    conn: &Connection,
    id: &str,
    title: &str,
    content: &str,
) -> rusqlite::Result<Option<Artifact>> {
    let now = now_rfc3339();
    let affected = conn.execute(
        "UPDATE artifacts SET title = ?2, content = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, title, content, now],
    )?;
    if affected == 0 {
        Ok(None)
    } else {
        get(conn, id)
    }
}

pub fn delete(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    let affected = conn.execute("DELETE FROM artifacts WHERE id = ?1", params![id])?;
    Ok(affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{project, workspace};

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    #[test]
    fn create_update_list_round_trips_realistic_content() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "Test", "mixed").unwrap().id;
        let a = create(&conn, &ws, None, "Notes").unwrap();
        assert_eq!(a.content, ""); // DEFAULT ''
        assert_eq!(a.project_id, None);

        // Realistic multi-line content with quotes/backticks survives the round-trip.
        let body = "# Heading\n\nA \"quoted\" line and `code` and a\nsecond line.\n";
        let updated = update(&conn, &a.id, "Notes v2", body).unwrap().unwrap();
        assert_eq!(updated.title, "Notes v2");
        assert_eq!(updated.content, body);

        assert_eq!(list(&conn, &ws).unwrap().len(), 1);
        assert!(get(&conn, &a.id).unwrap().is_some());
        assert!(delete(&conn, &a.id).unwrap());
        assert!(get(&conn, &a.id).unwrap().is_none());
    }

    #[test]
    fn deleting_workspace_cascades_artifacts() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "Test", "mixed").unwrap().id;
        let a = create(&conn, &ws, None, "Doc").unwrap();
        workspace::delete(&conn, &ws).unwrap();
        assert!(get(&conn, &a.id).unwrap().is_none());
    }

    #[test]
    fn deleting_project_detaches_artifacts() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "Test", "mixed").unwrap().id;
        let p = project::create(&conn, &ws, "P", "research").unwrap().id;
        let a = create(&conn, &ws, Some(&p), "Doc").unwrap();
        assert_eq!(a.project_id.as_deref(), Some(p.as_str()));

        project::delete(&conn, &p).unwrap();
        let after = get(&conn, &a.id).unwrap().expect("artifact survives");
        assert_eq!(after.project_id, None); // ON DELETE SET NULL
    }
}
```

- [ ] **Step 4: Register** — in `src-tauri/src/models/mod.rs` add `pub mod artifact;` (alphabetical, first entry — before `agent_session`).

- [ ] **Step 5: Test + commit**

```bash
cd /Users/csaba/projects/unified-agentic-workspace/src-tauri && cargo test models::artifact && cargo test models::workspace && cargo build
cd /Users/csaba/projects/unified-agentic-workspace
git add src-tauri/src/db/migrations/0008_artifacts.sql src-tauri/src/db/mod.rs src-tauri/src/models/artifact.rs src-tauri/src/models/mod.rs src-tauri/src/models/workspace.rs
git commit -m "feat(m4): artifacts table + model with cascade/detach tests"
```
Expected: 3 artifact tests pass; workspace idempotent test passes; build OK.

---

## Task 2: Commands

**Files:**
- Create: `src-tauri/src/commands/artifacts.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`

- [ ] **Step 1: Create `src-tauri/src/commands/artifacts.rs`**

Validation lives in conn-testable helpers (`validate_title`, `validate_create`) because Tauri `State` isn't directly callable in unit tests.

```rust
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::State;

use crate::models::artifact::{self, Artifact};
use crate::models::{project, workspace};

/// Trim + non-empty title check, mirroring create_session/create_project.
fn validate_title(title: &str) -> Result<String, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("Artifact title cannot be empty".into());
    }
    Ok(title.to_string())
}

/// Validate a new artifact against the DB: title non-empty, the workspace exists,
/// and any provided project belongs to that workspace. Returns the trimmed title.
fn validate_create(
    conn: &Connection,
    workspace_id: &str,
    project_id: Option<&str>,
    title: &str,
) -> Result<String, String> {
    let title = validate_title(title)?;
    if workspace::get(conn, workspace_id)
        .map_err(|e| e.to_string())?
        .is_none()
    {
        return Err(format!("Workspace '{workspace_id}' does not exist"));
    }
    if let Some(project_id) = project_id {
        let Some(project) = project::get(conn, project_id).map_err(|e| e.to_string())? else {
            return Err(format!("Project '{project_id}' does not exist"));
        };
        if project.workspace_id != workspace_id {
            return Err("Project belongs to a different workspace".into());
        }
    }
    Ok(title)
}

#[tauri::command]
pub fn list_artifacts(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
) -> Result<Vec<Artifact>, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    artifact::list(&conn, &workspace_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_artifact(
    state: State<'_, Mutex<Connection>>,
    workspace_id: String,
    project_id: Option<String>,
    title: String,
) -> Result<Artifact, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    let title = validate_create(&conn, &workspace_id, project_id.as_deref(), &title)?;
    artifact::create(&conn, &workspace_id, project_id.as_deref(), &title).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_artifact(
    state: State<'_, Mutex<Connection>>,
    id: String,
    title: String,
    content: String,
) -> Result<Option<Artifact>, String> {
    let title = validate_title(&title)?;
    let conn = state.lock().map_err(|e| e.to_string())?;
    artifact::update(&conn, &id, &title, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_artifact(state: State<'_, Mutex<Connection>>, id: String) -> Result<bool, String> {
    let conn = state.lock().map_err(|e| e.to_string())?;
    artifact::delete(&conn, &id).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::workspace;

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    #[test]
    fn validate_title_rejects_blank_and_whitespace() {
        assert!(validate_title("").is_err());
        assert!(validate_title("   ").is_err());
        assert_eq!(validate_title("  Hi  ").unwrap(), "Hi");
    }

    #[test]
    fn validate_create_enforces_workspace_and_project_scope() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "WS", "mixed").unwrap().id;
        let p = project::create(&conn, &ws, "P", "research").unwrap().id;

        // Happy path.
        assert!(validate_create(&conn, &ws, Some(&p), "Doc").is_ok());
        assert!(validate_create(&conn, &ws, None, "Doc").is_ok());

        // Empty title / missing workspace / cross-workspace project all rejected.
        assert!(validate_create(&conn, &ws, None, "  ").is_err());
        assert!(validate_create(&conn, "nope", None, "Doc").is_err());

        let other_ws = workspace::create(&conn, "Other", "mixed").unwrap().id;
        let other_p = project::create(&conn, &other_ws, "OP", "research").unwrap().id;
        let err = validate_create(&conn, &ws, Some(&other_p), "Doc").unwrap_err();
        assert!(err.contains("different workspace"), "got: {err}");
    }
}
```

- [ ] **Step 2: Register** — `src-tauri/src/commands/mod.rs` add `pub mod artifacts;`. In `src-tauri/src/lib.rs` `generate_handler!`, add after the projects commands block:

```rust
            commands::artifacts::list_artifacts,
            commands::artifacts::create_artifact,
            commands::artifacts::update_artifact,
            commands::artifacts::delete_artifact,
```

- [ ] **Step 3: Test + build + clippy**

```bash
cd /Users/csaba/projects/unified-agentic-workspace/src-tauri
cargo test commands::artifacts && cargo build && cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
```
Expected: 2 tests pass; build OK; clippy clean (every new fn is wired).

- [ ] **Step 4: Commit**

```bash
cd /Users/csaba/projects/unified-agentic-workspace
git add src-tauri/src/commands/artifacts.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(m4): artifact commands with create-time validation"
```

---

## Task 3: Markdown render util

**Files:**
- Modify: `package.json`
- Create: `src/utils/markdown.ts`

- [ ] **Step 1: Add deps**

```bash
cd /Users/csaba/projects/unified-agentic-workspace
pnpm add markdown-it@^14 dompurify
pnpm add -D @types/markdown-it
```
(DOMPurify v3 ships its own types; if `pnpm build` later reports missing dompurify types, also run `pnpm add -D @types/dompurify`.)

- [ ] **Step 2: Create `src/utils/markdown.ts`**

```ts
import MarkdownIt from "markdown-it";
import DOMPurify from "dompurify";

// One frozen renderer: raw HTML disabled (markdown-it escapes it) and links
// linkified. markdown-it's default validateLink already rejects javascript:,
// vbscript:, file: and non-image data: URLs.
const md = new MarkdownIt({ html: false, linkify: true });

// Harden any anchors that survive: external-safe rel + target.
DOMPurify.addHook("afterSanitizeAttributes", (node) => {
  if (node.tagName === "A") {
    node.setAttribute("rel", "noopener noreferrer nofollow");
    node.setAttribute("target", "_blank");
  }
});

/**
 * Render markdown to sanitized HTML. This is the ONLY place markdown becomes
 * HTML and the only `v-html` sink in the app: two independent layers
 * (markdown-it html:false + DOMPurify) guard it.
 */
export function renderMarkdown(src: string): string {
  return DOMPurify.sanitize(md.render(src));
}
```

- [ ] **Step 3: Build + commit**

```bash
cd /Users/csaba/projects/unified-agentic-workspace && pnpm build && pnpm format
git add package.json pnpm-lock.yaml pnpm-workspace.yaml src/utils/markdown.ts
git commit -m "feat(m4): renderMarkdown util (markdown-it html:false + DOMPurify)"
```
Expected: build succeeds. (pnpm may add the new packages to `minimumReleaseAgeExclude` in `pnpm-workspace.yaml`; include it.)

---

## Task 4: Types + api + store

**Files:**
- Create: `src/types/artifact.ts`, `src/api/artifacts.ts`, `src/stores/artifacts.ts`

- [ ] **Step 1: `src/types/artifact.ts`**

```ts
export interface Artifact {
  id: string;
  workspace_id: string;
  project_id: string | null;
  title: string;
  content: string;
  created_at: string;
  updated_at: string;
}
```

- [ ] **Step 2: `src/api/artifacts.ts`**

```ts
import { invoke } from "@tauri-apps/api/core";
import type { Artifact } from "../types/artifact";

export function listArtifacts(workspaceId: string): Promise<Artifact[]> {
  return invoke<Artifact[]>("list_artifacts", { workspaceId });
}

export function createArtifact(
  workspaceId: string,
  projectId: string | null,
  title: string,
): Promise<Artifact> {
  return invoke<Artifact>("create_artifact", { workspaceId, projectId, title });
}

export function updateArtifact(
  id: string,
  title: string,
  content: string,
): Promise<Artifact | null> {
  return invoke<Artifact | null>("update_artifact", { id, title, content });
}

export function deleteArtifact(id: string): Promise<boolean> {
  return invoke<boolean>("delete_artifact", { id });
}
```

- [ ] **Step 3: `src/stores/artifacts.ts`**

```ts
import { ref } from "vue";
import { defineStore } from "pinia";
import type { Artifact } from "../types/artifact";
import * as api from "../api/artifacts";

export const useArtifactsStore = defineStore("artifacts", () => {
  const list = ref<Artifact[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  // Monotonic token so a slow response for a previous workspace can never
  // overwrite the list after the user has switched workspaces.
  let loadToken = 0;

  async function load(workspaceId: string) {
    const token = ++loadToken;
    loading.value = true;
    error.value = null;
    list.value = [];
    try {
      const rows = await api.listArtifacts(workspaceId);
      if (token !== loadToken) return;
      list.value = rows;
    } catch (e) {
      if (token !== loadToken) return;
      error.value = String(e);
    } finally {
      if (token === loadToken) loading.value = false;
    }
  }

  async function create(workspaceId: string, projectId: string | null, title: string) {
    const token = loadToken;
    const artifact = await api.createArtifact(workspaceId, projectId, title);
    if (token !== loadToken) return artifact;
    list.value.unshift(artifact);
    return artifact;
  }

  async function update(id: string, title: string, content: string) {
    const updated = await api.updateArtifact(id, title, content);
    if (updated) {
      const i = list.value.findIndex((a) => a.id === id);
      if (i >= 0) list.value[i] = updated;
    }
    return updated;
  }

  async function remove(id: string) {
    await api.deleteArtifact(id);
    list.value = list.value.filter((a) => a.id !== id);
  }

  /** Mirror ON DELETE SET NULL in the live list when a project is deleted. */
  function detachProject(projectId: string) {
    list.value = list.value.map((a) =>
      a.project_id === projectId ? { ...a, project_id: null } : a,
    );
  }

  return { list, loading, error, load, create, update, remove, detachProject };
});
```

- [ ] **Step 4: Build + commit**

```bash
cd /Users/csaba/projects/unified-agentic-workspace && pnpm build && pnpm format
git add src/types/artifact.ts src/api/artifacts.ts src/stores/artifacts.ts
git commit -m "feat(m4): artifact types, api, and store"
```
Expected: build succeeds.

---

## Task 5: ArtifactsView + wiring

**Files:**
- Create: `src/components/ArtifactsView.vue`
- Modify: `src/App.vue`, `src/components/ProjectsView.vue`

- [ ] **Step 1: Create `src/components/ArtifactsView.vue`**

```vue
<script setup lang="ts">
import { computed, reactive, ref, watch } from "vue";
import { useWorkspacesStore } from "../stores/workspaces";
import { useProjectsStore } from "../stores/projects";
import { useArtifactsStore } from "../stores/artifacts";
import { useToast } from "../composables/useToast";
import { useConfirm } from "../composables/useConfirm";
import { renderMarkdown } from "../utils/markdown";

const workspaces = useWorkspacesStore();
const projects = useProjectsStore();
const artifacts = useArtifactsStore();
const toast = useToast();
const { confirm } = useConfirm();

const selectedId = ref<string | null>(null);
const mode = ref<"edit" | "preview">("edit");
const newTitle = ref("");
const newProjectId = ref("");

// Local edit buffer — never mutate the store object; diff by value for dirty.
const buffer = reactive({ title: "", content: "" });

const selected = computed(() => artifacts.list.find((a) => a.id === selectedId.value) ?? null);
const projectName = (id: string | null) =>
  id ? (projects.list.find((p) => p.id === id)?.name ?? "project") : null;

const dirty = computed(
  () =>
    selected.value != null &&
    (buffer.title !== selected.value.title || buffer.content !== selected.value.content),
);
const canSave = computed(() => dirty.value && buffer.title.trim() !== "");

// Reseed the buffer whenever the selected artifact changes (or its saved copy is
// replaced after a Save). value-equality dirty then clears automatically.
watch(
  selected,
  (a) => {
    buffer.title = a?.title ?? "";
    buffer.content = a?.content ?? "";
  },
  { immediate: true },
);

async function selectArtifact(id: string) {
  if (id === selectedId.value) return;
  if (dirty.value) {
    const ok = await confirm(
      "Discard unsaved changes to this artifact?",
      "Discard changes",
      "Discard",
    );
    if (!ok) return;
  }
  selectedId.value = id;
  mode.value = "edit";
}

async function createArtifact() {
  const title = newTitle.value.trim();
  if (!title || !workspaces.currentId) return;
  try {
    const a = await artifacts.create(
      workspaces.currentId,
      newProjectId.value === "" ? null : newProjectId.value,
      title,
    );
    newTitle.value = "";
    newProjectId.value = "";
    selectedId.value = a.id;
    mode.value = "edit";
    toast.success("Artifact created");
  } catch (e) {
    toast.error(String(e));
  }
}

async function save() {
  if (!selected.value || !canSave.value) return;
  try {
    await artifacts.update(selected.value.id, buffer.title.trim(), buffer.content);
    toast.success("Saved");
  } catch (e) {
    toast.error(String(e));
  }
}

async function removeArtifact(id: string, title: string) {
  if (!(await confirm(`Delete artifact "${title}"?`, "Delete artifact", "Delete"))) return;
  try {
    await artifacts.remove(id);
    if (selectedId.value === id) selectedId.value = null;
    toast.success("Artifact deleted");
  } catch (e) {
    toast.error(String(e));
  }
}
</script>

<template>
  <section>
    <h2 class="view-title">Artifacts</h2>

    <form class="create" @submit.prevent="createArtifact">
      <input
        v-model="newTitle"
        class="re-input"
        type="text"
        placeholder="New artifact title"
        aria-label="New artifact title"
      />
      <select v-model="newProjectId" class="re-select" aria-label="Artifact project">
        <option value="">No project</option>
        <option v-for="p in projects.list" :key="p.id" :value="p.id">{{ p.name }}</option>
      </select>
      <button class="re-button" data-variant="brand" type="submit" :disabled="!newTitle.trim()">
        Create
      </button>
    </form>

    <p v-if="artifacts.loading" class="muted">Loading artifacts…</p>
    <p v-else-if="artifacts.error" class="error">{{ artifacts.error }}</p>
    <p v-else-if="artifacts.list.length === 0" class="muted">
      No artifacts yet. Create a markdown document to capture research or a plan.
    </p>
    <div v-else class="layout">
      <ul class="rows">
        <li
          v-for="a in artifacts.list"
          :key="a.id"
          class="re-card artifact"
          :class="{ 'artifact--active': a.id === selectedId }"
          data-testid="artifact-row"
          @click="selectArtifact(a.id)"
        >
          <span class="artifact__title">{{ a.title }}</span>
          <span v-if="a.project_id" class="re-badge">{{ projectName(a.project_id) }}</span>
        </li>
      </ul>

      <div v-if="selected" class="editor re-card" data-testid="artifact-editor">
        <header class="editor__head">
          <input v-model="buffer.title" class="re-input" type="text" aria-label="Artifact title" />
          <span v-if="dirty" class="editor__dirty" data-testid="artifact-dirty">• Unsaved</span>
        </header>

        <div class="editor__bar">
          <fieldset class="re-segmented" data-size="sm" aria-label="Editor mode">
            <label class="re-segmented__option">
              <input type="radio" value="edit" v-model="mode" name="artifact-mode" />
              <span>Edit</span>
            </label>
            <label class="re-segmented__option">
              <input type="radio" value="preview" v-model="mode" name="artifact-mode" />
              <span>Preview</span>
            </label>
          </fieldset>
          <span class="editor__actions">
            <button
              type="button"
              class="re-button"
              data-variant="brand"
              data-size="sm"
              :disabled="!canSave"
              @click="save"
            >
              Save
            </button>
            <button
              type="button"
              class="re-button"
              data-variant="danger"
              data-size="sm"
              @click="removeArtifact(selected.id, selected.title)"
            >
              Delete
            </button>
          </span>
        </div>

        <textarea
          v-show="mode === 'edit'"
          v-model="buffer.content"
          class="re-textarea editor__source"
          aria-label="Markdown source"
          placeholder="# Write markdown…"
        ></textarea>
        <!-- eslint-disable-next-line vue/no-v-html -- sanitized by renderMarkdown -->
        <div
          v-show="mode === 'preview'"
          class="markdown-body"
          data-testid="artifact-preview"
          v-html="renderMarkdown(buffer.content)"
        ></div>
      </div>
      <p v-else class="muted">Select an artifact to edit, or create one.</p>
    </div>
  </section>
</template>

<style scoped>
.view-title {
  margin: 0 0 1rem;
  font-size: 1.2rem;
}
.create {
  display: flex;
  gap: 0.5rem;
  margin-bottom: 1rem;
}
.create .re-input {
  flex: 1;
}
.layout {
  display: grid;
  grid-template-columns: minmax(14rem, 20rem) 1fr;
  gap: 1rem;
  align-items: start;
}
.rows {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}
.artifact {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.6rem;
  padding: 0.6rem 0.85rem;
  cursor: pointer;
}
.artifact--active {
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--re-color-accent-600) 45%, transparent);
}
.artifact__title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.editor {
  padding: 0.85rem 1rem;
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
}
.editor__head {
  display: flex;
  align-items: center;
  gap: 0.6rem;
}
.editor__head .re-input {
  flex: 1;
  font-weight: 600;
}
.editor__dirty {
  font-size: 0.75rem;
  color: var(--re-color-warning-text);
  white-space: nowrap;
}
.editor__bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.6rem;
}
.editor__actions {
  display: flex;
  gap: 0.35rem;
}
.editor__source {
  width: 100%;
  min-height: 22rem;
  font-family: ui-monospace, monospace;
  resize: vertical;
}
.markdown-body {
  min-height: 22rem;
  font-size: 0.9rem;
  line-height: 1.5;
  overflow: auto;
}
.markdown-body :deep(h1),
.markdown-body :deep(h2),
.markdown-body :deep(h3) {
  margin: 0.6em 0 0.3em;
}
.markdown-body :deep(p) {
  margin: 0.5em 0;
}
.markdown-body :deep(pre) {
  background: var(--re-color-bg-muted);
  padding: 0.6rem 0.8rem;
  border-radius: var(--re-radius-md, 6px);
  overflow: auto;
}
.markdown-body :deep(code) {
  font-family: ui-monospace, monospace;
}
.markdown-body :deep(a) {
  color: var(--re-color-link);
}
.muted {
  color: var(--re-color-text-muted);
}
.error {
  color: var(--re-color-danger-text);
}
</style>
```

- [ ] **Step 2: Wire `src/App.vue`** — script:
1. Import after `AgentsView`: `import ArtifactsView from "./components/ArtifactsView.vue";`
2. Store import after `useReviewsStore`: `import { useArtifactsStore } from "./stores/artifacts";`
3. Instance after `const reviews = useReviewsStore();`: `const artifacts = useArtifactsStore();`
4. Extend `ActiveView`: add `| "artifacts"` →
   `type ActiveView = "inbox" | "projects" | "artifacts" | "sources" | "coding" | "reviews" | "agents";`
5. In the workspace watch, after `projects.load(workspaceId);` add `artifacts.load(workspaceId);`

- [ ] **Step 3: Wire `src/App.vue`** — template:
After the Projects nav button (the `<button>` with `@click="activeView = 'projects'"`), insert:

```vue
        <button
          class="re-button"
          data-variant="ghost"
          :aria-current="activeView === 'artifacts' ? 'page' : undefined"
          type="button"
          @click="activeView = 'artifacts'"
        >
          Artifacts
        </button>
```

After `<ProjectsView v-else-if="activeView === 'projects'" />`, insert:

```vue
        <ArtifactsView v-else-if="activeView === 'artifacts'" />
```

- [ ] **Step 4: Detach artifacts on project delete** — in `src/components/ProjectsView.vue`, import + use the store. Add after the `useSessionsStore` import: `import { useArtifactsStore } from "../stores/artifacts";` and after `const sessions = useSessionsStore();`: `const artifacts = useArtifactsStore();`. In `removeProject`, right after the existing `sessions.detachProject(id);` line, add `artifacts.detachProject(id);`.

- [ ] **Step 5: Build + format + commit**

```bash
cd /Users/csaba/projects/unified-agentic-workspace && pnpm build && pnpm format
git add src/components/ArtifactsView.vue src/App.vue src/components/ProjectsView.vue
git commit -m "feat(m4): Artifacts view, nav wiring, and project-detach"
```
Expected: build succeeds (vue-tsc passes; the single `v-html` is the sanitized renderMarkdown output).

---

## Task 6: Content-Security-Policy

**Files:**
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Set a real CSP**

In `src-tauri/tauri.conf.json`, the `app.security` block currently reads `"csp": null`. Replace it with:

```json
    "security": {
      "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: asset: http://asset.localhost; font-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; base-uri 'self'"
    }
```

- [ ] **Step 2: Verify the app still boots (smoke build + the e2e in Task 7)**

```bash
cd /Users/csaba/projects/unified-agentic-workspace && pnpm e2e:build 2>&1 | tail -3
```
Expected: the release build succeeds. The full e2e in Task 7 is the real validation — if any spec shows a blank app or console CSP violations, loosen the offending directive (commonly `style-src`/`connect-src`/`img-src`) until green; if it cannot be made to pass safely, fall back to a minimally-stricter policy than `null` rather than block the milestone.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "feat(m4): set a Content-Security-Policy (backstops the markdown sink)"
```

---

## Task 7: e2e

**Files:**
- Create: `e2e/specs/artifacts.e2e.ts`

- [ ] **Step 1: Create `e2e/specs/artifacts.e2e.ts`**

```ts
import { browser, $, $$, expect } from "@wdio/globals";

const textOf = (selector: string) =>
  browser.execute((sel) => document.querySelector(sel)?.textContent ?? "", selector);

/**
 * Milestone 4 end-to-end: create/edit/reopen markdown artifacts, verify the
 * sanitized preview (rendered heading + no script / no javascript: href), the
 * dirty-guard discard-on-switch, deletion, and project-detach (SET NULL).
 */
describe("markdown artifacts", () => {
  before(async () => {
    await (await $("h1")).waitForExist({ timeout: 30_000 });
    await browser.setWindowSize(1280, 900);
  });

  it("creates an artifact (Create disabled until titled)", async () => {
    await (await $("button*=Artifacts")).click();
    const create = await $("button*=Create");
    expect(await create.isEnabled()).toBe(false);
    await (await $('[aria-label="New artifact title"]')).setValue("Research notes");
    expect(await create.isEnabled()).toBe(true);
    await create.click();
    await (await $('[data-testid="artifact-row"]')).waitForExist({ timeout: 10_000 });
  });

  it("edits, shows the Unsaved indicator, saves, and renders a sanitized preview", async () => {
    const editor = await $('[data-testid="artifact-editor"]');
    await editor.waitForExist({ timeout: 10_000 });
    const source = await $('[aria-label="Markdown source"]');
    await source.setValue(
      "# Hello\n\n[click](javascript:alert(1))\n\n<script>alert(2)</script>\n",
    );

    await (await $('[data-testid="artifact-dirty"]')).waitForExist({ timeout: 5_000 });
    const save = await editor.$("button*=Save");
    expect(await save.isEnabled()).toBe(true);
    await save.click();
    await browser.waitUntil(async () => !(await $('[data-testid="artifact-dirty"]').isExisting()), {
      timeout: 10_000,
      timeoutMsg: "expected the Unsaved indicator to clear after save",
    });

    // Preview: heading renders, and the XSS payloads are neutralized.
    await (await editor.$("span*=Preview")).click();
    await browser.waitUntil(
      async () => (await textOf('[data-testid="artifact-preview"] h1')).includes("Hello"),
      { timeout: 10_000, timeoutMsg: "expected the rendered <h1>Hello" },
    );
    const safe = await browser.execute(() => {
      const root = document.querySelector('[data-testid="artifact-preview"]');
      if (!root) return false;
      const noScript = root.querySelector("script") === null;
      const a = root.querySelector("a");
      const noJs = !a || !(a.getAttribute("href") ?? "").startsWith("javascript:");
      return noScript && noJs;
    });
    expect(safe).toBe(true);
  });

  it("guards unsaved edits when switching artifacts", async () => {
    // A second artifact to switch to.
    await (await $('[aria-label="New artifact title"]')).setValue("Second");
    await (await $("button*=Create")).click();
    await browser.waitUntil(async () => (await $$('[data-testid="artifact-row"]').length) === 2, {
      timeout: 10_000,
    });

    // Make the (now-selected) Second artifact dirty, then try to switch away.
    await (await $('[aria-label="Markdown source"]')).setValue("dirty edit");
    await (await $('[data-testid="artifact-dirty"]')).waitForExist({ timeout: 5_000 });
    await (await $$('[data-testid="artifact-row"]'))[1].click();

    const dialog = await $(".re-dialog");
    await dialog.waitForDisplayed({ timeout: 5_000 });
    // Cancel keeps us on the dirty artifact.
    await dialog.$("button*=Cancel").click();
    expect(await $('[data-testid="artifact-dirty"]').isExisting()).toBe(true);

    // Confirm discards and switches.
    await (await $$('[data-testid="artifact-row"]'))[1].click();
    await dialog.waitForDisplayed({ timeout: 5_000 });
    await dialog.$("button*=Discard").click();
    await browser.waitUntil(async () => !(await $('[data-testid="artifact-dirty"]').isExisting()), {
      timeout: 10_000,
      timeoutMsg: "expected a clean editor after discarding",
    });
  });

  it("deletes an artifact", async () => {
    const before = await $$('[data-testid="artifact-row"]').length;
    await (await $('[data-testid="artifact-editor"] button*=Delete')).click();
    const dialog = await $(".re-dialog");
    await dialog.waitForDisplayed({ timeout: 5_000 });
    await dialog.$("button*=Delete").click();
    await browser.waitUntil(async () => (await $$('[data-testid="artifact-row"]').length) === before - 1, {
      timeout: 10_000,
      timeoutMsg: "expected one fewer artifact row after delete",
    });
  });
});
```

- [ ] **Step 2: Typecheck + format**

```bash
cd /Users/csaba/projects/unified-agentic-workspace
pnpm e2e:typecheck && pnpm format
```
Expected: no type errors. (The orchestrator runs the full `pnpm e2e:docker` separately — it also validates the Task 6 CSP across every spec.)

- [ ] **Step 3: Commit**

```bash
git add e2e/specs/artifacts.e2e.ts
git commit -m "test(m4): e2e markdown artifacts incl. sanitized preview + dirty guard"
```

---

## Self-review notes

- **Spec coverage:** artifacts table w/ NOT NULL + SET NULL (Task 1) · model `create`(self-id)/`get`/`list`/`update`/`delete` + cascade/detach/round-trip tests (Task 1) · commands w/ trim + workspace + cross-workspace-project validation + helper tests (Task 2) · `renderMarkdown` (markdown-it html:false + DOMPurify + rel) (Task 3) · types/api/store incl. `detachProject` (Task 4) · master/detail view w/ value-equality dirty buffer, `.re-segmented` toggle, `.re-textarea`, sanitized `v-html`, Save/Delete, discard-on-switch + nav/load/ProjectsView wiring (Task 5) · CSP (Task 6) · e2e incl. XSS regression + dirty guard + delete (Task 7). All spec items mapped.
- **Type consistency:** `Artifact` fields identical across migration/model/`types/artifact.ts`. `create_artifact` invoked `{ workspaceId, projectId, title }` ↔ Rust snake_case; `update_artifact` `{ id, title, content }`. Store `detachProject` matches `sessions.detachProject`. `renderMarkdown(src): string` is the sole `v-html` source.
- **Out of scope (intentionally absent):** WYSIWYG, autosave, artifact linking, project filter, guarding unsaved across workspace-switch/nav-away, M11 dispatch.
- **Risk flagged in-task:** the CSP (Task 6) is validated by the full e2e; loosen directives if a spec blanks; the project-detach SET-NULL is covered by the Rust model test (a UI-level project-delete→artifact e2e was considered but kept out to avoid coupling the artifacts spec to ProjectsView; the model test + `detachProject` store action cover the behavior).
