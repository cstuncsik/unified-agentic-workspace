import { ref } from "vue";
import { listRepositoryBranches } from "../api/repositories";
import { useRepositoriesStore } from "../stores/repositories";

/** Loads a repository's branches and tracks the chosen base branch. */
export function useRepositoryBranches() {
  const repositories = useRepositoriesStore();
  const branches = ref<string[]>([]);
  const baseBranch = ref("");
  const loading = ref(false);
  let token = 0;

  /** Select a repository (id or "") and load its branches; defaults the base. */
  async function selectRepo(repoId: string) {
    const t = ++token;
    branches.value = [];
    baseBranch.value = "";
    if (!repoId) return;
    loading.value = true;
    try {
      const result = await listRepositoryBranches(repoId);
      if (t !== token) return;
      branches.value = result;
      const repo = repositories.list.find((r) => r.id === repoId);
      baseBranch.value =
        repo && result.includes(repo.default_branch) ? repo.default_branch : (result[0] ?? "");
    } catch (e) {
      // Swallow errors from a superseded fetch (a newer selectRepo started);
      // surface only the current one to the caller (which toasts it).
      if (t !== token) return;
      throw e;
    } finally {
      if (t === token) loading.value = false;
    }
  }

  return { branches, baseBranch, loading, selectRepo };
}
