# Technical Architecture

## Architecture Goal

UAW should be a local-first desktop app that can safely coordinate research work, document artifacts, coding agents, repositories, git worktrees, automations, and review workflows.

The architecture should support long-running local processes and persistent work state without turning the frontend into a process manager.

## Recommended Initial Stack

- Desktop shell: Tauri.
- Frontend: Vue 3 + TypeScript.
- Backend: Rust services exposed through Tauri commands.
- Storage: SQLite.
- Secrets: OS keychain.
- Git operations: git CLI.
- Terminal/process handling: backend-managed PTY/process services.
- Agent execution: adapter interface.
- Future integrations: MCP and connector-specific adapters.

## Core Layers

```txt
UI Layer
- workspace sidebar
- project views
- session views
- artifact editor
- agent board
- review view
- settings and policy screens

Project Layer
- workspaces
- projects
- sessions
- artifacts
- memory
- sources
- skills
- automations
- reviews

Agent Layer
- adapter interface
- session manager
- permission manager
- event log
- review runner
- automation runner

System Layer
- SQLite
- filesystem
- git CLI
- shell commands
- PTY/process management
- OS keychain
- MCP later
```

## First Data Model

Start with a migration system and these core tables.

```sql
CREATE TABLE schema_migrations (
  version INTEGER PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  applied_at TEXT NOT NULL
);

CREATE TABLE workspaces (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  kind TEXT NOT NULL DEFAULT 'mixed',
  settings_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE projects (
  id TEXT PRIMARY KEY NOT NULL,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  mode TEXT NOT NULL DEFAULT 'research',
  settings_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE sessions (
  id TEXT PRIMARY KEY NOT NULL,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
  title TEXT NOT NULL,
  mode TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'todo',
  summary TEXT,
  permissions_json TEXT NOT NULL DEFAULT '{}',
  context_refs_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

Add these next:

```sql
CREATE TABLE sources (
  id TEXT PRIMARY KEY NOT NULL,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
  name TEXT NOT NULL,
  type TEXT NOT NULL,
  config_json TEXT NOT NULL DEFAULT '{}',
  permissions_json TEXT NOT NULL DEFAULT '{}',
  status TEXT NOT NULL DEFAULT 'connected',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE skills (
  id TEXT PRIMARY KEY NOT NULL,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  category TEXT NOT NULL,
  instructions TEXT NOT NULL,
  required_sources_json TEXT NOT NULL DEFAULT '[]',
  required_permissions_json TEXT NOT NULL DEFAULT '[]',
  output_type TEXT NOT NULL DEFAULT 'chat',
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE automations (
  id TEXT PRIMARY KEY NOT NULL,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  trigger_json TEXT NOT NULL DEFAULT '{}',
  action_json TEXT NOT NULL DEFAULT '{}',
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE artifacts (
  id TEXT PRIMARY KEY NOT NULL,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
  created_from_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  title TEXT NOT NULL,
  type TEXT NOT NULL DEFAULT 'markdown',
  content TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

Repository and review tables:

```sql
CREATE TABLE repository_sources (
  id TEXT PRIMARY KEY NOT NULL,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
  name TEXT NOT NULL,
  local_path TEXT NOT NULL,
  default_branch TEXT NOT NULL DEFAULT 'main',
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE coding_workspaces (
  id TEXT PRIMARY KEY NOT NULL,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  repository_source_id TEXT NOT NULL REFERENCES repository_sources(id) ON DELETE CASCADE,
  session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
  repo_path TEXT NOT NULL,
  worktree_path TEXT NOT NULL,
  branch_name TEXT NOT NULL,
  base_branch TEXT NOT NULL,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE reviews (
  id TEXT PRIMARY KEY NOT NULL,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
  session_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
  coding_workspace_id TEXT REFERENCES coding_workspaces(id) ON DELETE CASCADE,
  status TEXT NOT NULL DEFAULT 'pending',
  summary TEXT NOT NULL DEFAULT '',
  risk_notes TEXT NOT NULL DEFAULT '',
  diff_summary_json TEXT NOT NULL DEFAULT '{}',
  test_result_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE session_events (
  id TEXT PRIMARY KEY NOT NULL,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  type TEXT NOT NULL,
  payload_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);
```

## Frontend State Boundaries

Use separate stores by product concept:

- `workspaces`
- `projects`
- `sessions`
- `artifacts`
- `sources`
- `skills`
- `automations`
- `repositories`
- `codingWorkspaces`
- `reviews`

Avoid a single large app store. The product has enough domain boundaries to keep stores focused.

## Tauri Command Groups

Create command modules by concept:

```txt
commands/workspaces.rs
commands/projects.rs
commands/sessions.rs
commands/artifacts.rs
commands/sources.rs
commands/skills.rs
commands/automations.rs
commands/repositories.rs
commands/coding_workspaces.rs
commands/reviews.rs
commands/agents.rs
```

Keep commands thin:

```txt
command -> validate input -> call service/db layer -> return DTO
```

Add services as logic grows:

```txt
services/git.rs
services/worktrees.rs
services/agents.rs
services/reviews.rs
services/permissions.rs
services/automations.rs
```

## Agent Adapter Interface

Introduce an adapter boundary before hard-coding one engine.

```ts
interface AgentAdapter {
  id: string;
  name: string;
  startSession(input: StartSessionInput): Promise<AgentSessionHandle>;
  sendMessage(sessionId: string, message: string): Promise<void>;
  stopSession(sessionId: string): Promise<void>;
  onEvent(callback: (event: AgentEvent) => void): void;
}
```

Candidate adapters:

- Manual adapter: user performs work in the worktree, UAW collects diff/review.
- Codex CLI adapter.
- Claude Code CLI adapter.
- Anthropic API adapter for research/document sessions.
- OpenAI API adapter for research/document sessions.

Recommended first adapter:

```txt
Manual adapter -> one CLI adapter -> API-backed research adapter
```

## Worktree Lifecycle

MVP commands:

```txt
validate_repository(path)
list_branches(repository_id)
create_coding_workspace(project_id, repository_id, base_branch, branch_name)
get_coding_workspace_status(id)
get_coding_workspace_diff(id)
mark_coding_workspace_ready_for_review(id)
discard_coding_workspace(id)
```

Implementation rules:

- Use git CLI first.
- Never copy `.env` automatically.
- Never delete a dirty worktree without explicit confirmation.
- Store repo path, worktree path, branch, base branch, and status.
- Put generated worktrees in an app-controlled directory unless the user overrides it.
- Treat repository bootstrap commands as explicit project configuration.

## Review Flow

Review should be a dedicated model and view.

Minimum review record:

- changed files
- diff summary
- commands run
- test result
- risk notes
- recommended next action
- status: pending, approved, rejected, changes_requested

Initial review can be deterministic:

```txt
git status --short
git diff --stat
git diff --name-only
configured test command output
```

Add LLM review only after this data pipeline is reliable.

## Permission Model

Visible session permission modes:

- Explore: read-only.
- Ask to Edit: writes and commands require approval.
- Execute: writes and commands are allowed inside policy.

Longer-term hierarchy:

```txt
Workspace permissions
-> Project permissions
-> Session permissions
```

Agents should request operations. They should not receive raw secrets.

## Secrets Model

Use OS keychain:

- macOS Keychain.
- Windows Credential Manager.
- Linux Secret Service or equivalent.

Principle:

```txt
Agents can request an operation that uses a secret.
Agents should not be able to read the raw secret.
```

## Event Model

Persist important events for resumability and auditability:

- `session.started`
- `agent.started`
- `agent.output`
- `agent.completed`
- `agent.failed`
- `git.diff.updated`
- `tests.started`
- `tests.completed`
- `review.created`
- `automation.triggered`

## Main Technical Risks

1. Building chat first and forcing the rest of the product through chat abstractions.
2. SQLite schema churn without migrations.
3. UI becoming a process manager.
4. Worktree cleanup deleting user work accidentally.
5. Secrets leaking into prompts or logs.
6. Building connector breadth before the core review loop works.
7. Treating automations as settings instead of first-class product objects.

## Recommended Build Order

1. Scaffold app and migrations.
2. Add workspace foundation.
3. Add projects and sessions.
4. Add sidebar shell around the final navigation model.
5. Add artifacts.
6. Add sources, skills, and automations skeletons.
7. Add repository sources.
8. Add coding workspaces and git commands.
9. Add review records and review UI.
10. Add first automation.
11. Add real agent adapter.
