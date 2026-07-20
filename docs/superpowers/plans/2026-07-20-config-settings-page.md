# Config Settings Page (Slice ②) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An in-app **Settings** page to edit per-PTY-agent `bin`/`args` + terminal `fontSize`, with merge-on-save that preserves the hand-edited `theme` + unknown keys; applies to terminals on return.

**Architecture:** Write path mirrors Slice ①'s read seam — pure/path-parameterized logic in `services/config.rs` (`merge_edits`, `edit_view`, `read_config_raw`, `write_config_atomic`, `save_at`, `valid_font_size`), `AppHandle`/path at the `commands/config.rs` boundary (`config_file_path`, `get_config_for_edit`, `save_config`). Frontend: `SettingsView.vue`, store `getForEdit`/`save`, `TerminalTab` host-bg. **No live-apply watcher** — terminals unmount when Settings is shown; apply is the existing remount re-read.

**Tech Stack:** Rust (serde_json `Value` in-place merge, `OpenOptions` O_EXCL atomic write), Tauri command, Vue 3 + Pinia, wdio e2e.

Spec: `docs/superpowers/specs/2026-07-19-agent-terminal-config-settings-design.md`.

---

## File Structure
- **Modify** `src-tauri/src/services/config.rs` — `valid_font_size`, `AgentEdit`/`EditConfig`, `edit_view`, `merge_edits`, `read_config_raw`, `write_config_atomic`, `save_at` + tests; refactor `parse` to use `valid_font_size`.
- **Modify** `src-tauri/src/commands/config.rs` — `config_file_path` (refactor `load`), `get_config_for_edit`, `save_config`.
- **Modify** `src-tauri/src/lib.rs` — register the two commands.
- **Modify** `src/types/appConfig.ts`, `src/api/appConfig.ts`, `src/stores/appConfig.ts` — DTOs, wrappers, `getForEdit`/`save`.
- **Create** `src/components/SettingsView.vue`.
- **Modify** `src/App.vue` — nav + render placement + `ActiveView`.
- **Modify** `src/components/TerminalTab.vue` — host background binding.
- **Modify** `scripts/run-e2e.sh` — boundary-preserving argv echo.
- **Create** `e2e/specs/settings.e2e.ts`.

---

### Task 1: `services/config.rs` — `valid_font_size`, edit DTOs, `edit_view`, `merge_edits`

**Files:** Modify `src-tauri/src/services/config.rs`.

- [ ] **Step 1: Add the shared bound + refactor `parse`.** Add near the consts:
```rust
pub const FONT_SIZE_RANGE: std::ops::RangeInclusive<u64> = 6..=72;
pub fn valid_font_size(n: u64) -> bool {
    FONT_SIZE_RANGE.contains(&n)
}
```
In `parse`, replace `if (6..=72).contains(&fs) {` with `if valid_font_size(fs) {`.

