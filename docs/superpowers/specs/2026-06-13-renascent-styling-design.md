# Renascent Styling — Design

Date: 2026-06-13
Branch: `cstuncsik/style-renascent`

## Goal

Restyle the UAW desktop app with the user's own design system,
[`@relements/core`](https://www.npmjs.com/package/@relements/core) (v0.10.0),
applying its **renascent** theme. Full adoption of the design system's
component classes and tokens, a focused set of its JS behaviors, plus a
light/dark theme toggle. No new product features.

## The package (confirmed facts)

`@relements/core` is an HTML-first, framework-agnostic CSS design system (zero
runtime deps):

- **CSS classes + `data-*` variants**: `.re-button` (`data-variant`:
  primary/secondary/ghost/danger), `.re-input`, `.re-select`, `.re-textarea`,
  `.re-field` (+ `.re-field__label`/`__hint`), `.re-card`
  (`__header`/`__body`/`__footer`), `.re-badge` / `.re-tag`
  (`data-variant`: neutral/info/success/warning/danger), `.re-dialog`,
  `.re-menu`, `.re-toast-region`, and more.
- **Tokens**: `--re-*` CSS custom properties (e.g. `--re-color-bg`,
  `--re-color-surface`, `--re-color-text`, `--re-color-text-muted`,
  `--re-color-border`, `--re-color-accent-600`, `--re-color-text-danger`).
- **JS behaviors** (tree-shakable ESM): `showToast`, `enhanceDialog`,
  `enhanceMenuButton`, `enhanceTabs`, `enhancePopover`, `enhanceDismissible`.
- **Web components** (light-DOM): `<re-toast>`, `<re-menu>`, `<re-popover>`,
  `<re-tabs>`.

### Theme activation mechanism (confirmed from `themes/renascent.css`)

- `:root` defaults to **dark** (renascent navy).
- An `@media (prefers-color-scheme: light)` block flips `:root` and
  `.theme-renascent` to **light**.
- Explicit override classes:
  - `.theme-renascent-dark` → force dark (ignores OS preference)
  - `.theme-renascent-light` → force light (ignores OS preference)
  - `.theme-renascent` → follow OS preference

So the theme toggle is implemented purely by setting a class on the root
element; no conditional CSS imports are needed.

## Integration

1. `pnpm add @relements/core` (a dependency, not dev — it ships runtime CSS/JS).
2. In `src/main.ts`, import once, before mounting:
   ```ts
   import "@relements/core/index.css";
   import "@relements/core/themes/renascent.css";
   ```
3. Remove the bespoke `--uaw-*` palette declared in `src/App.vue`'s global
   `:root` block. All color/border/text usage moves to `--re-*` tokens.

### Token mapping (`--uaw-*` → `--re-*`)

| Current `--uaw-*`      | Replacement `--re-*`                          |
| ---------------------- | --------------------------------------------- |
| `--uaw-bg`             | `--re-color-bg`                               |
| `--uaw-surface`        | `--re-color-surface`                          |
| `--uaw-surface-hover`  | `--re-color-bg-muted`                         |
| `--uaw-border`         | `--re-color-border`                           |
| `--uaw-text`           | `--re-color-text`                             |
| `--uaw-muted`          | `--re-color-text-muted`                       |
| error red `#ff6b6b`    | `--re-color-text-danger`                      |

The renascent dark palette is close to the current hand-rolled one, so the app
keeps a similar dark look while gaining the design system's consistency.

## Theme toggle

- A small control in the **sidebar footer** with three states: **System**
  (default), **Light**, **Dark**.
- Applies the matching class to `document.documentElement`:
  System → `theme-renascent`, Light → `theme-renascent-light`,
  Dark → `theme-renascent-dark`.
- Persists the choice to `localStorage` under `uaw.theme`.
- Applied **before mount** in `main.ts` (reading `localStorage`) to avoid a
  flash of the wrong theme on startup.
- A tiny Pinia `ui` store (or a composable) holds the current mode and writes
  through to `localStorage` + the root class. No backend change.

## Component mapping

### `src/App.vue` (shell / sidebar)

- Drop the `--uaw-*` `:root` block; body bg/text come from the renascent theme.
- Sidebar surface → `--re-color-surface`; borders → `--re-color-border`.
- "New Session" → `.re-button[data-variant=primary]`.
- Nav items → `.re-button[data-variant=ghost]`; active item marked with
  `aria-current="page"` and a token-based active style. Keep the visible text
  ("Inbox", "Projects", status group labels) unchanged for e2e selectors.
- Brand and footer → tokens.

### `src/components/WorkspaceSwitcher.vue`

- Switcher → `.re-select` (keep `aria-label="Select workspace"`).
- Create flow stays inline: `.re-field` + `.re-input` (keep
  `aria-label="New workspace name"`); the `+` / confirm / cancel buttons →
  `.re-button[data-variant=ghost]`. Keep `aria-label="New workspace"` on the
  open button and `title="Create"` on the submit button.

### `src/components/ProjectsView.vue`

- Create form → `.re-field` / `.re-input` (keep `aria-label="New project name"`)
  / `.re-select` (keep `aria-label="Project mode"`); Create →
  `.re-button[data-variant=primary]`.
- Each row → `.re-card` (the `<li>`), keeping `data-testid="project-row"` and an
  inner `.row__title` element (e2e reads its `textContent`).
- Mode → `.re-badge[data-variant=neutral]`. Rename → `.re-button[data-variant=ghost]`,
  Delete → `.re-button[data-variant=danger]`.

### `src/components/SessionsView.vue`

- Create form mirrors ProjectsView; keep `aria-label`s "New session title",
  "Session mode", "Session project".
- Each row → `.re-card`, keeping `data-testid="session-row"`, `.row__title`,
  and `.row__project` (all read by e2e).
- Mode → `.re-badge`; project name keeps the `.row__project` element.
- Status `<select>` → `.re-select` (keep `aria-label="Session status"`).
- Group headings keep their text (e.g. "Done") for the `h3*=Done` selector.

## Behaviors (the "+ behaviors" set)

1. **Toasts** — mount a `<re-toast>` region (or call `showToast`) in `App.vue`.
   Replace the inline `formError` text in the views with error toasts, and add
   success toasts for create/delete. A thin `useToast` composable wraps
   `showToast` so views don't import the behavior directly.
2. **`re-dialog` confirms** — replace the `tauri-plugin-dialog` `ask()` calls
   for **delete project** and **delete session** with a `.re-dialog` modal wired
   by `enhanceDialog`, for a consistent in-app look. (`@tauri-apps/plugin-dialog`
   is removed if no longer used.)
3. **`re-menu`** — convert the workspace switcher's selector to a `.re-menu` +
   `enhanceMenuButton` if it improves the switch UX; otherwise keep `.re-select`.
   This is the one "nice to have" — drop it if it adds churn without payoff.

Behaviors are initialized in `App.vue`'s `onMounted` against `document`, and
torn down on unmount (each returns `{ destroy() }`).

## Keep the e2e gate green

The WebdriverIO suite (`e2e/specs/smoke.e2e.ts`) drives these hooks, which the
restyle MUST preserve or the spec must be updated in lockstep:

- `aria-label`s: Select workspace, New workspace, New workspace name,
  New project name, Project mode, New session title, Session mode,
  Session project, Session status.
- Visible text: "Projects", "Inbox", "Create"; `title="Create"` on the
  workspace submit; status group heading "Done".
- `data-testid="project-row"` / `data-testid="session-row"`, and inner
  `.row__title` / `.row__project` elements (read via `textContent`).

The current smoke spec does **not** exercise delete/confirm, so moving deletes
to `re-dialog` does not affect it. After the restyle, run `pnpm e2e:docker`
locally and confirm the CI `e2e` gate is green; update selectors only where
markup genuinely changes.

## Out of scope

- New product features or surfaces beyond the four current components.
- Backend/Rust changes (theme preference is client-side `localStorage`).
- Restyling the e2e/CI infrastructure.

## Verification

- `pnpm build` (vue-tsc + vite) and `pnpm e2e:typecheck` clean.
- `pnpm e2e:docker` green (5/5); CI `e2e` gate green on the PR.
- `prettier --check` clean.
- Manual `pnpm tauri dev`: app renders in renascent dark; the toggle switches
  Dark / Light / System and the choice survives a restart.
- `cargo` unaffected (no backend changes).
