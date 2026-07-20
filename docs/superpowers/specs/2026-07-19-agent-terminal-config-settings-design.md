# Agent & Terminal Config — Settings Page (Slice ②) — Design

**Goal:** An in-app **Settings** page to edit the config Slice ① introduced — per-PTY-agent `bin`/`args` and terminal `fontSize` — with **merge-on-save** that preserves the hand-edited `theme` + any unknown keys, applied to terminals when you return to them.

**Status:** Slice ② of the config feature (Slice ① shipped v0.1.7: the JSON read path). This builds the write path + UI.

**Decisions (confirmed in brainstorming):** page edits per-agent `bin`/`args` + `fontSize` only (palette stays hand-edit in `config.json`, like kommand0); changes apply to open terminals.

> Revised after `cst:plan-review` (2026-07-20): `agents` DTO → keyed map + write-side PTY whitelist; `merge_edits` guards nested non-objects; **the live-apply watcher was dropped** (terminals unmount when Settings is shown — live-apply is the existing remount re-read); SettingsView renders outside the workspace guard; atomic write moved into `services/` with symlink-safe temp + the read-path guards; `SaveResult` collapsed to `Result<TerminalOut, String>`; test/e2e tightened.

---

## Live-apply — how it actually works (important)

`App.vue` renders views with a plain `v-if/v-else-if` chain (no `<KeepAlive>`), so **opening Settings unmounts `AgentsView` and every `TerminalTab`**. Therefore there is **no watcher** — a watcher could never fire from a save (no terminal is mounted while Settings is open). Instead: `save()` sets the store's `terminal.value` to the freshly-merged config, and when the user **returns to Agents** the PTY sessions (persisted as store tabs) remount and `TerminalTab`'s existing `onMounted` reads `appConfig.terminal` → the new `fontSize`/`theme` apply. Net UX: change settings → go back to the terminal → it's updated. This is "apply to open terminals" for this shell; genuine in-place apply would need Settings to be a modal/overlay keeping Agents mounted (out of scope). `bin`/`args` apply to the next spawn (config is re-read per spawn).

## Architecture

