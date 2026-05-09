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
- Model providers: Anthropic, OpenAI, Google AI Studio, plus subscription-OAuth flows (Claude Max, ChatGPT Plus, GitHub Copilot)

### Why Tauri, not Electron

The frontend is intentionally thin: lists, forms, a markdown editor, a diff viewer, and a status board. That work does not justify shipping a full Chromium runtime per app instance.

Tauri is preferred because:

- Smaller binaries and lower memory than Electron.
- Rust backend is a better fit for filesystem, git CLI, PTY/process management, and OS keychain access.
- Process and permission boundaries are cleaner without a Node runtime hosting the app shell.
- Long-running agents, worktrees, and git operations belong in a backend service, not the renderer.

Reference products like [Craft Agents](https://github.com/craft-ai-agents/craft-agents-oss) ship on Electron + Bun + TypeScript end-to-end. UAW deliberately splits along the OS boundary instead.

## Reference Products

- [Craft Agents (OSS)](https://github.com/craft-ai-agents/craft-agents-oss) — closest existing product to UAW. Multi-provider, MCP sources, skills, automations, permission modes, multi-file diff. Validates the shape of the product. Differentiates from UAW on stack (Electron) and emphasis (document-centric vs worktree/review-centric).
