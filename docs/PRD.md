# Product Requirements Document

## Product Name

Unified Agentic Workspace.

Short name: UAW.

## Objective

Build a local desktop workspace for research, planning, documentation, coding agents, isolated git worktrees, automations, and review-first development.

UAW should feel like:

```txt
Workspace + project memory + documents + sources + skills + automations + coding agents + review
```

not:

```txt
chat + projects + model selector
```

## Positioning

A desktop AI workspace where each workspace has its own sources, skills, automations, sessions, policies, artifacts, repositories, and review flow.

Short positioning:

```txt
The bridge between AI project work and agentic coding.
```

## Target Users

Primary users:

- Solo builders using AI for research, product planning, coding, review, and documentation.
- Engineers who want to run multiple AI coding tasks safely in parallel.
- Technical operators managing different workspaces, clients, repos, and source contexts.

Secondary users:

- Freelancers managing multiple client contexts.
- Product-minded builders turning briefs into implementation plans.
- Teams experimenting with local-first agentic development workflows.

## Problems To Solve

1. Chat history is not enough project memory.
2. Planning tools usually cannot execute code safely.
3. Coding agents often lose product context.
4. Parallel agent work is hard to inspect, compare, and review.
5. Generated code is easy to create but expensive to trust.
6. Sources, skills, automations, permissions, and secrets need workspace-level boundaries.
7. Research artifacts should be dispatchable into coding tasks.

## Product Principles

- Workspace is the top-level environment boundary.
- Research mode creates durable artifacts: briefs, specs, decisions, tasks, docs.
- Coding mode consumes artifacts and produces diffs, test results, reviews, and implementation summaries.
- Dispatch is a first-class action from plans/specs/tasks to agent sessions.
- Review is first-class and visible in navigation.
- Automations are first-class and workspace-scoped.
- Agents operate under explicit permissions.
- Local-first storage and execution are preferred for the MVP.

## Core Concepts

### Workspace

A long-lived environment boundary. It owns sessions, projects, sources, skills, automations, settings, policies, secrets, artifacts, reviews, and connected repositories.

Examples:

- Personal
- Client workspace
- Product workspace
- Open source workspace

### Project

A concrete initiative inside a workspace.

Project modes:

- `research`: planning, synthesis, writing, docs.
- `code`: repo-backed implementation.
- `mixed`: both research artifacts and coding sessions.

### Session

An individual chat, document task, coding agent run, review task, or terminal task.

Session statuses:

- `backlog`
- `todo`
- `running`
- `worktree-created`
- `agent-running`
- `tests-running`
- `review-agent-running`
- `needs-review`
- `done`
- `merged`
- `discarded`
- `cancelled`
- `archived`
- `flagged`

### Source

Something agents can read from or interact with.

Examples:

- local folder
- git repository
- MCP server
- API
- issue tracker
- docs
- web
- Notion
- GitHub
- Linear

### Skill

A named workflow with instructions, required sources, permissions, and output type.

Examples:

- Write PRD
- Summarize research
- Convert research to MVP
- Implement issue
- Review diff
- Generate test plan
- Prepare PR summary

### Automation

A scheduled, event-based, or agentic workflow scoped to a workspace.

The first required automation:

```txt
When a coding agent finishes:
1. collect the diff
2. run tests if configured
3. run review agent
4. summarize risks
5. move session to Needs Review
```

### Artifact

A durable output independent of chat history.

Examples:

- PRD
- technical plan
- decision record
- research note
- implementation checklist
- review report
- markdown documentation

### Coding Workspace

An isolated git worktree and branch created for one coding task or agent run.

### Review

A structured surface for judging completed work. It should include changed files, diff summary, command output, test result, risk notes, and accept/discard actions.

## MVP Scope

### P0 Requirements

1. User can create and switch workspaces.
2. Workspaces scope projects, sessions, sources, skills, automations, artifacts, repositories, reviews, and settings.
3. User can create research, code, or mixed projects.
4. User can create sessions with statuses.
5. User can create and edit markdown artifacts.
6. User can attach a local repository as a source.
7. User can create a coding session for a repo-backed project.
8. UAW can create an isolated git worktree for a coding session.
9. UAW can collect changed files and a diff for a coding session.
10. UAW can create a Needs Review record.
11. User can accept, keep, discard, or mark a coding workspace done.
12. UAW exposes Sources, Skills, Automations, and Reviews in the sidebar.

### P1 Requirements

1. Dispatch a markdown artifact into one or more coding tasks.
2. Run multiple coding sessions in parallel.
3. Launch a real agent process through an adapter.
4. Stream agent logs and command output into the UI.
5. Run configured test commands after agent completion.
6. Run an automated review agent on final diff.
7. Store implementation summaries, commands run, tests run, files changed, and unresolved issues.

### P2 Requirements

1. MCP source management.
2. OS keychain-backed secret storage.
3. Connector setup for Linear, GitHub, Notion, and Google Drive.
4. Repository semantic search or knowledge graph.
5. Automation presets and templates.
6. Cross-workspace search.
7. Cloud sync or collaboration.

## Main Workflows

### Research To Spec

```txt
Create workspace
-> create research project
-> chat/research
-> save markdown artifact
-> capture decisions
-> create implementation tasks
```

### Spec To Coding

```txt
Select artifact or task list
-> Dispatch to agents
-> choose repo/source
-> create coding sessions
-> create worktrees
-> run agents
```

### Coding To Review

```txt
Agent completes
-> collect diff
-> run checks
-> generate review summary
-> move session to Needs Review
-> inspect diff and risk notes
-> accept, modify, keep branch, or discard
```

### Workspace Configuration

```txt
Create workspace
-> choose workspace type
-> add sources
-> enable skills
-> configure automations
-> set policies
-> start first session
```

## Navigation Requirements

The sidebar should be shaped around the core product model:

```txt
Workspace switcher

New Session

Inbox / All Sessions
  Backlog
  Todo
  Running
  Needs Review
  Done
  Cancelled
  Archived
  Flagged

Projects
  Research
  Code
  Mixed

Sources
  APIs
  MCPs
  Local Folders
  Git Repositories
  Issue Trackers
  Docs

Skills
  Research
  Writing
  Coding
  Review
  Custom

Automations
  Scheduled
  Event-based
  Agentic

Reviews
  Pending
  Approved
  Rejected

Settings
```

## MVP Acceptance Criteria

The MVP is useful when a user can:

1. Create a workspace for a real repo.
2. Attach the repo as a source.
3. Create a code project.
4. Start a coding session from a task.
5. Create an isolated git worktree.
6. Run or manually complete work in the worktree.
7. Return to UAW and see changed files, diff, status, and review summary.
8. Move the session to Needs Review.
9. Accept or discard the worktree outcome.
10. Preserve project memory and artifacts for later sessions.

## Non-Goals For First MVP

- Team collaboration.
- Cloud sync.
- Mobile app.
- Full connector marketplace.
- Full model-provider marketplace.
- Visual workflow builder.
- Complex RAG.
- Custom plugin SDK.
- Computer-use GUI automation.
- Full repository knowledge graph.

## Open Product Questions

1. Which first real agent adapter should ship: Codex CLI, Claude Code CLI, Anthropic API, or a manual adapter?
2. Should the first review agent be deterministic, LLM-based, or both?
3. Should document artifacts be database-backed markdown first or file-backed markdown first?
4. How much worktree cleanup should be automated after accept/discard?
5. Which integration should come first after local repos: GitHub, Linear, Notion, or MCP?