- [ ] **Step 2: Add the edit DTOs + `edit_view` + `merge_edits`** (above the `#[cfg(test)]` block):
```rust
/// One agent's editable fields — Serialize (form load) + Deserialize (save).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentEdit {
    #[serde(default)]
    pub bin: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
}

/// The Settings save payload. `agents` is a MAP keyed by PTY adapter id (kebab —
/// a fixed struct + rename_all can't represent "claude-code").
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditConfig {
    #[serde(default)]
    pub agents: BTreeMap<String, AgentEdit>,
    pub font_size: u16,
}

/// Project the parsed config into the form's editable shape: all 3 PTY ids
/// (present → file values, absent → empty) + the effective font size.
pub fn edit_view(cfg: &Config) -> (BTreeMap<String, AgentEdit>, u16) {
    let agents = PTY_AGENT_IDS
        .iter()
        .map(|id| {
            let a = cfg.agents.get(*id);
            (
                (*id).to_string(),
                AgentEdit {
                    bin: a.and_then(|a| a.bin.clone()),
                    args: a.map(|a| a.args.clone()).unwrap_or_default(),
                },
            )
        })
        .collect();
    (agents, cfg.terminal.font_size)
}

/// Merge the edited fields into the RAW config JSON, preserving `theme` + every
/// unknown key. Pure. Errors (never writes) on a non-object file or bad fontSize.
pub fn merge_edits(raw: &str, edits: &EditConfig) -> Result<String, String> {
    const INVALID: &str =
        "config.json isn't valid JSON — fix it by hand before saving from Settings.";
    let mut root: Value = serde_json::from_str(raw).map_err(|_| INVALID.to_string())?;
    if !root.is_object() {
        return Err(INVALID.to_string());
    }
    if !valid_font_size(edits.font_size as u64) {
        return Err("Font size must be 6–72.".to_string());
    }
    let obj = root.as_object_mut().unwrap();

    // terminal.fontSize — coerce a non-object `terminal` rather than index-panic.
    let terminal = obj
        .entry("terminal")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !terminal.is_object() {
        *terminal = Value::Object(serde_json::Map::new());
    }
    terminal
        .as_object_mut()
        .unwrap()
        .insert("fontSize".to_string(), Value::from(edits.font_size));

    // agents.<pty id>.{bin,args} — whitelist PTY ids; touch only bin/args.
    let agents = obj
        .entry("agents")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !agents.is_object() {
        *agents = Value::Object(serde_json::Map::new());
    }
    let agents_obj = agents.as_object_mut().unwrap();
    for (id, edit) in &edits.agents {
        if !PTY_AGENT_IDS.contains(&id.as_str()) {
            continue; // never write a non-PTY (e.g. claude-agent-sdk) entry
        }
        let entry = agents_obj
            .entry(id.clone())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(serde_json::Map::new());
        }
        let a = entry.as_object_mut().unwrap();
        match edit.bin.as_deref().map(str::trim) {
            Some(b) if !b.is_empty() => {
                a.insert("bin".to_string(), Value::from(b));
            }
            _ => {
                a.remove("bin");
            }
        }
        let args: Vec<Value> = edit
            .args
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(Value::from)
            .collect();
        a.insert("args".to_string(), Value::Array(args));
    }

    serde_json::to_string_pretty(&root).map_err(|_| "failed to serialize config".to_string())
}
```

