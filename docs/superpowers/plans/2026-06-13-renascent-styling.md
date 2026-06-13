# Renascent Styling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle the UAW app with `@relements/core`'s renascent theme — design-system tokens and component classes across all four surfaces, toast + confirm-dialog behaviors, and a tri-state (System/Light/Dark) theme toggle — without breaking the e2e gate.

**Architecture:** Import the design system's CSS globally and apply the renascent theme via a root-element class. Migrate incrementally: first alias the existing `--uaw-*` variables to `--re-*` tokens so the app immediately renders in renascent with zero markup changes, then convert each component's markup to `.re-*` classes, then remove the alias. Theme preference is client-side only (`localStorage`); no backend changes.

**Tech Stack:** Vue 3 + TypeScript, Pinia, Vite, `@relements/core` (framework-agnostic CSS + ESM behaviors), WebdriverIO e2e.

Spec: `docs/superpowers/specs/2026-06-13-renascent-styling-design.md`

---

## File structure

- Create: `src/theme.ts` — theme-mode types, `localStorage` read, root-class apply (no Vue deps; usable before mount).
- Create: `src/stores/ui.ts` — Pinia store wrapping `theme.ts` for reactive use in components.
- Create: `src/components/ThemeToggle.vue` — the sidebar-footer `.re-select` control.
- Create: `src/composables/useToast.ts` — thin wrapper over `@relements/core/behaviors/toast`.
- Create: `src/composables/useConfirm.ts` + `src/components/ConfirmDialog.vue` — shared `.re-dialog` confirm returning a `Promise<boolean>`.
- Create: `e2e/specs/theme.e2e.ts` — theme-toggle e2e test.
- Modify: `src/main.ts` (CSS imports + theme bootstrap), `src/App.vue` (token alias → removal, shell restyle, mount toast/confirm hosts + toggle), `src/components/WorkspaceSwitcher.vue`, `src/components/ProjectsView.vue`, `src/components/SessionsView.vue`.
- Modify (Task 8 only): `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/capabilities/default.json`, `package.json` (drop `@tauri-apps/plugin-dialog`).

**e2e hooks that MUST be preserved** (or the spec updated in lockstep): `aria-label`s (Select workspace, New workspace, New workspace name, New project name, Project mode, New session title, Session mode, Session project, Session status), button text "Projects"/"Inbox"/"Create", `title="Create"` on the workspace submit, status group heading "Done", `data-testid="project-row"`/`"session-row"`, and inner `.row__title`/`.row__project` elements.

---

## Task 1: Install the design system and bootstrap the renascent theme

Makes the whole app render in renascent immediately by aliasing the existing
`--uaw-*` variables to `--re-*` tokens — no markup changes, so the e2e gate
stays green while later tasks migrate components.

**Files:**
- Modify: `package.json` (add dependency)
- Create: `src/theme.ts`
- Modify: `src/main.ts`
- Modify: `src/App.vue` (the global `<style>` `:root` block, ~lines 64–95)

- [ ] **Step 1: Install the package**

Run: `pnpm add @relements/core`
Expected: `@relements/core` appears under `dependencies` in `package.json`.

- [ ] **Step 2: Create the theme module**

Create `src/theme.ts`:

```ts
export type ThemeMode = "system" | "light" | "dark";

const STORAGE_KEY = "uaw.theme";

// Exactly one of these classes is present on <html> at any time.
const THEME_CLASS: Record<ThemeMode, string> = {
  system: "theme-renascent",
  light: "theme-renascent-light",
  dark: "theme-renascent-dark",
};

export function getStoredTheme(): ThemeMode {
  const value = localStorage.getItem(STORAGE_KEY);
  return value === "light" || value === "dark" || value === "system" ? value : "system";
}

/** Apply a theme mode to the document root and persist the choice. */
export function applyTheme(mode: ThemeMode): void {
  const root = document.documentElement;
  root.classList.remove(...Object.values(THEME_CLASS));
  root.classList.add(THEME_CLASS[mode]);
  localStorage.setItem(STORAGE_KEY, mode);
}
```

- [ ] **Step 3: Import the CSS and bootstrap the theme before mount**

Replace the contents of `src/main.ts` with:

