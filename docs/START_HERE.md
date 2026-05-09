# Start Here

This is the starting context for UAW: Unified Agentic Workspace.

UAW is a greenfield local desktop product. It should be designed around workspaces, durable project memory, document artifacts, coding sessions, isolated git worktrees, automations, and review workflows from day one.

## Read Order

1. `docs/PRD.md` for the product definition and MVP scope.
2. `docs/TECHNICAL_ARCHITECTURE.md` for the recommended architecture.
3. `docs/IMPLEMENTATION_ROADMAP.md` for the build sequence.
4. `README.md` for the short project overview.

## Product North Star

Build a desktop AI workspace where each workspace has its own sources, skills, automations, sessions, policies, artifacts, repositories, and review flow.

The product should not feel like a generic AI chat client. It should feel like an operating environment for AI-assisted knowledge work and AI-assisted software work.

Core workflow:

```txt
Research -> plan/spec -> dispatch to coding sessions -> isolated implementation -> automated review -> accept/discard -> docs updated
```

## Naming

Product name:

```txt
Unified Agentic Workspace
```

Abbreviation:

```txt
UAW
```

Use UAW as the internal product and repo name. Before public launch, check naming conflicts and search visibility because "UAW" is also a widely used abbreviation outside software.

## First Useful Vertical Slice

Build this first:

```txt
Create workspace
-> attach local repo
-> create code project
-> start coding session
-> create git worktree
-> collect diff
-> run basic review
-> move session to Needs Review
-> accept/discard
```

For the first implementation, the actual coding agent can be manual or simulated. The first real product value is making the workspace, worktree, diff, review, and status loop persistent and visible.

## Initial Stack Decision

Recommended stack:

- Tauri desktop shell.
- Vue 3 + TypeScript frontend.
- Rust backend services exposed through Tauri commands.
- SQLite local database.
- OS keychain for secrets.
- Git CLI for repository and worktree operations.
- Adapter interface for agents.

This stack gives UAW a strong local-first foundation while keeping process, file, git, and permission boundaries close to the operating system.

## Domain Model To Start With

Build the app around these entities:

- `Workspace`
- `Project`
- `Session`
- `Source`
- `Skill`
- `Automation`
- `Artifact`
- `RepositorySource`
- `CodingWorkspace`
- `Review`
- `SessionEvent`
- `PermissionPolicy`

Do not start with "chat" as the main domain object. Chat is one surface inside a session, not the whole product.

## First Development Prompt

Use this prompt to begin implementation:

```txt
Read README.md, docs/START_HERE.md, docs/PRD.md, docs/TECHNICAL_ARCHITECTURE.md, and docs/IMPLEMENTATION_ROADMAP.md.

Scaffold the UAW app using Tauri, Vue 3, TypeScript, Rust, and SQLite.

Start with milestone 1: workspace foundation. Add the initial app shell, SQLite setup, Workspace model, workspace CRUD commands, frontend workspace store, and a workspace switcher. Keep the UI minimal but shaped around the final product navigation.
```

## Immediate Next Tasks

1. Scaffold the Tauri + Vue 3 + TypeScript app.
2. Add SQLite initialization and migration support.
3. Add `workspaces`.
4. Add workspace-scoped `projects`.
5. Add workspace-scoped `sessions`.
6. Add the sidebar shell with Workspaces, Sessions, Projects, Sources, Skills, Automations, Reviews, Settings.
7. Add markdown artifacts.
8. Add repository sources.
9. Add git worktree commands.
10. Add review records.
11. Add the first completion automation.

## Guardrails

- Do not build every reference-app feature at once.
- Do not make sources, skills, and automations global-only.
- Do not bury review inside chat.
- Do not give agents raw secrets in prompts or logs.
- Do not let the UI directly manage long-running processes.
- Do not add cloud collaboration before the local workflow works.
