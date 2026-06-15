import { invoke } from "@tauri-apps/api/core";
import type { Review } from "../types/review";

export function listReviews(workspaceId: string): Promise<Review[]> {
  return invoke<Review[]>("list_reviews", { workspaceId });
}

export function getReview(id: string): Promise<Review | null> {
  return invoke<Review | null>("get_review", { id });
}

export function createReviewForCodingWorkspace(codingWorkspaceId: string): Promise<Review> {
  return invoke<Review>("create_review_for_coding_workspace", { codingWorkspaceId });
}

export function updateReviewStatus(id: string, status: string): Promise<Review | null> {
  return invoke<Review | null>("update_review_status", { id, status });
}