```ts
import { createApp } from "vue";
import { createPinia } from "pinia";
import "@relements/core/index.css";
import "@relements/core/themes/renascent.css";
import App from "./App.vue";
import { applyTheme, getStoredTheme } from "./theme";

// Apply the persisted theme before mount to avoid a flash of the wrong theme.
applyTheme(getStoredTheme());

createApp(App).use(createPinia()).mount("#app");
```

- [ ] **Step 4: Alias the old variables to renascent tokens**

In `src/App.vue`, in the global (unscoped) `<style>` block, replace the
`:root { ... }` declaration (the `--uaw-*` definitions and `color-scheme`)
with aliases so existing scoped styles render in renascent:

```css
:root {
  --uaw-bg: var(--re-color-bg);
  --uaw-surface: var(--re-color-surface);
  --uaw-surface-hover: var(--re-color-bg-muted);
  --uaw-border: var(--re-color-border);
  --uaw-text: var(--re-color-text);
  --uaw-muted: var(--re-color-text-muted);
}
```

Leave the rest of the global block (the `*`, `html, body, #app`, and `body`
rules) intact, but delete the `background`/`color` overrides on `body` if
present (the theme sets these) — keep only the font-family, margins, and box-sizing.

- [ ] **Step 5: Verify it builds and renders**

Run: `pnpm build`
Expected: PASS (vue-tsc + vite, no errors).

Run: `pnpm e2e:typecheck`
Expected: PASS.

- [ ] **Step 6: Verify the e2e gate still passes (markup unchanged)**

Run: `pnpm e2e:docker`
Expected: `5 passing`, exit 0.

- [ ] **Step 7: Commit**

```bash
git add package.json pnpm-lock.yaml src/theme.ts src/main.ts src/App.vue
git commit -m "feat(ui): adopt @relements/core renascent theme via token alias"
```

---

## Task 2: Theme toggle (tri-state) in the sidebar footer — test first

**Files:**
- Create: `e2e/specs/theme.e2e.ts`
- Create: `src/stores/ui.ts`
- Create: `src/components/ThemeToggle.vue`
- Modify: `src/App.vue` (render `ThemeToggle` in the sidebar footer)

- [ ] **Step 1: Write the failing e2e test**

Create `e2e/specs/theme.e2e.ts`:

```ts
import { browser, $, expect } from "@wdio/globals";

const rootClass = () => browser.execute(() => document.documentElement.className.trim());
const stored = () => browser.execute(() => localStorage.getItem("uaw.theme"));

describe("theme toggle", () => {
  before(async () => {
    await (await $("h1")).waitForExist({ timeout: 30_000 });
  });

  it("switches Light / Dark / System and persists the choice", async () => {
    const select = await $('[aria-label="Theme"]');

    await select.selectByAttribute("value", "light");
    await browser.waitUntil(async () => (await rootClass()) === "theme-renascent-light");
    expect(await stored()).toBe("light");

    await select.selectByAttribute("value", "dark");
    await browser.waitUntil(async () => (await rootClass()) === "theme-renascent-dark");
    expect(await stored()).toBe("dark");

    await select.selectByAttribute("value", "system");
    await browser.waitUntil(async () => (await rootClass()) === "theme-renascent");
    expect(await stored()).toBe("system");
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `pnpm e2e:docker`
Expected: the `theme toggle` spec FAILS (no element with `aria-label="Theme"`),
while the `smoke` spec still passes.

- [ ] **Step 3: Create the UI store**

Create `src/stores/ui.ts`:

```ts
import { ref } from "vue";
import { defineStore } from "pinia";
import { applyTheme, getStoredTheme, type ThemeMode } from "../theme";

export const useUiStore = defineStore("ui", () => {
  const theme = ref<ThemeMode>(getStoredTheme());

  function setTheme(mode: ThemeMode) {
    theme.value = mode;
    applyTheme(mode);
  }

  return { theme, setTheme };
});
```

- [ ] **Step 4: Create the ThemeToggle component**

Create `src/components/ThemeToggle.vue`:

```vue
<script setup lang="ts">
import { useUiStore } from "../stores/ui";
import type { ThemeMode } from "../theme";

