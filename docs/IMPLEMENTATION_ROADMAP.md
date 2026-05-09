# Implementation Roadmap

This roadmap builds UAW from a clean repository into the first useful MVP.

Each milestone should be small enough to verify manually. Avoid broad rewrites and avoid building advanced integrations before the local workflow works.

## Milestone 0: Project Bootstrap

Goal:

Create the initial app shell and development foundation.

Tasks:

- Scaffold Tauri + Vue 3 + TypeScript.
- Add Rust backend structure.
- Add SQLite dependency and database initialization.
- Add migration runner.
- Add base app layout.
- Add formatting, typecheck, and build scripts.
- Add a minimal README with development commands.

Done when:

- Desktop app launches.
- Frontend builds.
- Rust backend checks.
- SQLite database initializes.

Validation:

```bash
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
pnpm tauri dev
```

## Milestone 1: Workspace Foundation

Goal:

Add workspace as the top-level environment boundary.

Backend tasks:

- Add `workspaces` table.
- Add Rust `Workspace` model.
- Add DB functions.
- Add Tauri commands:
  - `list_workspaces`
  - `create_workspace`
  - `get_workspace`
  - `update_workspace`
  - `delete_workspace`

Frontend tasks:

- Add `Workspace` type.
- Add Tauri API wrappers.
- Add workspace store.
- Add workspace switcher.
- Create a default workspace on first launch.

Done when:

- User can create and switch workspaces.
- Current workspace is visible in the app shell.
- Workspace selection scopes the visible empty state.

Manual test:

1. Launch UAW.
2. Confirm default workspace appears.
3. Create another workspace.
4. Switch between workspaces.

## Milestone 2: Projects And Sessions

Goal:

Add the core work organization model.

Backend tasks:

- Add `projects` table.
- Add `sessions` table.
- Add project modes: `research`, `code`, `mixed`.
- Add session modes: `research`, `document`, `code`, `review`, `terminal`.
- Add session statuses.
- Add CRUD commands.

Frontend tasks:

- Add project store.
- Add session store.
- Add project creation by mode.
- Add session list grouped by status.
- Add status filter sidebar sections.

Done when:

- User can create projects inside workspaces.
- User can create sessions inside workspaces/projects.
- User can move sessions through `todo`, `running`, `needs-review`, and `done`.

## Milestone 3: Product-Shaped Navigation

Goal:

Make the app shell reflect the final product model early.

Sidebar sections:

- Workspace switcher.
- New Session.
- Inbox / All Sessions.
- Projects.
- Sources.
- Skills.
- Automations.
- Reviews.
- Settings.

Done when:

- Navigation exposes the right concepts even if some screens are still empty.
- Review and Automations are first-class areas, not hidden settings.

## Milestone 4: Markdown Artifacts

Goal:

Make durable documents first-class.

Backend tasks:

- Add `artifacts` table.
- Add artifact commands:
  - `list_artifacts`
  - `create_artifact`
  - `get_artifact`
  - `update_artifact`
  - `delete_artifact`

Frontend tasks:

- Add artifact store.
- Add artifact list.
- Add markdown editor/viewer.
- Add artifact creation from scratch.

Done when:

- User can create, edit, and reopen markdown artifacts inside a workspace/project.

## Milestone 5: Sources, Skills, Automations Skeleton

Goal:

Make workspace-scoped product primitives visible and manageable.

Backend tasks:

- Add `sources`, `skills`, and `automations` tables.
- Add CRUD commands for each.
- Store config as JSON for now.

Frontend tasks:

- Add list/detail screens for Sources, Skills, and Automations.
- Add starter skill templates:
  - Write PRD.
  - Summarize research.
  - Review diff.
  - Generate test plan.
- Add starter automation template:
  - Coding session completed review.

Done when:

- User can manage workspace-scoped sources, skills, and automations.
- Automations are visible as first-class objects.

## Milestone 6: Repository Sources

Goal:

Attach local git repositories.

Backend tasks:

- Add `repository_sources` table.
- Add commands:
  - `validate_repository_path`
  - `create_repository_source`
  - `list_repository_sources`
  - `get_repository_status`
  - `list_repository_branches`

Frontend tasks:

- Add repository source form.
- Show path, default branch, current branch, dirty status, and validation errors.

Done when:

- User can attach a local repo.
- UAW can verify it is a git repository.
- UAW can show branch and dirty status.