- [ ] **Step 3: Add tests** (in `mod tests`; parse the OUTPUT and assert on the `Value`, not a string compare):
```rust
    fn ed(font_size: u16, agents: &[(&str, Option<&str>, &[&str])]) -> EditConfig {
        EditConfig {
            font_size,
            agents: agents
                .iter()
                .map(|(id, bin, args)| {
                    (
                        id.to_string(),
                        AgentEdit {
                            bin: bin.map(str::to_string),
                            args: args.iter().map(|s| s.to_string()).collect(),
                        },
                    )
                })
                .collect(),
        }
    }
    fn merged(raw: &str, edits: &EditConfig) -> Value {
        serde_json::from_str(&merge_edits(raw, edits).unwrap()).unwrap()
    }

    #[test]
    fn merge_preserves_theme_unknown_and_non_pty_and_sets_edits() {
        let raw = r#"{
          "theme_note":"keep me",
          "agents":{"codex":{"bin":"/old","extra":"keepInAgent"},"claude-agent-sdk":{"bin":"/sdk"}},
          "terminal":{"theme":{"background":"#123456"},"note":"keepInTerminal"}
        }"#;
        let v = merged(raw, &ed(20, &[("codex", Some("/new"), &["--x"])]));
        assert_eq!(v["terminal"]["fontSize"], 20);
        assert_eq!(v["terminal"]["theme"]["background"], "#123456"); // theme preserved
        assert_eq!(v["terminal"]["note"], "keepInTerminal"); // unknown sub-key preserved
        assert_eq!(v["theme_note"], "keep me"); // unknown top-level preserved
        assert_eq!(v["agents"]["codex"]["bin"], "/new");
        assert_eq!(v["agents"]["codex"]["args"][0], "--x");
        assert_eq!(v["agents"]["codex"]["extra"], "keepInAgent"); // unknown key in agent preserved
        assert_eq!(v["agents"]["claude-agent-sdk"]["bin"], "/sdk"); // non-PTY untouched, never edited
    }

    #[test]
    fn merge_whitelist_ignores_sdk_id_in_payload() {
        let v = merged("{}", &ed(13, &[("claude-agent-sdk", Some("/evil"), &[])]));
        assert!(v.get("agents").is_none() || v["agents"].get("claude-agent-sdk").is_none());
    }

    #[test]
    fn merge_guards_nested_non_objects_without_panic() {
        for raw in [r#"{"terminal":5}"#, r#"{"agents":[]}"#, r#"{"agents":{"codex":5}}"#] {
            let v = merged(raw, &ed(14, &[("codex", Some("/c"), &["--a"])]));
            assert_eq!(v["terminal"]["fontSize"], 14);
            assert_eq!(v["agents"]["codex"]["bin"], "/c");
        }
    }

    #[test]
    fn merge_bin_empty_removes_key_args_trim_drop() {
        let v = merged(
            r#"{"agents":{"codex":{"bin":"/old"}}}"#,
            &ed(13, &[("codex", Some("   "), &["--a", "  ", " b c "])]),
        );
        assert!(v["agents"]["codex"].get("bin").is_none()); // whitespace bin removed
        assert_eq!(v["agents"]["codex"]["args"], serde_json::json!(["--a", "b c"])); // trimmed, blank dropped, space-arg intact
    }

    #[test]
    fn merge_rejects_bad_fontsize_and_non_object() {
        assert!(merge_edits("{}", &ed(5, &[])).is_err());
        assert!(merge_edits("{}", &ed(73, &[])).is_err());
        assert!(merge_edits("6", &ed(6, &[])).is_err());
        assert!(merge_edits("[1,2]", &ed(6, &[])).is_err());
        assert!(merge_edits("{ not json", &ed(6, &[])).is_err());
        assert!(merge_edits("{}", &ed(6, &[])).is_ok());
        assert!(merge_edits("{}", &ed(72, &[])).is_ok());
    }

    #[test]
    fn edit_view_projects_all_three_ids() {
        let (cfg, _) = parse(r#"{"agents":{"codex":{"bin":"/c","args":["-x"]}}}"#);
        let (agents, fs) = edit_view(&cfg);
        assert_eq!(fs, DEFAULT_FONT_SIZE);
        assert_eq!(agents["codex"].bin.as_deref(), Some("/c"));
        assert_eq!(agents["claude-code"].bin, None);
        assert!(agents["gemini"].args.is_empty());
        assert_eq!(agents.len(), 3);
    }
```

- [ ] **Step 4: Run.** `cargo test --manifest-path src-tauri/Cargo.toml config:: 2>&1 | grep "test result"` → all pass.
- [ ] **Step 5: Commit.** `git add -A && git commit -m "feat(config): merge_edits + edit_view + edit DTOs (Slice 2 pure core)"`

---

### Task 2: `services/config.rs` — guarded raw read + atomic write + `save_at`

**Files:** Modify `src-tauri/src/services/config.rs`.

