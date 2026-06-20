export interface AgentCapabilities {
  streaming: boolean;
  tool_use: boolean;
  mcp: boolean;
  file_edits: boolean;
  shell_commands: boolean;
  multi_turn: boolean;
}

export interface AgentAdapter {
  id: string;
  name: string;
  program: string;
  args: string[];
  provider: string | null;
  kind: string; // "pty" | "sdk"
  requires_account: boolean;
  capabilities: AgentCapabilities;
}

/** One streamed Claude Agent SDK message (already redacted by the backend). */
export interface SdkEvent {
  type: "assistant" | "tool" | "result" | "error";
  text?: string;
  name?: string;
  summary?: string;
  message?: string;
  status?: string;
}

export interface AgentSession {
  id: string;
  workspace_id: string;
  coding_workspace_id: string;
  adapter_id: string;
  command: string;
  status: string; // running | exited | stopped | failed
  exit_code: number | null;
  transcript_path: string;
  account_id: string | null;
  model_id: string | null;
  kind: string; // "pty" | "sdk"
  mode: string | null; // "plan" | "edit" for sdk; null for pty
  created_at: string;
  updated_at: string;
}

/** Streamed PTY output (raw bytes as a number array). */
export interface AgentOutput {
  session_id: string;
  bytes: number[];
}

export interface AgentExit {
  session_id: string;
  status: string;
  exit_code: number | null;
}

/** One streamed Claude Agent SDK event line (a redacted NDJSON object). */
export interface AgentSdkEvent {
  session_id: string;
  line: string;
}