Security:

- Do not read ignored secret files.
- Do not run setup commands automatically.

## Milestone 7: Coding Workspaces And Git Worktrees

Goal:

Create isolated implementation environments.

Backend tasks:

- Add `coding_workspaces` table.
- Add git service functions using git CLI.
- Add commands:
  - `create_coding_workspace`
  - `list_coding_workspaces`
  - `get_coding_workspace`
  - `get_coding_workspace_diff`
  - `discard_coding_workspace`
  - `mark_coding_workspace_ready_for_review`

Frontend tasks:

- Add "New coding session" action for code projects.
- Add branch name/base branch fields.
- Show worktree path and status.
- Show changed files and diff stat.

Done when:

- User can create a worktree from an attached repo.
- User can make manual changes in the worktree.
- UAW can detect changed files and diff.
- User can mark it Needs Review.

Safety:

- Discard requires explicit confirmation.
- Keep branch/worktree is available.
- Dirty worktrees are never deleted silently.

## Milestone 8: Review Records And Review View

Goal:

Make completed work easy to judge.

Backend tasks:

- Add `reviews` table.
- Add deterministic review generation:
  - `git status --short`
  - `git diff --stat`
  - `git diff --name-only`
  - optional configured test command
- Add review commands:
  - `create_review_for_coding_workspace`
  - `list_reviews`
  - `get_review`
  - `update_review_status`

Frontend tasks:

- Add Reviews section.
- Add pending review list.
- Add review detail screen with summary, files changed, test output, and risk notes.
- Add status actions: approve, reject, changes requested, done.

Done when:

- Coding work can move from worktree to review record.
- User can inspect and decide without reading chat history.

## Milestone 9: First Automation

Goal:

Implement the mandatory coding completion automation.

Automation:

```txt
When coding session is marked complete:
1. collect diff
2. run configured checks if present
3. create review
4. summarize risks
5. move session to Needs Review
```

Backend tasks:

- Add automation runner service.
- Add event record for `coding_workspace.completed`.
- Add command to trigger the completion flow.

Frontend tasks:

- Add "Complete and review" button.
- Show automation progress.
- Link resulting review.

Done when:

- User can complete a coding workspace and land in Needs Review with a persisted review.

## Milestone 10: Agent Adapter MVP

Goal:

Replace manual worktree changes with a real agent execution path.

Recommended order:

```txt
Manual adapter -> one CLI adapter -> API-backed research adapter
```

Backend tasks:

- Add adapter trait/module boundary.
- Add session event log.
- Add commands to start, stop, and inspect an agent session.
- Persist agent output events.

Frontend tasks:

- Add agent session inspector.
- Show logs/events.
- Show permissions.
- Show current status.

Done when:

- UAW can start or track an implementation session.
- Session events persist.
- Completion triggers review automation.

## Milestone 11: Dispatch From Artifact To Coding Tasks

Goal:

Connect research mode to execution mode.

Backend tasks:

- Add task extraction or task artifact model.
- Add `dispatch_artifact` command.
- Create one or more coding sessions from selected artifact sections.

Frontend tasks:

- Add "Dispatch" action on artifact pages.
- Let user edit generated tasks before creating sessions.
- Let user choose repo and base branch per task.

Done when:

- User can turn a markdown spec into coding sessions.
- Sessions link back to the artifact that spawned them.

## Milestone 12: Parallel Agent Board

Goal:

Make simultaneous work visible.

Tasks:

- Add board view grouped by status.
- Show coding workspace health, review state, changed files, and last event.
- Allow multiple active sessions.
- Add compare view for multiple reviews/diffs.

Done when:

- User can monitor more than one coding task without losing track.

## Recommended First PR

Scope:

- Project scaffold.
- Migration runner.
- Workspace foundation.
- Minimal shell UI.

Suggested branch:

```bash
git checkout -b cdx-uaw-workspace-foundation
```

Suggested commit:

```txt
feat: scaffold workspace foundation
```

Do not include:

- Worktrees.
- Agent execution.
- Real review automation.
- Connector integrations.
- Full visual redesign.

## Verification Checklist For Every Milestone

- `pnpm build`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- Manual smoke test in `pnpm tauri dev`
- New destructive operations require explicit confirmation.
- New workspace-scoped records do not leak across workspaces.
- New long-running operations persist event state.