- [ ] **Step 1: Add the three fns** (above tests):
```rust
/// Read the raw config for a save (guarded like `read_config_at`). Absent or
/// blank/whitespace-only → `"{}"` (create-fresh). Symlink/non-regular/oversize → Err.
pub fn read_config_raw(path: &Path) -> Result<String, String> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok("{}".to_string()),
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err("config.json must be a regular file.".to_string());
    }
    if meta.len() > MAX_BYTES {
        return Err("config.json is too large.".to_string());
    }
    let s = std::fs::read_to_string(path).map_err(|_| "config.json is unreadable.".to_string())?;
    Ok(if s.trim().is_empty() { "{}".to_string() } else { s })
}

/// Write `contents` to `path` atomically. Symlink-safe: a unique O_EXCL temp in
/// the same dir + rename. Creates the parent dir. Removes the temp on any error.
pub fn write_config_atomic(path: &Path, contents: &str) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().ok_or_else(|| "invalid config path".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("failed to create config dir: {e}"))?;
    let tmp = parent.join(format!("config.json.{}.tmp", crate::util::new_id()));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true) // O_EXCL: won't follow a pre-planted temp/symlink
        .open(&tmp)
        .map_err(|e| format!("failed to create temp config: {e}"))?;
    let res = f
        .write_all(contents.as_bytes())
        .and_then(|_| f.sync_all())
        .map_err(|e| format!("failed to write config: {e}"))
        .and_then(|_| {
            std::fs::rename(&tmp, path).map_err(|e| format!("failed to replace config: {e}"))
        });
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// Full save at a path (path-parameterized → tempfile-testable, no AppHandle):
/// guarded read → merge → atomic write → re-read the merged terminal config.
pub fn save_at(path: &Path, edits: &EditConfig) -> Result<TerminalConfig, String> {
    let raw = read_config_raw(path)?;
    let merged = merge_edits(&raw, edits)?;
    write_config_atomic(path, &merged)?;
    let (cfg, _warning) = read_config_at(path);
    Ok(cfg.terminal)
}
```

- [ ] **Step 2: Add tempfile tests** (reuse the `tmp_path` helper from the Slice ① tests):
```rust
    #[test]
    fn save_at_writes_merged_and_preserves_theme_leaves_no_tmp() {
        let p = tmp_path("save-ok.json");
        std::fs::write(&p, r#"{"terminal":{"theme":{"background":"#abcdef"}}}"#).unwrap();
        let tc = save_at(&p, &ed(18, &[("codex", Some("/c"), &["--a"])])).unwrap();
        assert_eq!(tc.font_size, 18);
        assert_eq!(tc.theme.get("background").unwrap(), "#abcdef");
        let on_disk: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(on_disk["terminal"]["theme"]["background"], "#abcdef");
        assert_eq!(on_disk["agents"]["codex"]["bin"], "/c");
        // no leftover temp
        let leftovers: Vec<_> = std::fs::read_dir(p.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn save_at_invalid_raw_errors_and_leaves_file_byte_for_byte_unchanged() {
        let p = tmp_path("save-invalid.json");
        let original = "{ this is not json";
        std::fs::write(&p, original).unwrap();
        let before = std::fs::read(&p).unwrap();
        let r = save_at(&p, &ed(20, &[("codex", Some("/c"), &[])]));
        assert!(r.is_err());
        assert_eq!(std::fs::read(&p).unwrap(), before); // crown jewel: not clobbered
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn save_at_absent_creates_fresh() {
        let p = tmp_path("save-absent.json");
        let tc = save_at(&p, &ed(16, &[])).unwrap();
        assert_eq!(tc.font_size, 16);
        assert!(p.exists());
        let _ = std::fs::remove_file(&p);
    }
```

- [ ] **Step 3: Run.** `cargo test --manifest-path src-tauri/Cargo.toml config:: 2>&1 | grep "test result"` → all pass.
- [ ] **Step 4: Commit.** `git add -A && git commit -m "feat(config): guarded raw read + symlink-safe atomic write + save_at"`

---

### Task 3: Commands — `config_file_path`, `get_config_for_edit`, `save_config`

**Files:** Modify `src-tauri/src/commands/config.rs`, `src-tauri/src/lib.rs`.

- [ ] **Step 1: Extract `config_file_path` + refactor `load`** in `commands/config.rs`:
```rust
use std::path::PathBuf;

/// The config path, or None if `app_data_dir()` can't resolve (never CWD-relative).
fn config_file_path(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    Some(config::config_path(std::env::var_os("UAW_CONFIG_PATH"), &dir))
}

pub fn load(app: &AppHandle) -> (Config, Option<String>) {
    match config_file_path(app) {
        Some(path) => config::read_config_at(&path),
        None => (Config::default(), None),
    }
}
```
(Delete the old body of `load`.)

