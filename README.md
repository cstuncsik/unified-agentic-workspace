# UAW

Unified Agentic Workspace.

UAW is a local desktop workspace for research, planning, documentation, coding agents, git worktrees, automations, and review-first development.

The product goal is to bridge two workflows that are usually split across separate tools:

```txt
Research / planning / documentation
+
Parallel agentic coding / review / shipping
```

## Product Thesis

Each workspace should behave like a complete agent environment with its own:

- sessions
- projects
- sources
- skills
- automations
- policies
- artifacts
- repositories
- reviews

The core workflow:

```txt
Idea
-> research
-> spec
-> dispatch to coding sessions
-> isolated worktrees
-> automated review
-> accept / discard
-> update docs
```

## Start Here

- [Start Here](docs/START_HERE.md)
- [Product Requirements Document](docs/PRD.md)
- [Technical Architecture](docs/TECHNICAL_ARCHITECTURE.md)
- [Implementation Roadmap](docs/IMPLEMENTATION_ROADMAP.md)

## Initial Build Strategy

Start with the first useful vertical slice:

```txt
Create workspace
-> attach local repo
-> create code project
-> start coding session
-> create git worktree
-> collect diff
-> create review
-> accept / discard
```

Avoid building the connector marketplace, plugin system, complex RAG, cloud sync, or team features before this core loop works.

## Recommended Initial Stack

- Desktop shell: Tauri
- Frontend: Vue 3 + TypeScript
- Backend: Rust sidecar through Tauri commands
- Storage: SQLite
- Secrets: OS keychain
- Git operations: git CLI
- Agent execution: adapter interface, starting with manual/Codex/Claude Code adapter
