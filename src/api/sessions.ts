import { invoke } from "@tauri-apps/api/core";
import type { Session, SessionMode, SessionStatus } from "../types/session";

export function listSessions(workspaceId: string, projectId?: string): Promise<Session[]> {
  return invoke<Session[]>("list_sessions", { workspaceId, projectId });
}

export function getSession(id: string): Promise<Session | null> {
  return invoke<Session | null>("get_session", { id });
}

export interface CreateSessionInput {
  workspaceId: string;
  title: string;
  mode: SessionMode;
  projectId?: string;
  status?: SessionStatus;
}

export function createSession(input: CreateSessionInput): Promise<Session> {
  return invoke<Session>("create_session", { ...input });
}

export function updateSession(
  id: string,
  title: string,
  summary?: string,
): Promise<Session | null> {
  return invoke<Session | null>("update_session", { id, title, summary });
}

export function updateSessionStatus(id: string, status: SessionStatus): Promise<Session | null> {
  return invoke<Session | null>("update_session_status", { id, status });
}

export function deleteSession(id: string): Promise<boolean> {
  return invoke<boolean>("delete_session", { id });
}