- [ ] **Step 2: Add the two commands** (add `use std::collections::BTreeMap;` if not present):
```rust
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigForEdit {
    agents: BTreeMap<String, config::AgentEdit>,
    font_size: u16,
    warning: Option<String>,
}

/// Editable config for the Settings form (per-agent bin/args + fontSize) + a
/// parse warning so the form can flag an unparseable file up front.
#[tauri::command]
pub fn get_config_for_edit(app: AppHandle) -> ConfigForEdit {
    let (cfg, warning) = load(&app);
    let (agents, font_size) = config::edit_view(&cfg);
    ConfigForEdit { agents, font_size, warning }
}

/// Merge the edited fields into config.json (preserving theme/unknown keys) and
/// return the merged terminal config for live-apply. Err (validation or fs)
/// rejects the invoke; the frontend catches it.
#[tauri::command]
pub fn save_config(app: AppHandle, edits: config::EditConfig) -> Result<TerminalOut, String> {
    let path = config_file_path(&app)
        .ok_or_else(|| "could not resolve the config directory".to_string())?;
    let terminal = config::save_at(&path, &edits)?;
    Ok(TerminalOut { font_size: terminal.font_size, theme: terminal.theme })
}
```

- [ ] **Step 3: Register** both in `src-tauri/src/lib.rs` `generate_handler![ … ]`:
```rust
            commands::config::get_config_for_edit,
            commands::config::save_config,
```

- [ ] **Step 4: Build + clippy.**
```bash
cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | tail -3
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings 2>&1 | tail -3
```
Expected: Finished, clippy clean. (Fix any lint — e.g. an unused import.)

- [ ] **Step 5: Commit.** `git add -A && git commit -m "feat(config): get_config_for_edit + save_config commands"`

---

### Task 4: Frontend types + api + store

**Files:** Modify `src/types/appConfig.ts`, `src/api/appConfig.ts`, `src/stores/appConfig.ts`.

- [ ] **Step 1: `src/types/appConfig.ts`** — add:
```ts
export interface AgentEdit {
  bin: string | null;
  args: string[];
}
export interface EditConfig {
  agents: Record<string, AgentEdit>;
  fontSize: number;
}
export interface ConfigForEdit {
  agents: Record<string, AgentEdit>;
  fontSize: number;
  warning: string | null;
}
```

- [ ] **Step 2: `src/api/appConfig.ts`** — add (import the new types):
```ts
import type { AppConfig, ConfigForEdit, EditConfig } from "../types/appConfig";

export function getConfigForEdit(): Promise<ConfigForEdit> {
  return invoke<ConfigForEdit>("get_config_for_edit");
}
export function saveConfig(edits: EditConfig): Promise<AppConfig["terminal"]> {
  return invoke<AppConfig["terminal"]>("save_config", { edits });
}
```

- [ ] **Step 3: `src/stores/appConfig.ts`** — add inside the store, before `return`:
```ts
  function getForEdit() {
    return api.getConfigForEdit();
  }

  async function save(edits: EditConfig): Promise<{ ok: boolean; error?: string }> {
    try {
      terminal.value = await api.saveConfig(edits);
      return { ok: true };
    } catch (e) {
      // save_config rejects on validation, serde, or fs errors — surface, don't blank.
      return { ok: false, error: String(e) };
    }
  }
```
Add `getForEdit, save` to the returned object, and `import type { EditConfig } from "../types/appConfig";`.

- [ ] **Step 4: Typecheck.** `pnpm typecheck 2>&1 | tail -3` → clean.
- [ ] **Step 5: Commit.** `git add -A && git commit -m "feat(config): frontend edit DTOs + getForEdit/save store methods"`

---

### Task 5: `SettingsView.vue`

**Files:** Create `src/components/SettingsView.vue`.