const ui = useUiStore();
const modes: { value: ThemeMode; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

function onChange(event: Event) {
  ui.setTheme((event.target as HTMLSelectElement).value as ThemeMode);
}
</script>

<template>
  <label class="re-field theme-toggle">
    <span class="re-field__label">Theme</span>
    <select class="re-select" data-size="sm" aria-label="Theme" :value="ui.theme" @change="onChange">
      <option v-for="m in modes" :key="m.value" :value="m.value">{{ m.label }}</option>
    </select>
  </label>
</template>

<style scoped>
.theme-toggle {
  margin-top: auto;
}
</style>
```

- [ ] **Step 5: Render it in the sidebar footer**

In `src/App.vue`, import and place `ThemeToggle` in the sidebar footer area.
Add to the `<script setup>` imports:

```ts
import ThemeToggle from "./components/ThemeToggle.vue";
```

In the template, replace the existing footer line so the footer holds the toggle
above the label:

```vue
<div class="sidebar__footer">
  <ThemeToggle />
  <span class="sidebar__footer-label">Unified Agentic Workspace</span>
</div>
```

(Adjust the `.sidebar__footer` scoped style to `display: flex; flex-direction: column; gap: 0.6rem;` and move the existing `margin-top: auto` / font-size onto `.sidebar__footer-label`.)

- [ ] **Step 6: Run the test to verify it passes**

Run: `pnpm e2e:docker`
Expected: both `theme toggle` and `smoke` specs pass.

- [ ] **Step 7: Build + typecheck**

Run: `pnpm build && pnpm e2e:typecheck`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add e2e/specs/theme.e2e.ts src/stores/ui.ts src/components/ThemeToggle.vue src/App.vue
git commit -m "feat(ui): add System/Light/Dark theme toggle in sidebar footer"
```

---

## Task 3: Restyle the app shell / sidebar

**Files:**
- Modify: `src/App.vue` (template nav + scoped styles)

- [ ] **Step 1: Convert nav and primary action to design-system buttons**

In `src/App.vue` template:
- "New Session" button: add classes `class="re-button" data-variant="primary"` (keep its `@click` and the text "New Session").
- Each nav item (`Inbox`, the status-group buttons, `Projects`): `class="re-button" data-variant="ghost"`; keep the existing `:class` active binding but rename the active modifier to set `aria-current="page"` when active. Keep all visible label text unchanged.
- Disabled planned-section buttons: `class="re-button" data-variant="ghost"` with `disabled`.

- [ ] **Step 2: Trim the scoped styles to layout only**

In `src/App.vue` `<style scoped>`, delete the bespoke `.nav__item*` color/padding/border rules (the `.re-button` classes now own appearance). Keep the layout rules: `.app` grid, `.sidebar` (flex column, padding, `border-right: 1px solid var(--re-color-border)`, `background: var(--re-color-surface)`), `.nav` (flex column gap), `.main`, `.main__header`, `.brand`. Add a small rule so ghost nav buttons align left and fill width:

```css
.nav .re-button {
  justify-content: flex-start;
  width: 100%;
}
.nav .re-button[aria-current="page"] {
  background: var(--re-color-bg-muted);
}
```

Replace any remaining `var(--uaw-*)` usages in this block with the `--re-*`
equivalents from the spec's mapping table.

- [ ] **Step 3: Build + typecheck**

Run: `pnpm build && pnpm e2e:typecheck`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/App.vue
git commit -m "style(ui): restyle shell and sidebar nav with re-button"
```

---

## Task 4: Restyle WorkspaceSwitcher

**Files:**
- Modify: `src/components/WorkspaceSwitcher.vue`

- [ ] **Step 1: Apply design-system classes (preserve all hooks)**

In the template:
- Switcher `<select>`: `class="re-select" data-size="sm"`, keep `aria-label="Select workspace"`.
- Create `<input>`: `class="re-input" data-size="sm"`, keep `aria-label="New workspace name"`.
- The `+` / `✓` / `×` buttons: `class="re-button" data-variant="ghost"`, keep `aria-label="New workspace"` on the open button and `title="Create"` on the submit button.
- Keep the `switcher__row` flex wrapper.

- [ ] **Step 2: Trim scoped styles**

Delete the `.switcher__select`, `.switcher__input`, and `.switcher__new` color/border/padding rules (design-system classes own these). Keep `.switcher` (flex column gap), `.switcher__label` (use `color: var(--re-color-text-muted)`), and `.switcher__row` (flex gap). Constrain the icon buttons with `.switcher__row .re-button { flex: 0 0 auto; }` and let the select/input take `flex: 1`.

- [ ] **Step 3: Build + typecheck**

Run: `pnpm build && pnpm e2e:typecheck`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/components/WorkspaceSwitcher.vue
git commit -m "style(ui): restyle workspace switcher with re-select/re-input/re-button"
```

---

## Task 5: Restyle ProjectsView

**Files:**
- Modify: `src/components/ProjectsView.vue`

- [ ] **Step 1: Convert the create form**

- Name `<input>`: `class="re-input"`, keep `aria-label="New project name"`.
- Mode `<select>`: `class="re-select"`, keep `aria-label="Project mode"`.
- Create `<button>`: `class="re-button" data-variant="primary"` (keep text "Create" and `:disabled`).
- Wrap the form controls in `.re-field` labels only if it doesn't disturb the flex row; otherwise keep the existing `.create` flex layout and just add the `.re-*` classes to the controls.

- [ ] **Step 2: Convert rows to cards (preserve hooks)**

- Each row `<li>`: `class="re-card"`, keep `data-testid="project-row"`.
- Keep the inner `<span class="row__title">{{ project.name }}</span>` element unchanged (e2e reads its `textContent`).
- Mode badge: `class="re-badge" data-variant="neutral"` (keep the text).
- Rename button: `class="re-button" data-variant="ghost" data-size="sm"`; Delete button: `class="re-button" data-variant="danger" data-size="sm"`.
- For the inline rename input: `class="re-input" data-size="sm"`, Save/Cancel buttons `class="re-button" data-variant="ghost" data-size="sm"`.

- [ ] **Step 3: Trim scoped styles**

Delete the `.create__input`, `.create__select`, `.create__submit`, `.row`, `.row__action`, `.badge` appearance rules. Keep layout-only rules: `.view-title`, `.create` (flex gap), `.rows` (flex column gap), `.row__title` (`flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap`), `.row__actions` (flex gap). Swap any `var(--uaw-*)` to `--re-*`. The `.re-card` provides the row surface/border/padding.

- [ ] **Step 4: Build + typecheck**

Run: `pnpm build && pnpm e2e:typecheck`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/ProjectsView.vue
git commit -m "style(ui): restyle projects view with re-card/re-field/re-badge"
```

---

## Task 6: Restyle SessionsView

**Files:**
- Modify: `src/components/SessionsView.vue`

- [ ] **Step 1: Convert the create form**

- Title `<input>` → `class="re-input"` (keep `aria-label="New session title"`).
- Mode `<select>` → `class="re-select"` (keep `aria-label="Session mode"`).
- Project `<select>` → `class="re-select"` (keep `aria-label="Session project"`).
- Create `<button>` → `class="re-button" data-variant="primary"`.

- [ ] **Step 2: Convert rows (preserve hooks)**

- Each row `<li>` → `class="re-card"`, keep `data-testid="session-row"`.
- Keep inner `<span class="row__title">` (session title) and `<span class="row__project">` (project name) elements — both read by e2e.
- Mode badge → `class="re-badge" data-variant="neutral"`.
- Status `<select>` → `class="re-select" data-size="sm"`, keep `aria-label="Session status"`.
- Delete button → `class="re-button" data-variant="danger" data-size="sm"`.
- Keep the group heading `<h3>` text (e.g. "Done") and the `.group__count` element.

- [ ] **Step 3: Trim scoped styles**

Delete bespoke `.create__*`, `.row`, `.row__status`, `.row__action`, `.badge` appearance rules. Keep layout: `.view-title`, `.create`, `.group`, `.group__title`, `.group__count`, `.rows`, `.row__main` (flex column), `.row__title`, `.row__meta`, `.row__project` (`color: var(--re-color-text-muted)`). Swap `var(--uaw-*)` → `--re-*`.

- [ ] **Step 4: Build + typecheck + full e2e checkpoint**

Run: `pnpm build && pnpm e2e:typecheck`
Expected: PASS.

Run: `pnpm e2e:docker`
Expected: `smoke` (5) and `theme toggle` specs all pass — confirms the
restyle preserved every selector.

- [ ] **Step 5: Commit**

```bash
git add src/components/SessionsView.vue
git commit -m "style(ui): restyle sessions view with re-card/re-select/re-badge"
```

---

## Task 7: Toast feedback behavior

Replaces the inline `formError` text in the views with design-system toasts and
adds success toasts for create/delete.

**Files:**
- Create: `src/composables/useToast.ts`
- Modify: `src/components/WorkspaceSwitcher.vue`, `src/components/ProjectsView.vue`, `src/components/SessionsView.vue`

- [ ] **Step 1: Confirm the showToast signature**

Run: `cat node_modules/@relements/core/dist/behaviors/toast.d.ts`
Expected: shows the `showToast(message, options)` signature. Note the exact
option key for tone/variant (e.g. `variant` or `tone`) and the allowed values;
use it in the next step instead of guessing.

- [ ] **Step 2: Create the toast composable**

Create `src/composables/useToast.ts` (adjust the option key/values to match the
`.d.ts` from Step 1):

```ts
import { showToast } from "@relements/core/behaviors/toast";

export function useToast() {
  return {
    success: (message: string) => showToast(message, { variant: "success" }),
    error: (message: string) => showToast(message, { variant: "danger" }),
  };
}
```

- [ ] **Step 3: Replace inline errors with toasts in each view**

In `ProjectsView.vue` and `SessionsView.vue`:
- Import `useToast`, instantiate `const toast = useToast();`.
- In each `catch (e)` that currently sets `formError.value = String(e)`, call
  `toast.error(String(e))` instead. Remove the `formError` ref and the
  `<p v-if="formError" class="error">` element.
- After a successful create/delete, call `toast.success("Project created")` etc.

In `WorkspaceSwitcher.vue`: wrap `store.create` in try/catch and call
`toast.error` on failure, `toast.success("Workspace created")` on success.

- [ ] **Step 4: Build + typecheck + e2e**

Run: `pnpm build && pnpm e2e:typecheck`
Expected: PASS.

Run: `pnpm e2e:docker`
Expected: all specs pass (the smoke spec never asserted on error text).

- [ ] **Step 5: Commit**

```bash
git add src/composables/useToast.ts src/components/ProjectsView.vue src/components/SessionsView.vue src/components/WorkspaceSwitcher.vue
git commit -m "feat(ui): toast feedback for create/delete via relements behavior"
```

---

## Task 8: Replace native delete confirms with a re-dialog modal

**Files:**
- Create: `src/composables/useConfirm.ts`
- Create: `src/components/ConfirmDialog.vue`
- Modify: `src/App.vue` (mount `ConfirmDialog` once), `src/components/ProjectsView.vue`, `src/components/SessionsView.vue`
- Modify: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/capabilities/default.json` (remove the now-unused dialog plugin)

- [ ] **Step 1: Create the confirm composable**

Create `src/composables/useConfirm.ts` — a module-level shared dialog state with
a promise-returning `confirm()`:

```ts
import { ref } from "vue";

const open = ref(false);
const title = ref("Confirm");
const message = ref("");
let resolver: ((value: boolean) => void) | null = null;

export function useConfirm() {
  function confirm(msg: string, dialogTitle = "Confirm"): Promise<boolean> {
    message.value = msg;
    title.value = dialogTitle;
    open.value = true;
    return new Promise((resolve) => {
      resolver = resolve;
    });
  }

  function settle(value: boolean) {
    open.value = false;
    resolver?.(value);
    resolver = null;
  }

  return { open, title, message, confirm, settle };
}
```

- [ ] **Step 2: Create the ConfirmDialog component**

Create `src/components/ConfirmDialog.vue` — uses the native `<dialog>` with the
`.re-dialog` class for styling, driven by the shared `open` state:

```vue
<script setup lang="ts">
import { ref, watch } from "vue";
import { useConfirm } from "../composables/useConfirm";

const { open, title, message, settle } = useConfirm();
const dialog = ref<HTMLDialogElement | null>(null);

watch(open, (isOpen) => {
  const el = dialog.value;
  if (!el) return;
  if (isOpen && !el.open) el.showModal();
  if (!isOpen && el.open) el.close();
});

// Backdrop/Esc close resolves as cancel.
function onClose() {
  if (open.value) settle(false);
}
</script>

<template>
  <dialog ref="dialog" class="re-dialog" @close="onClose">
    <form method="dialog" class="re-dialog__body">
      <h2>{{ title }}</h2>
      <p>{{ message }}</p>
      <div class="re-dialog__footer">
        <button type="button" class="re-button" data-variant="ghost" @click="settle(false)">
          Cancel
        </button>
        <button type="button" class="re-button" data-variant="danger" @click="settle(true)">
          Delete
        </button>
      </div>
    </form>
  </dialog>
</template>
```

- [ ] **Step 3: Mount the dialog once in App.vue**

In `src/App.vue`, import `ConfirmDialog` and render it once inside the root
`.app` element (e.g. just before `</div>`):

```ts
import ConfirmDialog from "./components/ConfirmDialog.vue";
```

```vue
<ConfirmDialog />
```

- [ ] **Step 4: Switch the views from `ask()` to `confirm()`**

In `ProjectsView.vue` and `SessionsView.vue`:
- Remove `import { ask } from "@tauri-apps/plugin-dialog";`.
- Add `import { useConfirm } from "../composables/useConfirm";` and
  `const { confirm } = useConfirm();`.
- Replace `const confirmed = await ask(...)` with
  `const confirmed = await confirm(\`Delete project "\${name}"? Its sessions are kept and detached.\`, "Delete project");`
  (and the session equivalent: `confirm(\`Delete session "\${title}"?\`, "Delete session")`).

- [ ] **Step 5: Remove the unused Tauri dialog plugin**

- `package.json`: remove `@tauri-apps/plugin-dialog` (run `pnpm remove @tauri-apps/plugin-dialog`).
- `src-tauri/Cargo.toml`: delete the `tauri-plugin-dialog = "2"` line.
- `src-tauri/src/lib.rs`: delete `.plugin(tauri_plugin_dialog::init())`.
- `src-tauri/capabilities/default.json`: remove `"dialog:default"` from `permissions`.

- [ ] **Step 6: Verify backend + frontend build**

Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: PASS (no reference to the removed plugin).

Run: `pnpm build && pnpm e2e:typecheck`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/composables/useConfirm.ts src/components/ConfirmDialog.vue src/App.vue src/components/ProjectsView.vue src/components/SessionsView.vue package.json pnpm-lock.yaml src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src-tauri/capabilities/default.json
git commit -m "feat(ui): re-dialog delete confirms; drop tauri-plugin-dialog"
```

---

## Task 9: Remove the token alias and final verification

**Files:**
- Modify: `src/App.vue` (remove the `--uaw-*` alias block)

- [ ] **Step 1: Confirm nothing still references the old variables**

Run: `grep -rn "uaw-" src/ || echo "none"`
Expected: only the alias definitions in `App.vue` (if any component still uses
`var(--uaw-*)`, swap it to the `--re-*` equivalent from the spec mapping first).

- [ ] **Step 2: Delete the alias block**

In `src/App.vue`, remove the `:root { --uaw-*: var(--re-*) }` alias block added
in Task 1. Re-run the grep to confirm `none`.

- [ ] **Step 3: Full verification**

Run: `pnpm build && pnpm e2e:typecheck`
Expected: PASS.

Run: `pnpm format && pnpm format:check`
Expected: all files formatted, check clean.

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: 15 passing (unchanged — no backend logic touched beyond plugin removal).

Run: `pnpm e2e:docker`
Expected: `smoke` (5) + `theme toggle` specs all pass.

- [ ] **Step 4: Manual smoke (visual)**

Run: `pnpm tauri dev`
Confirm: app renders in renascent dark; the sidebar-footer Theme control
switches Dark / Light / System; choice persists across a restart; toasts appear
on create/delete; delete shows the in-app confirm dialog.

- [ ] **Step 5: Commit and push**

```bash
git add src/App.vue
git commit -m "style(ui): remove --uaw-* alias; renascent tokens throughout"
git push -u origin cstuncsik/style-renascent
```

---

## Self-review notes

- **Spec coverage:** integration + token mapping (Task 1), theme toggle tri-state in sidebar footer (Task 2), shell/switcher/projects/sessions component-class adoption (Tasks 3–6), toasts (Task 7), re-dialog confirms + plugin removal (Task 8), alias removal + verification (Task 9). The "keep `.re-select`, no re-menu" decision is reflected (Task 4 uses `.re-select`).
- **e2e green:** every UI task preserves the documented hooks; full `pnpm e2e:docker` checkpoints at Tasks 2, 6, and 9.
- **Deviation from spec wording:** the spec mentioned `enhanceDialog` for confirms; the plan uses the `.re-dialog` class with native `<dialog>.showModal()` because the confirm flow is programmatic (promise-returning), not trigger-bound. Same design-system look; `enhanceDialog` is unnecessary here. Toast option key is verified against the shipped `.d.ts` in Task 7 Step 1 rather than guessed.
