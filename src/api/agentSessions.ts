import { invoke } from "@tauri-apps/api/core";
import type { AgentAdapter, AgentSession } from "../types/agentSession";

export function listAgentAdapters(): Promise<AgentAdapter[]> {
  return invoke<AgentAdapter[]>("list_agent_adapters");
}

export function listAgentSessions(codingWorkspaceId: string): Promise<AgentSession[]> {
  return invoke<AgentSession[]>("list_agent_sessions", { codingWorkspaceId });
}

export function startAgentSession(
  codingWorkspaceId: string,
  adapterId: string,
  accountId: string | null,
  prompt: string | null,
  mode: string | null,
  cols: number,
  rows: number,
): Promise<AgentSession> {
  return invoke<AgentSession>("start_agent_session", {
    codingWorkspaceId,
    adapterId,
    accountId,
    prompt,
    mode,
    cols,
    rows,
  });
}

export function getAgentSdkTranscript(id: string): Promise<string[]> {
  return invoke<string[]>("get_agent_sdk_transcript", { id });
}

export function writeAgentSession(id: string, data: string): Promise<void> {
  return invoke<void>("write_agent_session", { id, data });
}

export function resizeAgentSession(id: string, cols: number, rows: number): Promise<void> {
  return invoke<void>("resize_agent_session", { id, cols, rows });
}

export function stopAgentSession(id: string): Promise<void> {
  return invoke<void>("stop_agent_session", { id });
}

export function getAgentSessionTranscript(id: string): Promise<string> {
  return invoke<string>("get_agent_session_transcript", { id });
}