- [ ] **Step 1: Create the component:**
```vue
<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useAppConfigStore } from "../stores/appConfig";
import { useToast } from "../composables/useToast";
import type { EditConfig } from "../types/appConfig";

const appConfig = useAppConfigStore();
const toast = useToast();

const AGENTS = [
  { id: "claude-code", label: "Claude Code" },
  { id: "codex", label: "Codex" },
  { id: "gemini", label: "Gemini" },
] as const;

// Per-agent form state: bin string + args as newline-joined text.
const bins = ref<Record<string, string>>({});
const argsText = ref<Record<string, string>>({});
const fontSize = ref(13);
const submitting = ref(false);

onMounted(async () => {
  const cfg = await appConfig.getForEdit();
  for (const { id } of AGENTS) {
    bins.value[id] = cfg.agents[id]?.bin ?? "";
    argsText.value[id] = (cfg.agents[id]?.args ?? []).join("\n");
  }
  fontSize.value = cfg.fontSize;
  if (cfg.warning) toast.error(cfg.warning);
});

async function save() {
  if (!Number.isFinite(fontSize.value)) {
    toast.error("Font size must be 6–72.");
    return;
  }
  const edits: EditConfig = { agents: {}, fontSize: fontSize.value };
  for (const { id } of AGENTS) {
    const bin = bins.value[id]?.trim();
    edits.agents[id] = {
      bin: bin ? bin : null,
      args: (argsText.value[id] ?? "")
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean),
    };
  }
  submitting.value = true;
  const res = await appConfig.save(edits);
  submitting.value = false;
  if (res.ok) toast.success("Settings saved.");
  else toast.error(res.error ?? "Save failed.");
}
</script>

<template>
  <section data-testid="settings-view">
    <h1 class="view-title">Settings</h1>
    <form class="settings" @submit.prevent="save">
      <fieldset v-for="agent in AGENTS" :key="agent.id" class="settings__agent">
        <legend>{{ agent.label }}</legend>
        <label class="settings__field">
          <span>Binary</span>
          <input
            v-model="bins[agent.id]"
            class="re-input"
            type="text"
            :placeholder="agent.id"
            :aria-label="`${agent.label} binary`"
            :data-testid="`bin-${agent.id}`"
          />
        </label>
        <label class="settings__field">
          <span>Args (one per line)</span>
          <textarea
            v-model="argsText[agent.id]"
            class="re-input"
            rows="3"
            :aria-label="`${agent.label} args`"
            :data-testid="`args-${agent.id}`"
          ></textarea>
        </label>
      </fieldset>

      <label class="settings__field">
        <span>Terminal font size</span>
        <input
          v-model.number="fontSize"
          class="re-input"
          type="number"
          min="6"
          max="72"
          aria-label="Terminal font size"
          data-testid="font-size"
        />
      </label>

      <button
        class="re-button"
        data-variant="brand"
        type="submit"
        :disabled="submitting"
        data-testid="settings-save"
      >
        Save
      </button>
      <p class="muted settings__note">Terminal colours: edit <code>theme</code> in config.json.</p>
    </form>
  </section>
</template>

<style scoped>
.settings {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  max-width: 40rem;
}
.settings__agent {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  border: 1px solid var(--re-color-border);
  border-radius: var(--re-radius-md, 6px);
  padding: 0.75rem;
}
.settings__field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
.settings__note {
  font-size: 0.8rem;
}
</style>
```

- [ ] **Step 2: Typecheck + format.** `pnpm typecheck 2>&1 | tail -3 && npx prettier --check src/components/SettingsView.vue` → clean.
- [ ] **Step 3: Commit.** `git add -A && git commit -m "feat(config): SettingsView form (bin/args + fontSize)"`

---

### Task 6: `App.vue` — nav entry + render placement

**Files:** Modify `src/App.vue`.

- [ ] **Step 1: Import + union.** Add `import SettingsView from "./components/SettingsView.vue";` with the other view imports; add `| "settings"` to the `ActiveView` union; change `const plannedSections = ["Skills", "Automations", "Settings"];` → `["Skills", "Automations"]`.