- **Nav (`App.vue`):** remove `"Settings"` from `plannedSections`, add an `activeView === 'settings'` nav button + `'settings'` to the `ActiveView` union. Render `SettingsView` **outside the `workspaces.current` guard** — every other view sits under `<template v-else-if="workspaces.current">` + the workspace header, but config is **app-global** and must be reachable with zero/loading/errored workspaces (exactly when you'd fix a broken `bin`). Shape: `<SettingsView v-if="activeView === 'settings'" />` as the first branch in `main`, the existing loading/error/current block moved under a `v-else`. SettingsView carries its own header (no workspace name/badge).
- **Backend:** the write path mirrors Slice ①'s read seam — pure/path-parameterized logic in `services/config.rs`, `AppHandle`/path resolution at the `commands/config.rs` boundary. New service fns: `merge_edits` (pure), `edit_view` (pure projection), `read_config_raw(path)` + `write_config_atomic(path, contents)` (path-parameterized fs, tempfile-testable), `valid_font_size`. New commands: `get_config_for_edit`, `save_config`. A shared `config_file_path(app) -> Option<PathBuf>` (extracted from `load`, used by all three) holds the "never CWD-relative / `None` if `app_data_dir()` fails" guard once.
- **Frontend:** `SettingsView.vue` (form), `useAppConfigStore` gains `getForEdit()` + `save(edits)`, `api/appConfig.ts` gains the two wrappers. `TerminalTab.vue`'s **only** change is binding the host background to `theme.background` (cosmetic — no `#000` frame); its existing mount-time read already delivers live-apply on remount.

## Data shapes (frontend ⇄ backend)

```ts
interface AgentEdit { bin: string | null; args: string[] }
// agents keyed by PTY adapter id (kebab-case) — a MAP, not a struct
// (rename_all can't produce "claude-code"; a fixed struct silently drops it)
interface EditConfig { agents: Record<string, AgentEdit>; fontSize: number }
interface ConfigForEdit { agents: Record<string, AgentEdit>; fontSize: number; warning: string | null }

// commands:
//   get_config_for_edit() -> ConfigForEdit
//   save_config(edits: EditConfig) -> TerminalOut     // Ok; Err(string) rejects the invoke
```
Rust: `EditConfig.agents` is `BTreeMap<String, AgentEdit>` (keyed by id); only `font_size` needs `#[serde(rename)]` → `fontSize`. `AgentEdit { bin: Option<String>, args: Vec<String> }`. `save_config -> Result<TerminalOut, String>` (reuse Slice ①'s `TerminalOut`); `Ok` drives live-apply, `Err` (validation **or** fs) rejects the invoke and the frontend `catch`es it.

## Backend

### `config_file_path(app) -> Option<PathBuf>` (refactor)
Extract from `load`: `Some(config_path(UAW_CONFIG_PATH, app_data_dir))` or `None` if `app_data_dir()` errors (never a CWD-relative path). `load`, `get_config_for_edit`, and `save_config` all use it.

### `edit_view(cfg: &Config) -> EditConfig` (pure, in services)
For each id in `PTY_AGENT_IDS`: `agents[id]` = the parsed `{bin, args}` or `{bin:null, args:[]}` if absent; `fontSize` = `cfg.terminal.font_size`. Unit-testable without `AppHandle`.

### `get_config_for_edit(app) -> ConfigForEdit`
`config_file_path` → `read_config_at` (returns `(Config, Option<warning>)`) → `edit_view(&cfg)` + the `warning`, so the form can surface "config.json is unparseable — fix it by hand before editing here" **up front** (not only on Save). `None` path → defaults + a warning.

### `save_config(app, edits) -> Result<TerminalOut, String>`
1. `config_file_path(app)` → `Err("could not resolve the config directory")` if `None` (writes nothing — never CWD-relative).
2. `read_config_raw(path)` — a **guarded** raw read in services: reuse `read_config_at`'s posture (refuse symlink/non-regular, refuse `>64 KiB`); **absent OR blank/whitespace-only → `"{}"`** (a blank file has nothing to lose → create-fresh, not "fix by hand"); symlink/oversize → `Err`.
3. `merge_edits(&raw, &edits)?` (pure — below).
4. `write_config_atomic(path, &merged)?` — `create_dir_all(parent)` first (app-data dir may not exist — Slice ① only read); create the temp with `OpenOptions::new().write(true).create_new(true)` (O_EXCL — won't follow a pre-planted symlink) at `config.json.<util::new_id()>.tmp` (unique → no fixed-name race/clobber); write; `fs::rename` over `config.json`; remove the temp on any error. Dep-free.
5. Re-read via `read_config_at` → return `Ok(TerminalOut { font_size, theme })` for live-apply.

### `merge_edits(raw: &str, edits: &EditConfig) -> Result<String, String>` (pure — the test seam)
- Parse `raw` → `Value`. If **not a JSON object** → `Err("config.json isn't valid JSON — fix it by hand before saving from Settings.")` (never clobber a recoverable file).
- Validate `edits.font_size` with the shared `valid_font_size` (`6..=72`) → else `Err("Font size must be 6–72.")`.
- Merge by **in-place mutation** (never rebuild `terminal`/`agents`, so unknown sub-keys survive), **guarding every nested container** — use `as_object_mut()`; if a needed nested value exists but isn't an object (`{"terminal":5}`, `{"agents":[]}`, `{"agents":{"codex":5}}`), replace *that* value with a fresh object (don't index-assign into a non-object → would panic):
  - `terminal.fontSize` = the value.
  - For each id in `edits.agents` **that is in `PTY_AGENT_IDS`** (whitelist — a non-PTY/`claude-agent-sdk` key in the payload is ignored, never written): set `agents[id].bin` (trim; `None`/empty/whitespace → **remove** the `bin` key) and `agents[id].args` (trim each, drop blanks; may be `[]`), touching only `bin`/`args` (unknown keys inside that agent object survive).
  - **Every other key** — `theme` under `terminal`, other/unknown agents, top-level unknowns — is left untouched.
- Return pretty-printed JSON.

## Frontend

- **`api/appConfig.ts`:** `getConfigForEdit()` + `saveConfig(edits)`.
- **`useAppConfigStore`:** `getForEdit()` (passthrough to `api.getConfigForEdit`); `save(edits)` — `try { const t = await api.saveConfig(edits); terminal.value = t; return {ok:true} } catch (e) { return {ok:false, error:String(e)} }` (a rejected invoke — validation, serde, or fs — is caught, not unhandled). `terminal` stays a `ref` (setting `terminal.value` to the returned `TerminalOut` is what a later remount reads).
- **`SettingsView.vue`:** on mount `appConfig.getForEdit()` → if `warning`, `toast.error(warning)`; seed form refs: per agent `bin = agent.bin ?? ""`, `argsText = agent.args.join("\n")`; `fontSize`. `<form @submit.prevent="save">` with, per agent, a `re-input` **Binary** + a `<textarea class="re-input">` **Args** (one per line), a numeric `<input type="number" min="6" max="72" v-model.number="fontSize" class="re-input">`, and a `re-button data-variant="brand"` Save. On save: guard `Number.isFinite(fontSize)` (empty/NaN → inline error, return); build `EditConfig` (agents map keyed by id; `bin` → trimmed or null; `argsText.split("\n").map(trim).filter(Boolean)`); a `submitting` ref disables Save during the async call; `appConfig.save(edits)` → `ok` → `toast.success("Settings saved.")`, `!ok` → `toast.error(error)`. Each field has an `aria-label`; agents grouped in `<fieldset><legend>`. A muted footnote: "Terminal colours: edit `theme` in config.json." `data-testid`s for e2e.
- **`TerminalTab.vue`:** bind the host: `:style="{ background: appConfig.terminal.theme.background }"` so a non-black `theme.background` isn't framed by the `.terminal { background:#000 }` padding. (No watcher; the mount-time read is unchanged.)

## Error handling / validation
- Invalid raw (present, non-object JSON) → refuse, "fix by hand" (don't clobber). Blank/absent → create-fresh.
- Nested non-object → coerced to a fresh object in the merge (no panic).
- `fontSize` ∉ `6..=72` → `Err` (shared `valid_font_size`; the numeric input's min/max is the first line of defense; empty/NaN guarded on the frontend before invoke).
- `bin` empty/whitespace/null → key removed (→ adapter default); args trimmed, blanks dropped.
- `app_data_dir()` unresolvable → `save_config` `Err`, writes nothing.
- Concurrent saves / a hand-edit racing a save: **last-writer-wins** (no lock across read-modify-write) — acceptable for a single-user settings file; noted, not fixed.

## Testing
- **Pure `merge_edits` (`services/config.rs`)** — assert on the **parsed `Value`** of the output (not a pretty-string compare): preserves a pre-existing `terminal.theme` **and** an unknown top-level key **and** a **non-PTY agent id** **and** an **unknown key inside a PTY agent object**, while changing `fontSize` (theme's sibling under `terminal`) + a PTY agent's `bin`/`args`; a `claude-agent-sdk` (or unknown) id in the payload is **not written** (write-side whitelist); nested non-object (`{"terminal":5}`, `{"agents":[]}`, `{"agents":{"codex":5}}`) → merges without panic; empty/whitespace/null `bin` → key removed; args trimmed + blanks dropped + an arg-with-spaces stays one element; `fontSize` `5`/`73`→`Err`, `6`/`72`→`Ok`; non-object top-level (`"[1,2,3]"`, `"42"`, `"{ not"`) → `Err`.
- **`write_config_atomic` + `read_config_raw` (tempfile tests):** valid raw+theme → file has merged content + theme; **invalid raw → `Err` AND the file is byte-for-byte unchanged** (the crown-jewel no-clobber assertion); absent/blank → creates; a successful write leaves **no `.tmp`** behind; symlink/oversize raw → `Err`.
- **`edit_view` (pure):** projects all 3 ids (present→file values, absent→`{bin:null,args:[]}`), out-of-range file `fontSize` → default.
- **e2e** (`agent-terminal.e2e.ts` / new `settings.e2e.ts`): pre-seed `UAW_CONFIG_PATH` with a `theme` + an unknown key; open **Settings**; set `claude-code` args to a **multi-line** value (a flag, a blank line, an arg-with-spaces) + a new `fontSize`; **Save** (click via `browser.execute` — a lingering toast can intercept the native click); then (a) read the file: it has the new args + fontSize **and still has the theme + unknown key** (merge preserved); (b) open a terminal, assert argv shows the arg **boundaries** — for this the fake agent's echo must be boundary-preserving: change `scripts/run-e2e.sh` to `( IFS='|'; printf 'ARGV:[%s]\n' "$*" )` (Slice ①'s `.includes("--uaw-e2e")` checks still pass), assert the space-arg is one element + the blank line was dropped; (c) seed a non-default `theme.background`, open a terminal, assert the host element's **computed background** matches (plain-DOM, no canvas). Live-apply of `fontSize` to an already-open terminal is not e2e'd (canvas renderer; the remount-read path is exercised by (b)/(c) reopening). Toast-selector gotcha: don't mix `[attr]` CSS with wdio `*=` text.

## Security
- `save_config` writes only to `config_file_path` (`UAW_CONFIG_PATH`/`app_data_dir`, never CWD/repo-relative; `Err` if unresolvable) — same provenance as the read path.
- **SDK key-exfil stays closed on the write side:** `merge_edits` whitelists `PTY_AGENT_IDS`, so a crafted `save_config` payload cannot write `agents["claude-agent-sdk"]`; the form only offers the 3 PTY ids; an SDK key merely *preserved* from a hand-edited raw file is inert (read `parse` drops it + the SDK spawn path never reads config).
- **Symlink-safe atomic write:** `create_new` (O_EXCL) + unique temp name prevents a pre-planted-symlink clobber and concurrent-name races; `read_config_raw` refuses symlink/non-regular so a linked `config.json` can't be read-through and materialized.
- Refuse-on-invalid-raw prevents clobbering a recoverable file (and the nested-object guard removes the panic that would have bypassed it).
- Merge is `serde_json::Value`-level (no shell/eval); `bin`/`args` exec-not-shell semantics unchanged from Slice ①. No secrets in the DTOs (theme = colours; error strings dataless).

## Out of scope
- Editing the theme palette in-app (hand-edit only — the confirmed decision).
- In-place live-apply while Settings is open (the shell unmounts terminals; would need a modal/overlay).
- Locking the read-modify-write (single-user, last-writer-wins accepted).
