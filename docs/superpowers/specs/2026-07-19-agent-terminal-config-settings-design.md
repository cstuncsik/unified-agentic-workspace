# Agent & Terminal Config — Settings Page (Slice ②) — Design

**Goal:** An in-app **Settings** page to edit the config Slice ① introduced — per-PTY-agent `bin`/`args` and terminal `fontSize` — with **merge-on-save** that preserves the hand-edited `theme` + any unknown keys, and **live-apply** of `fontSize`/`theme` to already-open terminals.

**Status:** Slice ② of the config feature. Slice ① (the JSON file + backend/frontend read path) shipped in v0.1.7. This builds the write path + UI.

**Decisions (confirmed in brainstorming):**
- The page edits **per-agent `bin`/`args` + `fontSize` only**. The **theme palette stays hand-edit-only** in `config.json` — matching kommand0 (its settings page omits `theme_colors`).
- **Save live-applies** `fontSize`/`theme` to open terminals. (`bin`/`args` apply to the *next* spawn — a running PTY can't swap its program; inherent, not a gap.)

---

## Architecture

- **Nav:** enable the currently-disabled `Settings` nav entry (`App.vue`: remove `"Settings"` from `plannedSections`, add an `activeView === 'settings'` button mirroring the others) → render a new **`src/components/SettingsView.vue`**. The page is **workspace-independent** — config is app-global (no `workspaces.currentId` scoping, unlike the other views).
- **Backend** (`commands/config.rs` + a pure `services/config.rs` helper): two commands — `get_config_for_edit` (read the current editable values) and `save_config` (validated, merge-on-save, atomic write). The merge logic is a pure function (`merge_edits`), the unit-test seam.
- **Frontend store:** `useAppConfigStore` gains `save(edits)` + a `getConfigForEdit()` path; its `terminal` ref (already reactive) is **watched** by `TerminalTab` for live-apply.
- **Live-apply:** `TerminalTab` watches `appConfig.terminal` and updates `term.options.fontSize`/`term.options.theme` (fontSize change → `doFit()` to recompute rows/cols + resize the PTY). The host element's background tracks `theme.background` (cosmetic fix — no `#000` frame around a custom bg).

## Data shape (frontend ⇄ backend)

```ts
// get_config_for_edit → EditConfig ; save_config(EditConfig) → SaveResult
interface AgentEdit { bin: string | null; args: string[] }
interface EditConfig {
  agents: { "claude-code": AgentEdit; codex: AgentEdit; gemini: AgentEdit };
  fontSize: number;
}
interface SaveResult { terminal: { fontSize: number; theme: ITheme }; ok: boolean; error: string | null }
```
Rust mirrors with `#[serde(rename_all = "camelCase")]` (same one-off already used for `TerminalOut`, matching the frontend/xterm camelCase).

## Backend

### `get_config_for_edit() -> EditConfig`
Reads the config via the existing `read_config_at`/`config_path` and **projects the editable fields**: for each id in `PTY_AGENT_IDS`, `agents[id]` = the file's `{bin, args}` or `{bin:null, args:[]}` if absent; `fontSize` = the parsed value (default `13`). It shows the *effective* value (an out-of-range file `fontSize` shows as the default) — the form only ever holds valid values. The theme is **not** returned (not editable here).

### `save_config(edits: EditConfig) -> SaveResult`
Command-boundary steps:
1. Resolve the path (`config_path`, as in `load`). Read the **raw** file contents — absent → treat as `"{}"`.
2. Call pure `merge_edits(raw, &edits)`:
   - Parse `raw` as `serde_json::Value`. **If it isn't a valid JSON object → `Err` (do NOT write)** — never clobber a hand-broken file the user can still recover; `save_config` returns `{ ok:false, error:"config.json isn't valid JSON — fix it by hand before saving from Settings." }`.
   - **Validate** `edits.font_size` ∈ `6..=72` → else `Err("Font size must be 6–72.")` (immediate feedback — closes Slice ①'s silent-default-at-read gap).
   - Merge into the Value, **preserving `terminal.theme` and every unknown key**: set `terminal.fontSize`; for each agent id, set/replace `agents[id].bin` (trim; **empty → remove the `bin` key** so it falls back to the adapter default) and `agents[id].args` (trim each, drop empties; empty list → set `[]`). Only touch these keys — `theme`, other agents' unknown keys, and any top-level unknown keys pass through untouched.
   - Return the merged pretty-printed JSON string.
3. **Atomic write:** write to `<config>.tmp` in the same dir, then `std::fs::rename` over `config.json` (atomic on POSIX; a crash mid-write can't leave a partial file).
4. Re-read via `read_config_at` and return `{ terminal: {fontSize, theme}, ok:true, error:null }` so the store can live-apply the fresh merged terminal config.

Pure `merge_edits(raw: &str, edits: &EditConfig) -> Result<String, String>` is the seam (no fs, no AppHandle).

## Frontend

- **`src/api/appConfig.ts`:** add `getConfigForEdit()` + `saveConfig(edits)` wrappers.
- **`useAppConfigStore`:** add `save(edits)` — calls `saveConfig`; on `ok`, set `terminal.value = result.terminal` (this drives live-apply via the watcher) and return `{ ok:true }`; on `!ok`, return `{ ok:false, error }` (store `terminal` untouched). Add `getForEdit()` passthrough. `terminal` stays a `ref` (reactive).
- **`SettingsView.vue`:** on mount, `getConfigForEdit()` → local form refs (3 agents × `{bin, args-as-newline-text}`, `fontSize`). A `<form @submit.prevent="save">` with `re-input` for each `bin`, a `<textarea class="re-input">` for each `args` (one arg per line — an arg with spaces survives), a numeric `re-input` (min 6 max 72) for `fontSize`, a `re-button data-variant="brand"` Save. On save: build `EditConfig` (split args textarea by newline, trim, drop empties), call `appConfig.save(edits)`; `ok` → `toast.success("Settings saved.")`; `!ok` → `toast.error(error)` (+ inline message). A muted footnote: "Terminal colours: edit `theme` in config.json." `data-testid`s for e2e.
- **`TerminalTab.vue` — live-apply:** add `watch(() => appConfig.terminal, (t) => { if (!term) return; term.options.fontSize = t.fontSize; term.options.theme = t.theme; doFit(); })` (xterm's `options` setters apply live; the `fontSize` change needs `doFit()` to recompute rows/cols + push the new PTY size). Keep the existing `active`-prop re-show repaint + ResizeObserver. **Cosmetic:** bind the host background to the theme (`:style="{ background: appConfig.terminal.theme.background }"`) so a non-black `theme.background` isn't framed in the `.terminal { background:#000 }` — the padding then reads as the themed bg, not a black border.

## Error handling / validation
- Invalid raw file → refuse to save, surface the fix-by-hand message (don't clobber).
- fontSize out of `6..=72` → refuse with a clear message (front + back; the numeric input's min/max is the first line of defense).
- Trimming: `bin` empty → key removed (→ adapter default); each arg trimmed, blank lines dropped.
- `save` never partially writes (atomic rename).

## Testing
- **Pure `merge_edits` unit tests (`services/config.rs`):** preserves a pre-existing `theme` + an unknown top-level key across a save; sets `fontSize` + agent `bin`/`args`; empty `bin` removes the key; absent raw (`"{}"`) creates a valid file; invalid raw → `Err` (no write); fontSize out-of-range → `Err`; args trimmed/blank-dropped.
- **`get_config_for_edit`:** projects all three agent ids (present → file values; absent → `{bin:null,args:[]}`) + fontSize.
- **e2e (`agent-terminal.e2e.ts` or a new `settings.e2e.ts`):** pre-seed `UAW_CONFIG_PATH` with a `theme` + an unknown key; open **Settings**; set `claude-code` args to `--uaw-set` + a new fontSize; **Save**; then (a) read the config file and assert it has the new args + fontSize **and still has the theme + unknown key** (merge-on-save preserved them — the critical assertion); (b) open a terminal and assert `--uaw-set` reaches argv (reusing Slice ①'s `ARGV:[…]` fake-agent echo). *Live-apply of fontSize to an already-open terminal is verified manually* — xterm's canvas renderer makes a font-size DOM assertion unreliable (same rationale as Slice ①).
- No frontend unit tests (no vitest); e2e + the `data-testid`s cover the UI.

## Security
- `save_config` writes **only** to the resolved config path (`UAW_CONFIG_PATH`/`app_data_dir`, never CWD/repo-relative — same provenance as Slice ①). It's user-initiated, writes the user's own file, no elevation.
- Merge is `serde_json::Value`-level; no `eval`/shell. `bin`/`args` semantics (arbitrary program by the user's own config) are unchanged from Slice ① — still the intended feature, still exec-not-shell at spawn, still SDK-excluded (the form only offers the three PTY ids).
- Atomic write avoids a truncated config (a corrupt file would fail-safe to defaults on next read, but the atomic write prevents even that).
- No secrets touched; `get_config_for_edit`/`save_config` carry only bin/args/fontSize.

## Out of scope
- Editing the theme palette in-app (stays hand-edit — the confirmed decision).
- Live-applying `bin`/`args` to a running PTY (impossible — next spawn).
- A general preferences framework — this is the config file's editor, nothing more.