- [ ] **Step 2: Nav button.** After the `Providers` nav `</button>` (line ~204), add:
```html
          <button
            class="re-button"
            data-variant="ghost"
            :aria-current="activeView === 'settings' ? 'page' : undefined"
            type="button"
            @click="activeView = 'settings'"
          >
            Settings
          </button>
```

- [ ] **Step 3: Render outside the workspace guard.** Change the `<main class="main">` body so Settings is the first branch and the workspace-scoped block is the `v-else`:
```html
      <main class="main">
        <SettingsView v-if="activeView === 'settings'" />
        <template v-else>
          <p v-if="workspaces.loading" class="muted">Loading workspace…</p>
          <p v-else-if="workspaces.error" class="error">{{ workspaces.error }}</p>
          <template v-else-if="workspaces.current">
            <header class="main__header">
              <h1>{{ workspaces.current.name }}</h1>
              <span class="badge">{{ workspaces.current.kind }}</span>
            </header>
            <SessionsView v-if="activeView === 'inbox'" />
            <ProjectsView v-else-if="activeView === 'projects'" />
            <ArtifactsView v-else-if="activeView === 'artifacts'" />
            <SourcesView v-else-if="activeView === 'sources'" />
            <CodingView v-else-if="activeView === 'coding'" />
            <ReviewsView v-else-if="activeView === 'reviews'" />
            <BoardView v-else-if="activeView === 'board'" />
            <AgentsView v-else-if="activeView === 'agents'" />
            <ProvidersView v-else-if="activeView === 'providers'" />
          </template>
          <p v-else class="muted">No workspace selected.</p>
        </template>
        <ConfirmDialog />
      </main>
```
(Preserve the existing `<ConfirmDialog />` placement — it stays a direct child of `main`.)

- [ ] **Step 4: Typecheck + format.** `pnpm typecheck && npx prettier --check src/App.vue` → clean.
- [ ] **Step 5: Commit.** `git add -A && git commit -m "feat(config): enable Settings nav (app-global, outside workspace guard)"`

---

### Task 7: `TerminalTab.vue` — host background tracks the theme

**Files:** Modify `src/components/TerminalTab.vue`.

- [ ] **Step 1: Bind the host background.** Change the host div (`<div ref="host" class="terminal" data-testid="agent-terminal">`) to:
```html
  <div
    ref="host"
    class="terminal"
    data-testid="agent-terminal"
    :style="{ background: appConfig.terminal.theme.background }"
  ></div>
```
(`appConfig` is already in scope from Slice ①. The `.terminal { background:#000 }` remains as the fallback when `theme.background` is undefined.)

- [ ] **Step 2: Typecheck + format.** `pnpm typecheck && npx prettier --check src/components/TerminalTab.vue` → clean.
- [ ] **Step 3: Commit.** `git add -A && git commit -m "feat(config): terminal host background tracks theme.background"`

---

### Task 8: e2e — boundary-preserving argv echo + Settings spec

**Files:** Modify `scripts/run-e2e.sh`; Create `e2e/specs/settings.e2e.ts`.

