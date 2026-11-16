<script lang="ts">
  import { onMount } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';

  import FileList from './components/FileList.svelte';
  import JjRevisionSelector from './components/JjRevisionSelector.svelte';
  import JjRevisionTree from './components/JjRevisionTree.svelte';
  import {
    type DiffFile,
    type DiffResponse,
    type JjChange,
    type JjCommit,
    type JjGraphRow,
    type ViewedFileRecord,
  } from '../types/diff';
  import {
    approveJjRevision,
    fetchDiff,
    fetchGeneratedStatus,
    fetchJjChanges,
    fetchJjCommitsForChange,
    fetchJjDiff,
    fetchJjStatus,
  } from './services/api';
  import { fetchClientSettings } from './services/userSettings';
  import {
    APPEARANCE_STORAGE_KEY,
    applyResolvedTheme,
    type ColorVisionMode,
    getStoredAppearanceSettings,
    resolveThemePreference,
    type ThemePreference,
  } from './utils/appearanceTheme';
  import { normalizeAutoViewedPatterns } from './utils/autoViewedPatterns';
  import { formatStatusBadge } from './utils/diffUtils';
  import { getFileElementId } from './utils/domUtils';
  import { makeStandaloneCommitRow } from './utils/jjGraph';
  import { commitIdsMatch, pickInitialChange } from './utils/jjSelection';
  import { connectLiveWatch } from './utils/liveWatch';
  import {
    hydrateViewedState,
    setMultipleFilesViewed,
    toggleSingleFileViewed,
    type ViewedContextParams,
  } from './utils/viewedState';

  const SIDEBAR_OPEN_STORAGE_KEY = 'difit.sidebarOpen';

  function getInitialSidebarOpen(): boolean {
    if (typeof window === 'undefined') return true;
    try {
      const stored = window.localStorage.getItem(SIDEBAR_OPEN_STORAGE_KEY);
      if (stored === 'false') return false;
      if (stored === 'true') return true;
    } catch {
      // Ignore storage access errors.
    }
    return true;
  }

  function getInitialThemePreference(): ThemePreference {
    return getStoredAppearanceSettings()?.theme ?? 'auto';
  }

  function getInitialColorVision(): ColorVisionMode {
    return getStoredAppearanceSettings()?.colorVision ?? 'normal';
  }

  function persistThemePreference(nextTheme: ThemePreference) {
    if (typeof window === 'undefined') return;
    try {
      const raw = window.localStorage.getItem(APPEARANCE_STORAGE_KEY);
      const parsed: unknown = raw ? JSON.parse(raw) : null;
      const base =
        parsed && typeof parsed === 'object' && !Array.isArray(parsed)
          ? (parsed as Record<string, unknown>)
          : {};
      window.localStorage.setItem(
        APPEARANCE_STORAGE_KEY,
        JSON.stringify({ ...base, theme: nextTheme }),
      );
    } catch {
      // Ignore storage access errors.
    }
  }

  function isAbortError(err: unknown): boolean {
    return (
      typeof err === 'object' &&
      err !== null &&
      'name' in err &&
      (err as { name?: unknown }).name === 'AbortError'
    );
  }

  function buildViewedContext(diff: DiffResponse): ViewedContextParams {
    return {
      baseCommitish: diff.baseCommitish ?? diff.requestedBaseCommitish ?? 'HEAD^',
      targetCommitish: diff.targetCommitish ?? diff.requestedTargetCommitish ?? 'HEAD',
      currentCommitHash: diff.commit,
      repositoryId: diff.repositoryId,
      baseMode: diff.requestedBaseMode,
    };
  }

  function selectTargetCommitForChange(
    commits: JjCommit[],
    changeHeadCommitId: string,
    preferredCommitId?: string,
  ): string {
    if (preferredCommitId) {
      const explicitMatch = commits.find((c) => commitIdsMatch(c.commitId, preferredCommitId));
      if (explicitMatch) {
        return explicitMatch.commitId;
      }
    }
    const headMatch = commits.find((c) => commitIdsMatch(c.commitId, changeHeadCommitId));
    if (headMatch) {
      return headMatch.commitId;
    }
    return commits[commits.length - 1]?.commitId ?? changeHeadCommitId;
  }

  let loading = $state(true);
  let error = $state<string | null>(null);
  let isJjRepo = $state(false);
  let jjChanges = $state<JjChange[]>([]);
  let jjGraphRows = $state<JjGraphRow[]>([]);
  let selectedChangeId = $state('');
  let jjCommits = $state<JjCommit[]>([]);
  let jjTargetCommitId = $state('');
  let jjBaseCommitId = $state('parent');
  let ignoreRebase = $state(true);
  let loadingJjChanges = $state(false);
  let loadingJjCommits = $state(false);
  let approving = $state(false);

  let diffData = $state<DiffResponse | null>(null);
  let ignoreWhitespace = $state(true);
  let sidebarOpen = $state(getInitialSidebarOpen());
  let themePreference = $state<ThemePreference>(getInitialThemePreference());
  let colorVision = $state<ColorVisionMode>(getInitialColorVision());

  let revQueryError = $state<string | null>(null);
  let watchReloadPending = $state(false);
  let isReloading = $state(false);

  let autoViewedPatterns = $state<string[]>([]);
  let viewedRecords = $state<ViewedFileRecord[]>([]);
  let viewedFiles = new SvelteSet<string>();
  let changedSinceViewedFiles = new SvelteSet<string>();
  let collapsedFiles = new SvelteSet<string>();
  let manuallyToggledPaths = new SvelteSet<string>();
  let fileHashesByPath = new Map<string, string>();

  let activeRequestController: AbortController | null = null;
  let activeRequestId = 0;
  let activeGeneratedController: AbortController | null = null;
  let diffRevisionId = 0;
  let viewedMutationChain: Promise<void> = Promise.resolve();

  function enqueueViewedMutation(task: () => Promise<void>): Promise<void> {
    const next = viewedMutationChain.then(task, task);
    viewedMutationChain = next;
    return next;
  }

  function startNewRequest(): { signal: AbortSignal; requestId: number } {
    activeRequestController?.abort();
    const controller = new AbortController();
    activeRequestController = controller;
    const requestId = ++activeRequestId;
    isReloading = false;
    loadingJjChanges = false;
    loadingJjCommits = false;
    return { signal: controller.signal, requestId };
  }

  const activeJjTargetIndex = $derived(
    jjCommits.findIndex((c) => commitIdsMatch(c.commitId, jjTargetCommitId)),
  );
  const activeJjTargetCommit = $derived(
    activeJjTargetIndex >= 0 ? jjCommits[activeJjTargetIndex] : undefined,
  );

  const totalAdditions = $derived(
    diffData?.files.reduce((sum, file) => sum + file.additions, 0) ?? 0,
  );

  const totalDeletions = $derived(
    diffData?.files.reduce((sum, file) => sum + file.deletions, 0) ?? 0,
  );

  $effect(() => {
    const resolved = resolveThemePreference(themePreference);
    applyResolvedTheme(resolved, colorVision);

    if (themePreference !== 'auto' || typeof window === 'undefined') {
      return;
    }

    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
    const handleMediaChange = (event: MediaQueryListEvent) => {
      applyResolvedTheme(event.matches ? 'dark' : 'light', colorVision);
    };
    mediaQuery.addEventListener('change', handleMediaChange);
    return () => {
      mediaQuery.removeEventListener('change', handleMediaChange);
    };
  });

  $effect(() => {
    if (typeof window === 'undefined') return;
    try {
      window.localStorage.setItem(SIDEBAR_OPEN_STORAGE_KEY, String(sidebarOpen));
    } catch {
      // Ignore storage access errors.
    }
  });

  function applyDiffResponse(nextDiff: DiffResponse) {
    diffData = nextDiff;
    const revisionId = ++diffRevisionId;

    activeGeneratedController?.abort();
    const genController = new AbortController();
    activeGeneratedController = genController;

    manuallyToggledPaths.clear();
    collapsedFiles.clear();
    viewedRecords = [];
    fileHashesByPath = new Map();
    viewedFiles.clear();
    changedSinceViewedFiles.clear();
    for (const file of nextDiff.files) {
      if (file.isGenerated) {
        collapsedFiles.add(file.path);
      }
    }

    const context = buildViewedContext(nextDiff);
    const isStdin =
      nextDiff.targetCommitish === 'stdin' || nextDiff.baseCommitish === 'stdin';
    const ref = nextDiff.targetCommitish || 'HEAD';

    void enqueueViewedMutation(async () => {
      if (genController.signal.aborted || revisionId !== diffRevisionId) return;
      try {
        const hydrated = await hydrateViewedState(context, nextDiff.files, autoViewedPatterns);
        if (genController.signal.aborted || revisionId !== diffRevisionId) return;

        viewedRecords = hydrated.records;
        fileHashesByPath = hydrated.hashesByPath;

        viewedFiles.clear();
        for (const path of hydrated.viewedPaths) {
          viewedFiles.add(path);
          if (!manuallyToggledPaths.has(path)) {
            collapsedFiles.add(path);
          }
        }

        changedSinceViewedFiles.clear();
        for (const path of hydrated.changedSinceViewedPaths) {
          changedSinceViewedFiles.add(path);
        }
      } catch {
        // Keep UI responsive even if storage/crypto is unavailable.
      }

      if (isStdin || genController.signal.aborted || revisionId !== diffRevisionId) return;

      for (const file of nextDiff.files) {
        if (
          file.path === 'Commit Message' ||
          file.status === 'deleted' ||
          file.isGenerated === true
        ) {
          continue;
        }

        void fetchGeneratedStatus(file.path, ref, genController.signal)
          .then((status) => {
            if (genController.signal.aborted || revisionId !== diffRevisionId) return;
            if (!status.isGenerated) return;

            return enqueueViewedMutation(async () => {
              if (genController.signal.aborted || revisionId !== diffRevisionId || !diffData) {
                return;
              }

              const updatedFiles = diffData.files.map((candidate) =>
                candidate.path === file.path ? { ...candidate, isGenerated: true } : candidate,
              );
              diffData = { ...diffData, files: updatedFiles };

              if (!manuallyToggledPaths.has(file.path)) {
                collapsedFiles.add(file.path);
                const updatedFile = updatedFiles.find((f) => f.path === file.path);
                if (updatedFile && !viewedFiles.has(file.path)) {
                  const nextRecords = await setMultipleFilesViewed(
                    context,
                    viewedRecords,
                    [updatedFile],
                    true,
                    fileHashesByPath,
                  );
                  if (genController.signal.aborted || revisionId !== diffRevisionId) return;
                  viewedRecords = nextRecords;
                  viewedFiles.add(file.path);
                  changedSinceViewedFiles.delete(file.path);
                }
              }
            });
          })
          .catch(() => {
            // Ignore generated-status failures and keep fast path.
          });
      }
    });
  }

  async function bootstrapApp(signal: AbortSignal, requestId: number) {
    loading = true;
    error = null;
    revQueryError = null;

    try {
      const [clientSettings, jjStatus, initialDiff] = await Promise.all([
        fetchClientSettings(),
        fetchJjStatus(signal),
        fetchDiff({ ignoreWhitespace, signal }),
      ]);
      if (signal.aborted || requestId !== activeRequestId) return;

      if (clientSettings) {
        autoViewedPatterns = normalizeAutoViewedPatterns(clientSettings.autoViewedPatterns);
      }

      const isStdinDiff =
        initialDiff.targetCommitish === 'stdin' || initialDiff.baseCommitish === 'stdin';
      const useJjMode = jjStatus.isJj && !isStdinDiff;
      isJjRepo = useJjMode;

      if (useJjMode) {
        loadingJjChanges = true;
        const { changes, graphRows } = await fetchJjChanges(undefined, signal);
        if (signal.aborted || requestId !== activeRequestId) return;

        jjChanges = changes;
        jjGraphRows = graphRows;
        loadingJjChanges = false;

        const initialChange = pickInitialChange(changes, initialDiff.targetCommitish);
        if (initialChange) {
          loadingJjCommits = true;
          const commits = await fetchJjCommitsForChange(initialChange.changeId, signal);
          if (signal.aborted || requestId !== activeRequestId) return;

          const targetCommit = selectTargetCommitForChange(
            commits,
            initialChange.commitId,
            initialDiff.targetCommitish,
          );
          const jjDiff = await fetchJjDiff({
            target: targetCommit,
            ignoreRebase,
            signal,
          });
          if (signal.aborted || requestId !== activeRequestId) return;

          selectedChangeId = initialChange.changeId;
          jjCommits = commits;
          jjTargetCommitId = targetCommit;
          jjBaseCommitId = 'parent';
          loadingJjCommits = false;
          applyDiffResponse(jjDiff);
        } else {
          applyDiffResponse(initialDiff);
        }
      } else {
        applyDiffResponse(initialDiff);
      }
    } catch (err) {
      if (isAbortError(err) || signal.aborted || requestId !== activeRequestId) return;
      error = err instanceof Error ? err.message : 'Failed to load diff';
    } finally {
      if (!signal.aborted && requestId === activeRequestId) {
        loading = false;
        loadingJjChanges = false;
        loadingJjCommits = false;
      }
    }
  }

  async function preserveOutOfGraphChange(
    changes: JjChange[],
    graphRows: JjGraphRow[],
    changeId: string,
    signal?: AbortSignal,
    commitsOverride?: JjCommit[] | null,
  ): Promise<{ changes: JjChange[]; graphRows: JjGraphRow[] }> {
    if (!changeId || changes.some((c) => c.changeId === changeId)) {
      return { changes, graphRows };
    }

    let preservedChange: JjChange | undefined;
    let preservedRow: JjGraphRow | undefined;

    try {
      const queried = await fetchJjChanges(changeId, signal);
      if (signal?.aborted) {
        return { changes, graphRows };
      }
      const found = queried.changes.find((c) => c.changeId === changeId) ?? queried.changes[0];
      if (found) {
        preservedChange = found;
        preservedRow = queried.graphRows[0] ?? makeStandaloneCommitRow(found);
      }
    } catch (err) {
      if (isAbortError(err) || signal?.aborted) {
        throw err;
      }
      // Fall back to existing queried change metadata below.
    }

    if (!preservedChange) {
      const existing = jjChanges.find((c) => c.changeId === changeId);
      if (existing) {
        const matchedCommit = commitsOverride?.find((c) =>
          commitIdsMatch(c.commitId, existing.commitId),
        );
        preservedChange = matchedCommit
          ? {
              ...existing,
              approved: matchedCommit.approved,
              cleanRebaseApproved: matchedCommit.cleanRebaseApproved,
              commentCount: matchedCommit.commentCount,
              unresolvedCommentCount: matchedCommit.unresolvedCommentCount,
            }
          : existing;
        preservedRow =
          jjGraphRows.find((r) => r.type === 'commit' && r.change.changeId === changeId) ??
          makeStandaloneCommitRow(preservedChange);
      }
    }

    if (!preservedChange) {
      return { changes, graphRows };
    }

    return {
      changes: [preservedChange, ...changes],
      graphRows: [preservedRow ?? makeStandaloneCommitRow(preservedChange), ...graphRows],
    };
  }

  async function refreshJjMetadata(expectedChangeId: string) {
    const [changesData, commitsData] = await Promise.all([
      fetchJjChanges(),
      expectedChangeId ? fetchJjCommitsForChange(expectedChangeId) : Promise.resolve(null),
    ]);

    const activeChangeId = selectedChangeId;
    const effectiveCommitsForActive = activeChangeId === expectedChangeId ? commitsData : null;
    const merged = await preserveOutOfGraphChange(
      changesData.changes,
      changesData.graphRows,
      activeChangeId,
      undefined,
      effectiveCommitsForActive,
    );

    jjChanges = merged.changes;
    jjGraphRows = merged.graphRows;
    if (commitsData && selectedChangeId === expectedChangeId) {
      jjCommits = commitsData;
    }
  }

  async function refreshJjMetadataInBackground() {
    if (!isJjRepo) return;
    try {
      await refreshJjMetadata(selectedChangeId);
    } catch {
      // Ignore background metadata refresh errors.
    }
  }

  onMount(() => {
    const { signal, requestId } = startNewRequest();
    void bootstrapApp(signal, requestId);

    const disconnectLiveWatch = connectLiveWatch({
      onReloadSignal: () => {
        watchReloadPending = true;
      },
      onCommentsChanged: () => {
        void refreshJjMetadataInBackground();
      },
    });

    return () => {
      disconnectLiveWatch();
      activeRequestController?.abort();
      activeGeneratedController?.abort();
    };
  });

  async function handleSelectJjChange(changeId: string) {
    if (changeId === selectedChangeId && diffData) return;
    const change = jjChanges.find((c) => c.changeId === changeId);
    if (!change) return;

    const { signal, requestId } = startNewRequest();
    revQueryError = null;
    error = null;
    loadingJjCommits = true;

    try {
      const commits = await fetchJjCommitsForChange(changeId, signal);
      if (signal.aborted || requestId !== activeRequestId) return;

      const targetCommit = selectTargetCommitForChange(commits, change.commitId);
      const nextDiff = await fetchJjDiff({
        target: targetCommit,
        ignoreRebase,
        signal,
      });
      if (signal.aborted || requestId !== activeRequestId) return;

      selectedChangeId = changeId;
      jjCommits = commits;
      jjTargetCommitId = targetCommit;
      jjBaseCommitId = 'parent';
      applyDiffResponse(nextDiff);
    } catch (err) {
      if (isAbortError(err) || signal.aborted || requestId !== activeRequestId) return;
      error = err instanceof Error ? err.message : 'Failed to load Jujutsu diff';
    } finally {
      if (!signal.aborted && requestId === activeRequestId) {
        loadingJjCommits = false;
      }
    }
  }

  async function handleQueryJjChange(query: string): Promise<boolean> {
    const trimmed = query.trim();
    if (!trimmed) return false;

    const { signal, requestId } = startNewRequest();
    revQueryError = null;
    error = null;

    try {
      const { changes, graphRows } = await fetchJjChanges(trimmed, signal);
      if (signal.aborted || requestId !== activeRequestId) return false;

      const found = changes[0];
      if (!found) {
        revQueryError = 'Revision not found';
        return false;
      }

      const commits = await fetchJjCommitsForChange(found.changeId, signal);
      if (signal.aborted || requestId !== activeRequestId) return false;

      const targetCommit = selectTargetCommitForChange(commits, found.commitId);
      const nextDiff = await fetchJjDiff({
        target: targetCommit,
        ignoreRebase,
        signal,
      });
      if (signal.aborted || requestId !== activeRequestId) return false;

      if (!jjChanges.some((c) => c.changeId === found.changeId)) {
        jjChanges = [found, ...jjChanges];
        const addedRow = graphRows[0] ?? makeStandaloneCommitRow(found);
        jjGraphRows = [addedRow, ...jjGraphRows];
      }
      selectedChangeId = found.changeId;
      jjCommits = commits;
      jjTargetCommitId = targetCommit;
      jjBaseCommitId = 'parent';
      applyDiffResponse(nextDiff);
      return true;
    } catch (err) {
      if (isAbortError(err) || signal.aborted || requestId !== activeRequestId) return false;
      revQueryError = err instanceof Error ? err.message : 'Query failed';
      return false;
    }
  }

  async function updateJjComparison(
    nextTarget: string,
    nextBase: string,
    nextIgnoreRebase: boolean,
  ) {
    const { signal, requestId } = startNewRequest();
    error = null;

    try {
      const nextDiff = await fetchJjDiff({
        target: nextTarget,
        base: nextBase === 'parent' ? undefined : nextBase,
        ignoreRebase: nextIgnoreRebase,
        signal,
      });
      if (signal.aborted || requestId !== activeRequestId) return;

      jjTargetCommitId = nextTarget;
      jjBaseCommitId = nextBase;
      ignoreRebase = nextIgnoreRebase;
      applyDiffResponse(nextDiff);
    } catch (err) {
      if (isAbortError(err) || signal.aborted || requestId !== activeRequestId) return;
      error = err instanceof Error ? err.message : 'Failed to update comparison diff';
    }
  }

  async function handleToggleApprove(commitId: string, currentApproved: boolean) {
    if (approving) return;
    approving = true;
    error = null;
    const changeAtClick = selectedChangeId;

    try {
      await approveJjRevision(commitId, currentApproved);
      await refreshJjMetadata(changeAtClick);
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to update approval';
    } finally {
      approving = false;
    }
  }

  async function handleReload() {
    if (isReloading) return;
    const { signal, requestId } = startNewRequest();
    isReloading = true;
    error = null;

    try {
      if (isJjRepo && selectedChangeId) {
        const currentChangeId = selectedChangeId;
        const changesData = await fetchJjChanges(undefined, signal);
        if (signal.aborted || requestId !== activeRequestId) return;

        if (!changesData.changes.some((c) => c.changeId === currentChangeId)) {
          try {
            const queried = await fetchJjChanges(currentChangeId, signal);
            if (signal.aborted || requestId !== activeRequestId) return;
            const found =
              queried.changes.find((c) => c.changeId === currentChangeId) ?? queried.changes[0];
            if (found) {
              changesData.changes = [found, ...changesData.changes];
              changesData.graphRows = [
                queried.graphRows[0] ?? makeStandaloneCommitRow(found),
                ...changesData.graphRows,
              ];
            }
          } catch (err) {
            if (isAbortError(err) || signal.aborted) throw err;
          }
        }

        const refreshedChange =
          changesData.changes.find((c) => c.changeId === currentChangeId) ??
          pickInitialChange(changesData.changes);

        if (refreshedChange) {
          const targetChangeId = refreshedChange.changeId;
          const switchedChange = targetChangeId !== currentChangeId;
          const commits = await fetchJjCommitsForChange(targetChangeId, signal);
          if (signal.aborted || requestId !== activeRequestId) return;

          const effectiveBase = switchedChange ? 'parent' : jjBaseCommitId;
          const nextTarget =
            effectiveBase === 'parent'
              ? selectTargetCommitForChange(commits, refreshedChange.commitId)
              : jjTargetCommitId;

          const nextDiff = await fetchJjDiff({
            target: nextTarget,
            base: effectiveBase === 'parent' ? undefined : effectiveBase,
            ignoreRebase,
            signal,
          });
          if (signal.aborted || requestId !== activeRequestId) return;

          jjChanges = changesData.changes;
          jjGraphRows = changesData.graphRows;
          selectedChangeId = targetChangeId;
          jjCommits = commits;
          jjBaseCommitId = effectiveBase;
          jjTargetCommitId = nextTarget;
          applyDiffResponse(nextDiff);
        } else {
          jjChanges = changesData.changes;
          jjGraphRows = changesData.graphRows;
        }
      } else {
        const nextDiff = await fetchDiff({
          ignoreWhitespace,
          base: diffData?.requestedBaseCommitish,
          target: diffData?.requestedTargetCommitish,
          baseMode: diffData?.requestedBaseMode,
          signal,
        });
        if (signal.aborted || requestId !== activeRequestId) return;
        applyDiffResponse(nextDiff);
      }
      watchReloadPending = false;
    } catch (err) {
      if (isAbortError(err) || signal.aborted || requestId !== activeRequestId) return;
      error = err instanceof Error ? err.message : 'Failed to reload diff';
    } finally {
      if (!signal.aborted && requestId === activeRequestId) {
        isReloading = false;
      }
    }
  }

  async function handleIgnoreWhitespaceChange(nextValue: boolean) {
    ignoreWhitespace = nextValue;
    if (isJjRepo) return;

    const { signal, requestId } = startNewRequest();
    error = null;

    try {
      const nextDiff = await fetchDiff({
        ignoreWhitespace: nextValue,
        base: diffData?.requestedBaseCommitish,
        target: diffData?.requestedTargetCommitish,
        baseMode: diffData?.requestedBaseMode,
        signal,
      });
      if (signal.aborted || requestId !== activeRequestId) return;
      applyDiffResponse(nextDiff);
    } catch (err) {
      if (isAbortError(err) || signal.aborted || requestId !== activeRequestId) return;
      error = err instanceof Error ? err.message : 'Failed to reload diff';
    }
  }

  function handleToggleFileViewed(file: DiffFile): Promise<void> {
    if (!diffData) return Promise.resolve();
    const currentRevisionId = diffRevisionId;
    manuallyToggledPaths.add(file.path);
    const context = buildViewedContext(diffData);

    return enqueueViewedMutation(async () => {
      if (currentRevisionId !== diffRevisionId) return;
      const result = await toggleSingleFileViewed(
        context,
        viewedRecords,
        file,
        fileHashesByPath.get(file.path),
      );
      if (currentRevisionId !== diffRevisionId) return;
      viewedRecords = result.records;
      fileHashesByPath.set(file.path, result.hash);

      if (result.isViewed) {
        viewedFiles.add(file.path);
        collapsedFiles.add(file.path);
        changedSinceViewedFiles.delete(file.path);
      } else {
        viewedFiles.delete(file.path);
        collapsedFiles.delete(file.path);
      }
    });
  }

  function handleToggleFolderViewed(folderFiles: DiffFile[], nextViewed: boolean): Promise<void> {
    if (!diffData || folderFiles.length === 0) return Promise.resolve();
    const currentRevisionId = diffRevisionId;
    for (const file of folderFiles) {
      manuallyToggledPaths.add(file.path);
    }
    const context = buildViewedContext(diffData);

    return enqueueViewedMutation(async () => {
      if (currentRevisionId !== diffRevisionId) return;
      const nextRecords = await setMultipleFilesViewed(
        context,
        viewedRecords,
        folderFiles,
        nextViewed,
        fileHashesByPath,
      );
      if (currentRevisionId !== diffRevisionId) return;
      viewedRecords = nextRecords;
      for (const file of folderFiles) {
        if (nextViewed) {
          viewedFiles.add(file.path);
          collapsedFiles.add(file.path);
          changedSinceViewedFiles.delete(file.path);
        } else {
          viewedFiles.delete(file.path);
          collapsedFiles.delete(file.path);
        }
      }
    });
  }

  function handleThemeChange(nextTheme: ThemePreference) {
    themePreference = nextTheme;
    persistThemePreference(nextTheme);
  }

  function toggleFileCollapsed(path: string) {
    manuallyToggledPaths.add(path);
    if (collapsedFiles.has(path)) {
      collapsedFiles.delete(path);
    } else {
      collapsedFiles.add(path);
    }
  }

  function scrollToFile(path: string) {
    const element = document.getElementById(getFileElementId(path));
    element?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  function retryBootstrap() {
    const { signal, requestId } = startNewRequest();
    void bootstrapApp(signal, requestId);
  }
</script>

{#if loading}
  <div class="flex items-center justify-center h-screen bg-github-bg-primary">
    <div class="text-github-text-secondary text-base">Loading diff...</div>
  </div>
{:else if error && !diffData}
  <div
    role="alert"
    class="flex flex-col items-center justify-center h-screen bg-github-bg-primary text-center gap-3 px-4"
  >
    <h2 class="text-github-danger text-2xl font-semibold">Error</h2>
    <p class="text-github-text-secondary text-base max-w-lg">{error}</p>
    <button
      type="button"
      onclick={retryBootstrap}
      class="px-3 py-1.5 text-xs font-medium rounded border border-github-border bg-github-bg-secondary text-github-text-primary hover:bg-github-bg-tertiary"
    >
      Retry
    </button>
  </div>
{:else if !diffData}
  <div class="flex flex-col items-center justify-center h-screen bg-github-bg-primary text-center gap-2">
    <h2 class="text-github-danger text-2xl font-semibold">No data</h2>
    <p class="text-github-text-secondary text-base">No diff data available</p>
  </div>
{:else}
  <div class="h-screen flex flex-col bg-github-bg-primary text-github-text-primary">
    <header class="bg-github-bg-secondary border-b border-github-border px-4 py-2.5 flex flex-wrap items-center justify-between gap-3">
      <div class="flex flex-wrap items-center gap-3 min-w-0">
        <button
          type="button"
          onclick={() => {
            sidebarOpen = !sidebarOpen;
          }}
          aria-expanded={sidebarOpen}
          aria-controls="sidebar-panel"
          aria-label="Toggle sidebar"
          title={sidebarOpen ? 'Collapse sidebar' : 'Expand sidebar'}
          class="px-2.5 py-1 text-xs font-medium rounded border border-github-border bg-github-bg-tertiary text-github-text-secondary hover:text-github-text-primary"
        >
          {sidebarOpen ? 'Hide Sidebar' : 'Show Sidebar'}
        </button>

        <span class="font-semibold text-sm tracking-tight text-github-text-primary">tellit</span>

        {#if isJjRepo}
          <JjRevisionSelector
            changes={jjChanges}
            graphRows={jjGraphRows}
            {selectedChangeId}
            commits={jjCommits}
            targetCommitId={jjTargetCommitId}
            baseCommitId={jjBaseCommitId}
            isRebaseDiff={diffData.isRebaseDiff}
            rebaseIgnored={diffData.rebaseIgnored}
            rebaseConflict={diffData.rebaseConflict}
            loadingChanges={loadingJjChanges}
            loadingCommits={loadingJjCommits}
            {approving}
            queryError={revQueryError}
            onClearQueryError={() => {
              revQueryError = null;
            }}
            onSelectChange={(changeId) => void handleSelectJjChange(changeId)}
            onQueryChange={(query) => handleQueryJjChange(query)}
            onSelectBase={(baseId) => void updateJjComparison(jjTargetCommitId, baseId, ignoreRebase)}
            onSelectTarget={(targetId) => void updateJjComparison(targetId, jjBaseCommitId, ignoreRebase)}
            onToggleIgnoreRebase={() => void updateJjComparison(jjTargetCommitId, jjBaseCommitId, !ignoreRebase)}
            onToggleApprove={(commitId, currentApproved) => void handleToggleApprove(commitId, currentApproved)}
          />
        {:else}
          <span class="text-xs text-github-text-secondary">
            Reviewing:
            <code class="bg-github-bg-tertiary px-1.5 py-0.5 rounded font-mono text-github-text-primary">
              {diffData.commit}
            </code>
          </span>
          <label class="inline-flex items-center gap-1.5 text-xs text-github-text-secondary cursor-pointer select-none">
            <input
              type="checkbox"
              checked={ignoreWhitespace}
              onchange={(event) => void handleIgnoreWhitespaceChange(event.currentTarget.checked)}
              class="rounded border-github-border"
            />
            <span>Ignore Whitespace</span>
          </label>
        {/if}

        <button
          type="button"
          onclick={() => void handleReload()}
          disabled={isReloading}
          class="px-2.5 py-1 text-xs font-medium rounded border transition-colors cursor-pointer disabled:opacity-60 {watchReloadPending
            ? 'bg-yellow-500/15 text-yellow-300 border-yellow-500/40 hover:bg-yellow-500/25'
            : 'bg-github-bg-tertiary text-github-text-secondary border-github-border hover:text-github-text-primary'}"
          title={watchReloadPending
            ? 'Repository changes detected: click to reload diff'
            : 'Reload diff'}
        >
          {isReloading ? 'Reloading...' : watchReloadPending ? 'Reload (updated)' : 'Reload'}
        </button>
      </div>

      <div class="flex flex-wrap items-center gap-4 text-xs text-github-text-secondary">
        <div class="flex items-center gap-2 font-mono" aria-label="Diff summary">
          <span>{viewedFiles.size}/{diffData.files.length} viewed</span>
          <span class="text-green-400">+{totalAdditions}</span>
          <span class="text-red-400">-{totalDeletions}</span>
        </div>

        <label class="inline-flex items-center gap-1.5">
          <span>Theme</span>
          <select
            aria-label="Theme preference"
            value={themePreference}
            onchange={(event) => handleThemeChange(event.currentTarget.value as ThemePreference)}
            class="bg-github-bg-tertiary border border-github-border rounded px-2 py-1 text-xs text-github-text-primary"
          >
            <option value="auto">Auto</option>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </select>
        </label>
      </div>
    </header>

    {#if diffData.rebaseConflict}
      <div class="bg-yellow-500/10 border-b border-yellow-500/30 px-4 py-2 text-xs text-yellow-300">
        Rebase conflict detected between patchsets: displaying full diff including upstream changes.
      </div>
    {/if}

    {#if isJjRepo && activeJjTargetCommit?.cleanRebaseApproved && !activeJjTargetCommit?.approved}
      <div class="bg-emerald-500/10 border-b border-emerald-500/30 px-4 py-2 text-xs text-emerald-300">
        Revision #{activeJjTargetIndex + 1} contains no code changes compared to approved revision #{activeJjTargetCommit.cleanRebaseSourceCommitNumber || 'earlier'} (clean rebase onto upstream).
      </div>
    {/if}

    <div class="flex flex-1 min-h-0 overflow-hidden">
      {#if sidebarOpen}
        <aside
          id="sidebar-panel"
          class="w-72 shrink-0 bg-github-bg-secondary border-r border-github-border flex flex-col min-h-0"
        >
          {#if isJjRepo}
            <JjRevisionTree
              changes={jjChanges}
              graphRows={jjGraphRows}
              {selectedChangeId}
              loading={loadingJjChanges}
              queryError={revQueryError}
              onClearQueryError={() => {
                revQueryError = null;
              }}
              onSelectChange={(changeId) => void handleSelectJjChange(changeId)}
              onQuerySubmit={(query) => handleQueryJjChange(query)}
            />
          {/if}

          <FileList
            files={diffData.files}
            {viewedFiles}
            {changedSinceViewedFiles}
            onScrollToFile={scrollToFile}
            onToggleViewed={(file) => void handleToggleFileViewed(file)}
            onToggleFolderViewed={(folderFiles, nextViewed) =>
              void handleToggleFolderViewed(folderFiles, nextViewed)}
          />
        </aside>
      {/if}

      <main class="flex-1 overflow-y-auto p-4 space-y-4">
        {#if error}
          <div
            role="alert"
            class="px-3 py-2 rounded-md border border-red-500/40 bg-red-500/10 text-xs text-red-300 flex items-center justify-between gap-3"
          >
            <span>{error}</span>
            <button
              type="button"
              onclick={() => {
                error = null;
              }}
              aria-label="Dismiss error"
              class="px-2 py-0.5 rounded border border-red-500/30 text-red-200 hover:bg-red-500/20 shrink-0"
            >
              Dismiss
            </button>
          </div>
        {/if}

        {#if diffData.files.length === 0}
          <div class="flex flex-col items-center justify-center p-16 text-center text-github-text-secondary min-h-[280px]">
            <div class="text-lg font-medium text-github-text-primary mb-2">
              No changes in this revision
            </div>
            <p class="text-sm text-github-text-muted max-w-md">
              Working copy or commit
              <code class="text-github-text-primary bg-github-bg-tertiary px-1.5 py-0.5 rounded text-xs font-mono">
                {diffData.commit}
              </code>
              has no modified files.
            </p>
          </div>
        {:else}
          {#each diffData.files as file (file.path)}
            {@const badge = formatStatusBadge(file.status)}
            {@const isCollapsed = collapsedFiles.has(file.path)}
            {@const isViewed = viewedFiles.has(file.path)}
            <article
              id={getFileElementId(file.path)}
              data-file-path={file.path}
              class="border border-github-border rounded-md overflow-hidden bg-github-bg-secondary"
            >
              <div class="px-3 py-2 bg-github-bg-secondary border-b border-github-border flex items-center justify-between gap-3">
                <button
                  type="button"
                  onclick={() => toggleFileCollapsed(file.path)}
                  aria-expanded={!isCollapsed}
                  class="flex items-center gap-2 min-w-0 text-left text-xs font-mono text-github-text-primary hover:text-github-accent"
                >
                  <span class="text-github-text-muted w-3 text-center">
                    {isCollapsed ? '▸' : '▾'}
                  </span>
                  <span class="inline-flex items-center justify-center w-4 h-4 text-[10px] font-semibold rounded border shrink-0 {badge.classes}">
                    {badge.label}
                  </span>
                  <span class="truncate">
                    {#if file.oldPath && file.oldPath !== file.path}
                      {file.oldPath} → {file.path}
                    {:else}
                      {file.path}
                    {/if}
                  </span>
                  {#if file.isGenerated}
                    <span class="px-1.5 py-0 text-[10px] font-sans rounded bg-github-bg-tertiary border border-github-border text-github-text-muted shrink-0">
                      Generated
                    </span>
                  {/if}
                </button>

                <div class="flex items-center gap-3 shrink-0">
                  <label class="inline-flex items-center gap-1.5 text-xs text-github-text-secondary cursor-pointer select-none">
                    <input
                      type="checkbox"
                      checked={isViewed}
                      onchange={() => void handleToggleFileViewed(file)}
                      class="rounded border-github-border cursor-pointer"
                    />
                    <span>Viewed</span>
                  </label>
                  <div class="font-mono text-xs flex items-center gap-2">
                    <span class="text-green-400">+{file.additions}</span>
                    <span class="text-red-400">-{file.deletions}</span>
                  </div>
                </div>
              </div>

              {#if !isCollapsed}
                <div class="overflow-x-auto bg-github-bg-primary">
                  <table class="w-full border-collapse font-mono text-xs">
                    <tbody>
                      {#each file.chunks as chunk, chunkIdx (chunkIdx)}
                        <tr class="bg-github-bg-tertiary/70 text-github-text-muted">
                          <td colspan="3" class="px-3 py-1 select-none">{chunk.header}</td>
                        </tr>
                        {#each chunk.lines as line, lineIdx (lineIdx)}
                          {@const rowClass =
                            line.type === 'add'
                              ? 'bg-diff-addition-bg/45'
                              : line.type === 'delete' || line.type === 'remove'
                                ? 'bg-diff-deletion-bg/45'
                                : ''}
                          {@const prefix =
                            line.type === 'add'
                              ? '+'
                              : line.type === 'delete' || line.type === 'remove'
                                ? '-'
                                : ' '}
                          <tr class={rowClass}>
                            <td class="w-12 px-2 py-0.5 text-right select-none text-github-text-muted border-r border-github-border/50">
                              {line.oldLineNumber ?? ''}
                            </td>
                            <td class="w-12 px-2 py-0.5 text-right select-none text-github-text-muted border-r border-github-border/50">
                              {line.newLineNumber ?? ''}
                            </td>
                            <td class="px-3 py-0.5 whitespace-pre text-github-text-primary">{prefix}{line.content}</td>
                          </tr>
                        {/each}
                      {/each}
                    </tbody>
                  </table>
                </div>
              {/if}
            </article>
          {/each}
        {/if}
      </main>
    </div>
  </div>
{/if}