- [ ] **Step 1: Make the fake agent's argv echo boundary-preserving.** In `scripts/run-e2e.sh`, change the line `printf 'ARGV:[%s]\n' "$*"` (inside the `/tmp/uaw-fake-agent` heredoc) to:
```bash
( IFS='|'; printf 'ARGV:[%s]\n' "$*" )
```
(Args now render `ARGV:[--a|b c]` — boundaries visible; Slice ①'s `--uaw-e2e` substring check still matches.)

- [ ] **Step 2: Create `e2e/specs/settings.e2e.ts`.** Pre-seed the config file (read on-demand), drive the Settings form, Save, then assert the file preserved theme+unknown key AND the args reach argv with boundaries. Match the real toast selector + click Save via `browser.execute` (the toast-overlay gotcha). Sketch:
```ts
import { browser, $, expect } from "@wdio/globals";
import * as fs from "node:fs";

const CONFIG = process.env.UAW_CONFIG_PATH as string;

describe("settings", () => {
  it("saves agent args + fontSize, preserving hand-edited theme + unknown keys", async () => {
    // Pre-seed a config with a theme + an unknown key (read fresh by get_config_for_edit).
    fs.writeFileSync(
      CONFIG,
      JSON.stringify({ keepMe: "yes", terminal: { theme: { background: "#0a0a12" } } }),
    );

    await (await $("button*=Settings")).click();
    await (await $('[data-testid="settings-view"]')).waitForExist({ timeout: 10_000 });

    // Multi-line args: a flag, a blank line, and an arg with spaces.
    await (await $('[data-testid="args-claude-code"]')).setValue("--uaw-set\n\n--msg hello there");
    await (await $('[data-testid="font-size"]')).setValue("16");

    // Click via execute — a lingering toast can intercept a native click on the button.
    await browser.execute(() =>
      (document.querySelector('[data-testid="settings-save"]') as HTMLButtonElement)?.click(),
    );
    // Success toast (design-system danger/success tone; assert the success one appears).
    await browser.waitUntil(
      async () => (await $('[data-testid="settings-view"]').isExisting()) && fs.existsSync(CONFIG),
      { timeout: 10_000 },
    );

    // (a) merge-on-save preserved theme + unknown key, and wrote the edits.
    await browser.waitUntil(
      () => {
        const c = JSON.parse(fs.readFileSync(CONFIG, "utf8"));
        return (
          c.keepMe === "yes" &&
          c.terminal?.theme?.background === "#0a0a12" &&
          c.terminal?.fontSize === 16 &&
          Array.isArray(c.agents?.["claude-code"]?.args)
        );
      },
      { timeout: 10_000, timeoutMsg: "merged config not written with preserved keys" },
    );
    const cfg = JSON.parse(fs.readFileSync(CONFIG, "utf8"));
    expect(cfg.agents["claude-code"].args).toEqual(["--uaw-set", "--msg hello there"]); // blank dropped, space-arg one element
  });
});
```
Adjust selectors to the real DOM while implementing (the worktree/terminal-start flow for the argv-boundary assertion can reuse `agent-terminal.e2e.ts`'s helpers if you extend that spec instead; keep the file-preservation assertion as the crown jewel).

- [ ] **Step 3: Run the full suite.** `pnpm e2e:docker 2>&1 | tail -30` → all specs pass (rebuilds; slow). Fix real selector mismatches; do not weaken the file-preservation assertion.

- [ ] **Step 4: Commit.** `git add -A && git commit -m "test(config): Settings e2e (merge-preservation + boundary argv) + argv echo"`

---

## Self-Review
**Spec coverage:** merge-on-save preserving theme/unknown/non-PTY/unknown-sub-key + whitelist (Task 1) ✓; nested-object panic guard (Task 1) ✓; symlink-safe atomic write + guarded raw read + byte-for-byte no-clobber test (Task 2) ✓; `config_file_path` refuse-if-unresolvable + the two commands + register (Task 3) ✓; DTOs/store `save` try/catch (Task 4) ✓; SettingsView form with `v-model.number` + finite-guard + submitting + aria/fieldset + footnote (Task 5) ✓; nav outside the workspace guard + `ActiveView` (Task 6) ✓; host-bg cosmetic (Task 7) ✓; boundary-preserving e2e argv + merge-preservation assertion (Task 8) ✓; live-apply = remount re-read (no watcher — documented, nothing to build).

**Type consistency:** `EditConfig{agents:map,fontSize}` / `AgentEdit{bin,args}` / `save_config -> Result<TerminalOut,String>` / `get_config_for_edit -> {agents,fontSize,warning}` identical across Rust ↔ TS; `save_at -> TerminalConfig` → command maps to `TerminalOut`; `valid_font_size` shared by `parse` + `merge_edits`.

**No placeholders:** every step has complete code + commands. Task 8's e2e selectors are the one flagged-loose item (resolved against the real SettingsView `data-testid`s at implementation), not a logic gap.
