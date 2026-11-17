<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';

  import CommentsDrawer from './components/CommentsDrawer.svelte';
  import FileList from './components/FileList.svelte';
  import HelpModal from './components/HelpModal.svelte';
  import JjRevisionSelector from './components/JjRevisionSelector.svelte';
  import JjRevisionTree from './components/JjRevisionTree.svelte';
  import SettingsModal from './components/SettingsModal.svelte';
  import { getViewerForFile } from './viewers/registry';
  import type { PendingCursorCommentRequest } from './viewers/types';
  import {
    type CommentThread,
    type DiffCommentThread,
    type DiffFile,
    type DiffResponse,
    type DiffSide,
    type DiffViewMode,
    type ExpandedLinesState,
    type FileExpandedState,
    type JjChange,
    type JjCommit,
    type JjGraphRow,
    type LineNumber,
    type MergedChunk,
    type ViewedFileRecord,
  } from '../types/diff';
  import { createId } from '../utils/createId';
  import { DEFAULT_DIFF_VIEW_MODE, normalizeDiffViewMode } from '../utils/diffMode';
  import { normalizeBaseMode } from '../utils/diffSelection';
  import { resolveEditorOption } from '../utils/editorOptions';
  import {
    addJjComment,
    approveJjRevision,
    fetchDiff,
    fetchFileBlobText,
    fetchGeneratedStatus,
    fetchJjChanges,
    fetchJjComments,
    fetchJjCommitsForChange,
    fetchJjDiff,
    fetchJjStatus,
    fetchLineCount,
    normalizeJjBaseCommitId,
    openInEditor,
    replyJjComment,
    resolveJjComment,
  } from './services/api';
  import { storageService } from './services/StorageService';
  import { fetchClientSettings, saveClientSettings } from './services/userSettings';
  import {
    type AppearanceSettings,
    DEFAULT_APPEARANCE_SETTINGS,
    normalizeAppearanceSettings,
  } from './utils/appearanceSettings';
  import {
    APPEARANCE_STORAGE_KEY,
    applyResolvedTheme,
    resolveThemePreference,
    type ThemePreference,
  } from './utils/appearanceTheme';
  import { normalizeAutoViewedPatterns } from './utils/autoViewedPatterns';
  import { copyTextToClipboard } from './utils/clipboard';
  import { hasMultipleCommentAuthors } from './utils/commentAuthors';
  import { formatStatusBadge } from './utils/diffUtils';
  import { getFileElementId } from './utils/domUtils';
  import {
    addExpandRange,
    consolidateBetweenChunkRanges,
    DEFAULT_EXPAND_COUNT,
    getMergedChunksForFile,
    isExpandableFile,
    splitBlobLines,
  } from './utils/expandedLines';
  import { makeStandaloneCommitRow } from './utils/jjGraph';
  import { commitIdsMatch, pickInitialChange } from './utils/jjSelection';
  import {
    buildCommentIndex,
    createDiffNavigationFilters,
    type CursorPosition,
    findCursorForCommentThread,
    findNextMatchingPosition,
    findNextUnviewedFileIndex,
    fixSide,
    getStartPosition,
    isEditableEventTarget,
    scrollCursorIntoView,
    switchSplitCursorSide,
  } from './utils/keyboardNavigation';
  import { connectLiveWatch } from './utils/liveWatch';
  import { getElementId } from './utils/navigation/domHelpers';
  import { buildFileLineIndex, type FileLineIndex, isThreadOutdated } from './utils/outdatedComments';
  import { getFallbackSyntaxTheme, isSyntaxThemeForResolvedTheme } from './utils/themeLoader';
  import {
    hydrateViewedState,
    setMultipleFilesViewed,
    toggleSingleFileViewed,
    type ViewedContextParams,
  } from './utils/viewedState';
  import { createWordHighlightHandlers, setWordHighlightContext } from './utils/wordHighlight';

  const SIDEBAR_OPEN_STORAGE_KEY = 'difit.sidebarOpen';
  const DIFF_VIEW_MODE_STORAGE_KEY = 'difit.diffViewMode';
  const SHOW_RESOLVED_INLINE_STORAGE_KEY = 'tellit.showResolvedInline';
  const EDITOR_SETTINGS_STORAGE_KEY = 'difit.editor';

  function getInitialDiffViewMode(): DiffViewMode {
    if (typeof window === 'undefined') return DEFAULT_DIFF_VIEW_MODE;
    try {
      const stored = window.localStorage.getItem(DIFF_VIEW_MODE_STORAGE_KEY);
      if (stored) return normalizeDiffViewMode(stored);
    } catch {
      // Ignore storage access errors.
    }
    return DEFAULT_DIFF_VIEW_MODE;
  }

  function getInitialShowResolvedInline(): boolean {
    if (typeof window === 'undefined') return true;
    try {
      const stored = window.localStorage.getItem(SHOW_RESOLVED_INLINE_STORAGE_KEY);
      return stored === null ? true : stored === 'true';
    } catch {
      return true;
    }
  }

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

  function parseLegacyEditorPreference(raw: unknown): AppearanceSettings['editor'] | null {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) {
      return null;
    }
    const candidate = raw as Partial<AppearanceSettings['editor']>;
    if (typeof candidate.id !== 'string') {
      return null;
    }
    const preset = resolveEditorOption(candidate.id);
    return {
      id: preset.id,
      command: typeof candidate.command === 'string' ? candidate.command : preset.command,
      argsTemplate:
        typeof candidate.argsTemplate === 'string'
          ? candidate.argsTemplate
          : preset.argsTemplate,
    };
  }

  function getInitialAppearanceSettings(): AppearanceSettings {
    let base: AppearanceSettings = {
      ...DEFAULT_APPEARANCE_SETTINGS,
      editor: { ...DEFAULT_APPEARANCE_SETTINGS.editor },
    };

    if (typeof window !== 'undefined') {
      try {
        const legacyEditorRaw = window.localStorage.getItem(EDITOR_SETTINGS_STORAGE_KEY);
        if (legacyEditorRaw) {
          const legacyEditor = parseLegacyEditorPreference(JSON.parse(legacyEditorRaw));
          if (legacyEditor) {
            base = { ...base, editor: legacyEditor };
          }
        }
        const storedAppearance = window.localStorage.getItem(APPEARANCE_STORAGE_KEY);
        if (storedAppearance) {
          const parsed = normalizeAppearanceSettings(JSON.parse(storedAppearance), base);
          if (parsed) {
            base = parsed;
          }
        }
      } catch {
        // Ignore storage errors and fall back to defaults.
      }
    }

    const resolved = resolveThemePreference(base.theme);
    if (!isSyntaxThemeForResolvedTheme(base.syntaxTheme, resolved)) {
      base.syntaxTheme = getFallbackSyntaxTheme(resolved)?.id ?? base.syntaxTheme;
    }
    return base;
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
  let diffMode = $state<DiffViewMode>(getInitialDiffViewMode());
  let sidebarOpen = $state(getInitialSidebarOpen());
  let appearanceSettings = $state<AppearanceSettings>(getInitialAppearanceSettings());

  let revQueryError = $state<string | null>(null);
  let watchReloadPending = $state(false);
  let isReloading = $state(false);

  let viewedRecords = $state<ViewedFileRecord[]>([]);
  let viewedFiles = new SvelteSet<string>();
  let changedSinceViewedFiles = new SvelteSet<string>();
  let collapsedFiles = new SvelteSet<string>();
  let manuallyToggledPaths = new SvelteSet<string>();
  let fileHashesByPath = new Map<string, string>();

  let expandedStateByFile = $state<ExpandedLinesState>({});
  let isExpandLoading = $state(false);
  const pendingExpandedFetches = new Map<string, Promise<FileExpandedState | null>>();
  const prefetchedLineCountKeys = new Set<string>();

  let rawThreads = $state<DiffCommentThread[]>([]);
  let showResolvedInline = $state(getInitialShowResolvedInline());
  let commentsDrawerOpen = $state(false);
  let settingsModalOpen = $state(false);
  let helpModalOpen = $state(false);
  let activeWholeFileCommentPath = $state<string | null>(null);
  let highlightedThreadId = $state<string | null>(null);
  let copiedFilePath = $state<string | null>(null);
  let commentError = $state<string | null>(null);

  let keyboardCursor = $state<CursorPosition | null>(null);
  let lastRememberedCursor = $state<CursorPosition | null>(null);
  let lastNavigatedThreadId = $state<string | null>(null);
  let hoveredFileIndex = $state<number | null>(null);
  let pendingLineComment = $state<PendingCursorCommentRequest | null>(null);
  let nextPendingCommentSeq = 0;

  let hoveredWord = $state<string | null>(null);
  let pinnedWord = $state<string | null>(null);

  let activeRequestController: AbortController | null = null;
  let activeRequestId = 0;
  let activeCommentsController: AbortController | null = null;
  let activeCommentsRequestId = 0;
  let activeGeneratedController: AbortController | null = null;
  let diffRevisionId = 0;
  let viewedMutationChain: Promise<void> = Promise.resolve();
  let highlightResetTimer: ReturnType<typeof setTimeout> | null = null;
  let copiedFilePathTimer: ReturnType<typeof setTimeout> | null = null;

  const wordHighlightHandlers = createWordHighlightHandlers({
    getEffectiveWord: () => pinnedWord ?? hoveredWord,
    getPinnedWord: () => pinnedWord,
    setHoveredWord: (word) => {
      hoveredWord = word;
    },
    setPinnedWord: (word) => {
      pinnedWord = word;
    },
  });
  setWordHighlightContext(wordHighlightHandlers);

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

  const mergedChunksByFile = $derived.by(() => {
    const map = new Map<string, MergedChunk[]>();
    if (!diffData) return map;
    for (const file of diffData.files) {
      map.set(file.path, getMergedChunksForFile(file, expandedStateByFile[file.path]));
    }
    return map;
  });

  const fileLineIndexByPath = $derived.by(() => {
    const map = new Map<string, FileLineIndex>();
    if (!diffData) return map;
    for (const file of diffData.files) {
      const mergedChunks = mergedChunksByFile.get(file.path) ?? file.chunks;
      map.set(file.path, buildFileLineIndex({ ...file, chunks: mergedChunks }));
    }
    return map;
  });

  const normalizedThreads = $derived.by<CommentThread[]>(() =>
    rawThreads.map((thread) => ({
      id: thread.id,
      file: thread.filePath,
      line:
        typeof thread.position.line === 'number'
          ? thread.position.line
          : ([thread.position.line.start, thread.position.line.end] as [number, number]),
      side: thread.position.side,
      createdAt: thread.createdAt,
      updatedAt: thread.updatedAt,
      codeContent: thread.codeSnapshot?.content,
      isOutdated: isThreadOutdated(thread, fileLineIndexByPath.get(thread.filePath)),
      messages: thread.messages,
      resolved: thread.resolved,
    })),
  );

  const showAuthorBadges = $derived(
    hasMultipleCommentAuthors(normalizedThreads.flatMap((thread) => thread.messages)),
  );

  const unresolvedCommentCount = $derived(
    normalizedThreads.filter((thread) => !thread.resolved).length,
  );

  const threadsByFile = $derived.by(() => {
    const map = new Map<string, CommentThread[]>();
    for (const thread of normalizedThreads) {
      if (!showResolvedInline && thread.resolved) {
        continue;
      }
      const list = map.get(thread.file);
      if (list) {
        list.push(thread);
      } else {
        map.set(thread.file, [thread]);
      }
    }
    return map;
  });

  const themePreference = $derived(appearanceSettings.theme);
  const colorVision = $derived(appearanceSettings.colorVision);
  const resolvedTheme = $derived(resolveThemePreference(themePreference));
  const syntaxTheme = $derived(
    isSyntaxThemeForResolvedTheme(appearanceSettings.syntaxTheme, resolvedTheme)
      ? appearanceSettings.syntaxTheme
      : (getFallbackSyntaxTheme(resolvedTheme)?.id ?? appearanceSettings.syntaxTheme),
  );
  const editorSetting = $derived(appearanceSettings.editor);
  const autoViewedPatterns = $derived(appearanceSettings.autoViewedPatterns);

  const navigableFiles = $derived.by<DiffFile[]>(() => {
    if (!diffData) return [];
    return diffData.files.map((file) => ({
      ...file,
      chunks: mergedChunksByFile.get(file.path) ?? file.chunks,
    }));
  });

  const openInEditorAvailable = $derived(
    diffData?.openInEditorAvailable !== false &&
      editorSetting.id !== 'none' &&
      editorSetting.command.trim() !== '' &&
      editorSetting.argsTemplate.trim() !== '',
  );

  $effect(() => {
    if (typeof document !== 'undefined') {
      document.documentElement.style.setProperty(
        '--app-font-size',
        `${appearanceSettings.fontSize}px`,
      );
      document.documentElement.style.setProperty('--app-font-family', appearanceSettings.fontFamily);
    }

    const resolved = resolveThemePreference(themePreference);
    applyResolvedTheme(resolved, colorVision);

    if (themePreference !== 'auto' || typeof window === 'undefined') {
      return;
    }

    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
    const handleMediaChange = (event: MediaQueryListEvent) => {
      const nextResolved = event.matches ? 'dark' : 'light';
      applyResolvedTheme(nextResolved, colorVision);
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

  $effect(() => {
    if (typeof window === 'undefined') return;
    try {
      window.localStorage.setItem(SHOW_RESOLVED_INLINE_STORAGE_KEY, String(showResolvedInline));
    } catch {
      // Ignore storage access errors.
    }
  });

  async function loadJjCommentsForSelection(
    targetCommit: string,
    baseCommit?: string,
    rethrowOnError = false,
  ): Promise<void> {
    if (!targetCommit || targetCommit === 'stdin') {
      rawThreads = [];
      return;
    }

    activeCommentsController?.abort();
    const controller = new AbortController();
    activeCommentsController = controller;
    const reqId = ++activeCommentsRequestId;

    try {
      const threads = await fetchJjComments(targetCommit, baseCommit, controller.signal);
      if (controller.signal.aborted || reqId !== activeCommentsRequestId) return;
      rawThreads = threads;
    } catch (err) {
      if (isAbortError(err) || controller.signal.aborted || reqId !== activeCommentsRequestId) {
        return;
      }
      if (rethrowOnError) {
        throw err;
      }
      // Keep UI responsive if comment fetch fails on initial or background reload.
    }
  }

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
    expandedStateByFile = {};
    isExpandLoading = false;
    pendingExpandedFetches.clear();
    prefetchedLineCountKeys.clear();
    activeWholeFileCommentPath = null;
    highlightedThreadId = null;
    commentError = null;
    pinnedWord = null;
    keyboardCursor = null;
    lastRememberedCursor = null;
    lastNavigatedThreadId = null;
    hoveredFileIndex = null;
    pendingLineComment = null;

    for (const file of nextDiff.files) {
      if (file.isGenerated) {
        collapsedFiles.add(file.path);
      }
    }

    const context = buildViewedContext(nextDiff);
    const isStdin =
      nextDiff.targetCommitish === 'stdin' || nextDiff.baseCommitish === 'stdin';
    const ref = nextDiff.targetCommitish || 'HEAD';

    if (isJjRepo && !isStdin) {
      rawThreads = [];
      const targetCommit = jjTargetCommitId || nextDiff.targetCommitish || nextDiff.commit;
      void loadJjCommentsForSelection(targetCommit, jjBaseCommitId);
    } else {
      activeCommentsController?.abort();
      rawThreads = storageService.getCommentThreads(
        context.baseCommitish,
        context.targetCommitish,
        context.currentCommitHash,
        undefined,
        context.repositoryId,
        context.baseMode,
      );
    }

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
        let fallbackSettings = {
          ...appearanceSettings,
          editor: { ...appearanceSettings.editor },
          autoViewedPatterns: [...appearanceSettings.autoViewedPatterns],
        };
        let updatedFromLegacy = false;
        if (clientSettings.autoViewedPatterns !== undefined) {
          fallbackSettings.autoViewedPatterns = normalizeAutoViewedPatterns(
            clientSettings.autoViewedPatterns,
          );
          updatedFromLegacy = true;
        }
        const legacyRemoteEditor = parseLegacyEditorPreference(clientSettings.editor);
        if (legacyRemoteEditor) {
          fallbackSettings.editor = legacyRemoteEditor;
          updatedFromLegacy = true;
        }

        const remoteAppearance = normalizeAppearanceSettings(
          clientSettings[APPEARANCE_STORAGE_KEY],
          fallbackSettings,
        );
        if (remoteAppearance) {
          appearanceSettings = remoteAppearance;
          try {
            window.localStorage.setItem(APPEARANCE_STORAGE_KEY, JSON.stringify(remoteAppearance));
          } catch {
            // Ignore local cache write errors.
          }
        } else if (updatedFromLegacy) {
          appearanceSettings = fallbackSettings;
        } else {
          let hasLocalAppearance = false;
          try {
            hasLocalAppearance = window.localStorage.getItem(APPEARANCE_STORAGE_KEY) !== null;
          } catch {
            // Ignore local storage errors.
          }
          if (hasLocalAppearance) {
            saveClientSettings({ [APPEARANCE_STORAGE_KEY]: appearanceSettings });
          }
        }

        if (typeof clientSettings.diffViewMode === 'string') {
          let hasLocalDiffMode = false;
          try {
            hasLocalDiffMode = Boolean(window.localStorage.getItem(DIFF_VIEW_MODE_STORAGE_KEY));
          } catch {
            // Ignore storage errors.
          }
          if (!hasLocalDiffMode) {
            diffMode = normalizeDiffViewMode(clientSettings.diffViewMode);
          }
        }
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

  function refreshJjCommentsInBackground() {
    if (!isJjRepo || !diffData) return;
    const isStdin =
      diffData.targetCommitish === 'stdin' || diffData.baseCommitish === 'stdin';
    if (isStdin) return;
    const targetCommit = jjTargetCommitId || diffData.targetCommitish || diffData.commit;
    void loadJjCommentsForSelection(targetCommit, jjBaseCommitId);
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
        refreshJjCommentsInBackground();
      },
    });

    return () => {
      disconnectLiveWatch();
      activeRequestController?.abort();
      activeCommentsController?.abort();
      activeGeneratedController?.abort();
      wordHighlightHandlers.dispose();
      if (highlightResetTimer) clearTimeout(highlightResetTimer);
      if (copiedFilePathTimer) clearTimeout(copiedFilePathTimer);
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

  function handleDiffModeChange(nextMode: DiffViewMode) {
    diffMode = nextMode;
    if (typeof window !== 'undefined') {
      try {
        window.localStorage.setItem(DIFF_VIEW_MODE_STORAGE_KEY, nextMode);
      } catch {
        // Ignore storage errors.
      }
    }
    saveClientSettings({ diffViewMode: nextMode });
  }

  function handleAppearanceSettingsChange(next: AppearanceSettings) {
    const prevPatterns = appearanceSettings.autoViewedPatterns;
    appearanceSettings = next;
    if (typeof window !== 'undefined') {
      try {
        window.localStorage.setItem(APPEARANCE_STORAGE_KEY, JSON.stringify(next));
      } catch {
        // Ignore storage errors.
      }
    }
    saveClientSettings({
      [APPEARANCE_STORAGE_KEY]: next,
      autoViewedPatterns: next.autoViewedPatterns,
      editor: next.editor,
    });

    if (
      diffData &&
      JSON.stringify(prevPatterns) !== JSON.stringify(next.autoViewedPatterns)
    ) {
      const snapshotDiff = diffData;
      const revisionSnapshot = diffRevisionId;
      const context = buildViewedContext(snapshotDiff);
      void enqueueViewedMutation(async () => {
        if (revisionSnapshot !== diffRevisionId) return;
        try {
          const hydrated = await hydrateViewedState(
            context,
            snapshotDiff.files,
            next.autoViewedPatterns,
          );
          if (revisionSnapshot !== diffRevisionId) return;
          viewedRecords = hydrated.records;
          fileHashesByPath = hydrated.hashesByPath;
          viewedFiles.clear();
          changedSinceViewedFiles.clear();
          for (const f of snapshotDiff.files) {
            if (hydrated.viewedPaths.has(f.path)) {
              viewedFiles.add(f.path);
              if (!manuallyToggledPaths.has(f.path)) {
                collapsedFiles.add(f.path);
              }
            }
          }
        } catch {
          // Keep existing viewed state on transient error.
        }
      });
    }
  }

  function handleThemeChange(nextTheme: ThemePreference) {
    const nextResolved = resolveThemePreference(nextTheme);
    const nextSyntaxTheme = isSyntaxThemeForResolvedTheme(
      appearanceSettings.syntaxTheme,
      nextResolved,
    )
      ? appearanceSettings.syntaxTheme
      : (getFallbackSyntaxTheme(nextResolved)?.id ?? appearanceSettings.syntaxTheme);
    handleAppearanceSettingsChange({
      ...appearanceSettings,
      theme: nextTheme,
      syntaxTheme: nextSyntaxTheme,
    });
  }

  async function ensureExpandedFileContent(
    file: DiffFile,
    revisionIdSnapshot: number,
  ): Promise<FileExpandedState | null> {
    if (!diffData || !isExpandableFile(file)) return null;
    const baseRef = diffData.baseCommitish;
    const targetRef = diffData.targetCommitish;
    if (baseRef === 'stdin' || targetRef === 'stdin') return null;

    const existing = expandedStateByFile[file.path];
    if (existing?.oldContent && existing?.newContent) {
      return existing;
    }

    const pendingKey = `${revisionIdSnapshot}:${file.path}`;
    const inFlight = pendingExpandedFetches.get(pendingKey);
    if (inFlight) return inFlight;

    const promise = (async (): Promise<FileExpandedState | null> => {
      const nextState: FileExpandedState = {
        ...existing,
        expandedRanges: existing?.expandedRanges ?? [],
      };

      if (file.status !== 'added' && baseRef) {
        try {
          const text = await fetchFileBlobText(file.oldPath || file.path, baseRef);
          if (revisionIdSnapshot !== diffRevisionId) return null;
          const lines = splitBlobLines(text);
          nextState.oldContent = lines;
          nextState.oldTotalLines = lines.length;
        } catch {
          nextState.oldContent = [];
          nextState.oldTotalLines = 0;
        }
      }

      if (file.status !== 'deleted' && targetRef) {
        try {
          const text = await fetchFileBlobText(file.path, targetRef);
          if (revisionIdSnapshot !== diffRevisionId) return null;
          const lines = splitBlobLines(text);
          nextState.newContent = lines;
          nextState.newTotalLines = lines.length;
        } catch {
          nextState.newContent = [];
          nextState.newTotalLines = 0;
        }
      }

      return nextState;
    })();

    pendingExpandedFetches.set(pendingKey, promise);
    try {
      return await promise;
    } finally {
      pendingExpandedFetches.delete(pendingKey);
    }
  }

  function handlePrefetchLineCount(file: DiffFile) {
    if (!diffData || !isExpandableFile(file)) return;
    const baseRef = diffData.baseCommitish;
    const targetRef = diffData.targetCommitish;
    if (baseRef === 'stdin' || targetRef === 'stdin') return;

    const existing = expandedStateByFile[file.path];
    if (existing?.oldTotalLines !== undefined || existing?.newTotalLines !== undefined) return;

    const revisionIdSnapshot = diffRevisionId;
    const key = `${revisionIdSnapshot}:${file.path}`;
    if (prefetchedLineCountKeys.has(key)) return;
    prefetchedLineCountKeys.add(key);

    const oldRef = file.status !== 'added' ? baseRef : undefined;
    const newRef = file.status !== 'deleted' ? targetRef : undefined;
    if (!oldRef && !newRef) return;

    void fetchLineCount(file.path, oldRef, newRef, file.oldPath)
      .then((counts) => {
        if (revisionIdSnapshot !== diffRevisionId) return;
        const current = expandedStateByFile[file.path];
        if (current?.oldTotalLines !== undefined || current?.newTotalLines !== undefined) return;
        expandedStateByFile = {
          ...expandedStateByFile,
          [file.path]: {
            ...current,
            expandedRanges: current?.expandedRanges ?? [],
            oldTotalLines: counts.oldLineCount,
            newTotalLines: counts.newLineCount,
          },
        };
      })
      .catch(() => {
        // Ignore prefetch line-count failures and allow manual expand.
      });
  }

  async function handleExpandLines(
    file: DiffFile,
    chunkIndex: number,
    direction: 'up' | 'down',
    count: number = DEFAULT_EXPAND_COUNT,
  ) {
    const revisionIdSnapshot = diffRevisionId;
    isExpandLoading = true;
    try {
      const loaded = await ensureExpandedFileContent(file, revisionIdSnapshot);
      if (!loaded || revisionIdSnapshot !== diffRevisionId) return;
      const current = expandedStateByFile[file.path];
      const currentRanges = current?.expandedRanges ?? loaded.expandedRanges;
      expandedStateByFile = {
        ...expandedStateByFile,
        [file.path]: {
          oldContent: current?.oldContent ?? loaded.oldContent,
          newContent: current?.newContent ?? loaded.newContent,
          oldTotalLines: current?.oldTotalLines ?? loaded.oldTotalLines,
          newTotalLines: current?.newTotalLines ?? loaded.newTotalLines,
          expandedRanges: addExpandRange(currentRanges, chunkIndex, direction, count),
        },
      };
    } finally {
      if (revisionIdSnapshot === diffRevisionId) {
        isExpandLoading = false;
      }
    }
  }

  async function handleExpandAllBetween(file: DiffFile, chunkIndex: number, hiddenLines: number) {
    const revisionIdSnapshot = diffRevisionId;
    isExpandLoading = true;
    try {
      const loaded = await ensureExpandedFileContent(file, revisionIdSnapshot);
      if (!loaded || revisionIdSnapshot !== diffRevisionId) return;
      const current = expandedStateByFile[file.path];
      const currentRanges = current?.expandedRanges ?? loaded.expandedRanges;
      expandedStateByFile = {
        ...expandedStateByFile,
        [file.path]: {
          oldContent: current?.oldContent ?? loaded.oldContent,
          newContent: current?.newContent ?? loaded.newContent,
          oldTotalLines: current?.oldTotalLines ?? loaded.oldTotalLines,
          newTotalLines: current?.newTotalLines ?? loaded.newTotalLines,
          expandedRanges: consolidateBetweenChunkRanges(currentRanges, chunkIndex, hiddenLines),
        },
      };
    } finally {
      if (revisionIdSnapshot === diffRevisionId) {
        isExpandLoading = false;
      }
    }
  }

  function persistNonJjThreads(nextThreads: DiffCommentThread[]) {
    rawThreads = nextThreads;
    if (!diffData) return;
    const context = buildViewedContext(diffData);
    storageService.saveCommentThreads(
      context.baseCommitish,
      context.targetCommitish,
      nextThreads,
      context.currentCommitHash,
      undefined,
      context.repositoryId,
      context.baseMode,
    );
  }

  async function handleAddComment(
    filePath: string,
    line: LineNumber | undefined,
    body: string,
    codeContent?: string,
    side?: DiffSide,
  ) {
    if (isJjRepo && diffData) {
      const normalizedBase = normalizeJjBaseCommitId(jjBaseCommitId);
      const targetCommit =
        side === 'old' && normalizedBase
          ? normalizedBase
          : jjTargetCommitId || diffData.targetCommitish || diffData.commit;
      try {
        await addJjComment({
          commitId: targetCommit,
          filePath,
          line:
            typeof line === 'number'
              ? line
              : Array.isArray(line)
                ? { start: line[0], end: line[1] }
                : undefined,
          content: body,
        });
        commentError = null;
        const activeTarget = jjTargetCommitId || diffData.targetCommitish || diffData.commit;
        await loadJjCommentsForSelection(activeTarget, jjBaseCommitId);
        void refreshJjMetadataInBackground();
      } catch (err) {
        const msg = err instanceof Error && err.message ? err.message : 'Failed to add comment';
        commentError = msg;
        throw new Error(msg, { cause: err });
      }
      return;
    }

    const now = new Date().toISOString();
    const id = createId();
    const positionLine =
      typeof line === 'number' ? line : Array.isArray(line) ? { start: line[0], end: line[1] } : 1;
    const nextThread: DiffCommentThread = {
      id,
      filePath,
      createdAt: now,
      updatedAt: now,
      position: { side: side ?? 'new', line: positionLine },
      codeSnapshot: codeContent !== undefined ? { content: codeContent } : undefined,
      messages: [{ id, body, author: 'User', createdAt: now, updatedAt: now }],
    };
    persistNonJjThreads([...rawThreads, nextThread]);
  }

  async function handleReplyToThread(threadId: string, body: string) {
    if (isJjRepo && diffData) {
      try {
        await replyJjComment(threadId, body);
        commentError = null;
        const activeTarget = jjTargetCommitId || diffData.targetCommitish || diffData.commit;
        await loadJjCommentsForSelection(activeTarget, jjBaseCommitId);
        void refreshJjMetadataInBackground();
      } catch (err) {
        const msg =
          err instanceof Error && err.message ? err.message : 'Failed to reply to comment';
        commentError = msg;
        throw new Error(msg, { cause: err });
      }
      return;
    }

    const now = new Date().toISOString();
    const nextThreads = rawThreads.map((thread) =>
      thread.id === threadId
        ? {
            ...thread,
            updatedAt: now,
            messages: [
              ...thread.messages,
              { id: createId(), body, author: 'User', createdAt: now, updatedAt: now },
            ],
          }
        : thread,
    );
    persistNonJjThreads(nextThreads);
  }

  async function handleToggleResolve(threadId: string, currentResolved: boolean) {
    if (isJjRepo && diffData) {
      try {
        await resolveJjComment(threadId, currentResolved);
        commentError = null;
        const activeTarget = jjTargetCommitId || diffData.targetCommitish || diffData.commit;
        await loadJjCommentsForSelection(activeTarget, jjBaseCommitId);
        void refreshJjMetadataInBackground();
      } catch (err) {
        const msg =
          err instanceof Error && err.message
            ? err.message
            : currentResolved
              ? 'Failed to reopen comment'
              : 'Failed to resolve comment';
        commentError = msg;
        throw new Error(msg, { cause: err });
      }
      return;
    }

    const nextThreads = rawThreads.map((thread) =>
      thread.id === threadId
        ? { ...thread, resolved: !currentResolved, updatedAt: new Date().toISOString() }
        : thread,
    );
    persistNonJjThreads(nextThreads);
  }

  function handleRemoveThread(threadId: string) {
    if (isJjRepo) return;
    persistNonJjThreads(rawThreads.filter((thread) => thread.id !== threadId));
  }

  async function handleOpenInEditor(filePath: string, lineNumber: number) {
    if (filePath === 'Commit Message') return;
    try {
      await openInEditor({ filePath, line: lineNumber, editor: editorSetting });
      commentError = null;
    } catch (err) {
      commentError = err instanceof Error ? err.message : 'Failed to open file in editor';
    }
  }

  async function handleCopyFilePath(filePath: string) {
    try {
      await copyTextToClipboard(filePath);
      if (copiedFilePathTimer) clearTimeout(copiedFilePathTimer);
      copiedFilePath = filePath;
      copiedFilePathTimer = setTimeout(() => {
        if (copiedFilePath === filePath) {
          copiedFilePath = null;
        }
        copiedFilePathTimer = null;
      }, 2000);
    } catch (err) {
      commentError = err instanceof Error ? err.message : 'Failed to copy file path';
    }
  }

  function openWholeFileComment(filePath: string) {
    if (collapsedFiles.has(filePath)) {
      manuallyToggledPaths.add(filePath);
      collapsedFiles.delete(filePath);
    }
    activeWholeFileCommentPath = activeWholeFileCommentPath === filePath ? null : filePath;
  }

  async function handleNavigateToThread(thread: CommentThread) {
    if (!showResolvedInline && thread.resolved) {
      showResolvedInline = true;
    }
    if (collapsedFiles.has(thread.file)) {
      manuallyToggledPaths.add(thread.file);
      collapsedFiles.delete(thread.file);
    }
    highlightedThreadId = thread.id;
    if (highlightResetTimer) clearTimeout(highlightResetTimer);
    highlightResetTimer = setTimeout(() => {
      highlightedThreadId = null;
      highlightResetTimer = null;
    }, 2000);

    await tick();
    const threadEl = document.getElementById(`comment-thread-${thread.id}`);
    if (threadEl) {
      threadEl.scrollIntoView({ behavior: 'smooth', block: 'center' });
      return;
    }
    scrollToFile(thread.file);
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

  function setKeyboardCursor(pos: CursorPosition | null) {
    keyboardCursor = pos;
    if (pos) {
      lastRememberedCursor = pos;
    }
  }

  function navigateWithFilter(direction: 'next' | 'prev', filterKind: 'line' | 'chunk' | 'comment') {
    if (navigableFiles.length === 0) return;
    const commentIndex = buildCommentIndex(normalizedThreads);
    const filters = createDiffNavigationFilters(
      navigableFiles,
      commentIndex,
      diffMode,
      collapsedFiles,
    );
    const startPos = getStartPosition(keyboardCursor ?? lastRememberedCursor);
    const result = findNextMatchingPosition(
      startPos,
      direction,
      filters[filterKind],
      navigableFiles,
      diffMode,
    );
    if (result.position) {
      setKeyboardCursor(result.position);
      if (result.scrollTarget) {
        scrollCursorIntoView(result.scrollTarget);
      }
    }
  }

  function jumpToFileIndex(targetIndex: number) {
    const file = navigableFiles[targetIndex];
    if (!file) return;
    if (file.chunks[0]?.lines[0]) {
      const nextPos = fixSide(
        {
          fileIndex: targetIndex,
          chunkIndex: 0,
          lineIndex: 0,
          side: diffMode === 'split' ? 'left' : 'right',
        },
        navigableFiles,
      );
      setKeyboardCursor(nextPos);
      if (collapsedFiles.has(file.path)) {
        scrollToFile(file.path);
      } else {
        scrollCursorIntoView(getElementId(nextPos, diffMode));
      }
    } else {
      setKeyboardCursor({
        fileIndex: targetIndex,
        chunkIndex: 0,
        lineIndex: 0,
        side: diffMode === 'split' ? 'left' : 'right',
      });
      scrollToFile(file.path);
    }
  }

  function stepFile(direction: 'next' | 'prev') {
    const total = navigableFiles.length;
    if (total === 0) return;
    const currentIdx = keyboardCursor?.fileIndex ?? lastRememberedCursor?.fileIndex ?? -1;
    const targetIdx =
      direction === 'next'
        ? currentIdx < 0
          ? 0
          : (currentIdx + 1) % total
        : currentIdx < 0
          ? total - 1
          : (currentIdx - 1 + total) % total;
    jumpToFileIndex(targetIdx);
  }

  function stepUnresolvedComment(direction: 'next' | 'prev') {
    const unresolved = normalizedThreads.filter((t) => !t.resolved);
    if (unresolved.length === 0) return;

    // Advance linearly in file/line order independently of transient highlight timeout.
    const ordered = [...unresolved].sort((a, b) => {
      const aFileIdx = navigableFiles.findIndex((f) => f.path === a.file);
      const bFileIdx = navigableFiles.findIndex((f) => f.path === b.file);
      if (aFileIdx !== bFileIdx) return aFileIdx - bFileIdx;
      const aLine = Array.isArray(a.line) ? a.line[0] : a.line;
      const bLine = Array.isArray(b.line) ? b.line[0] : b.line;
      if (aLine !== bLine) return aLine - bLine;
      return a.createdAt.localeCompare(b.createdAt);
    });

    const activeIdx = lastNavigatedThreadId
      ? ordered.findIndex((t) => t.id === lastNavigatedThreadId)
      : -1;
    const nextIdx =
      activeIdx < 0
        ? direction === 'next'
          ? 0
          : ordered.length - 1
        : (activeIdx + (direction === 'next' ? 1 : -1) + ordered.length) % ordered.length;
    const targetThread = ordered[nextIdx];
    if (!targetThread) return;

    lastNavigatedThreadId = targetThread.id;
    const pos = findCursorForCommentThread(targetThread, navigableFiles);
    if (pos) {
      setKeyboardCursor(pos);
    }
    void handleNavigateToThread(targetThread);
  }

  function triggerCommentOnCursor() {
    if (!keyboardCursor) return;
    const file = navigableFiles[keyboardCursor.fileIndex];
    if (!file) return;
    if (collapsedFiles.has(file.path)) {
      manuallyToggledPaths.add(file.path);
      collapsedFiles.delete(file.path);
    }
    if (getViewerForFile(file).id === 'image') {
      openWholeFileComment(file.path);
      return;
    }
    const line = file.chunks[keyboardCursor.chunkIndex]?.lines[keyboardCursor.lineIndex];
    if (!line) {
      openWholeFileComment(file.path);
      return;
    }
    const isDelete = line.type === 'delete' || line.type === 'remove';
    const side: DiffSide =
      isDelete || (keyboardCursor.side === 'left' && line.oldLineNumber !== undefined)
        ? 'old'
        : 'new';
    const lineNum =
      side === 'old' ? line.oldLineNumber : (line.newLineNumber ?? line.oldLineNumber);
    if (lineNum === undefined) return;
    pendingLineComment = {
      filePath: file.path,
      side,
      line: lineNum,
      seq: ++nextPendingCommentSeq,
    };
  }

  async function toggleCurrentFileViewed(advanceToNext: boolean) {
    if (!diffData || diffData.files.length === 0) return;
    const targetIdx =
      keyboardCursor?.fileIndex ??
      hoveredFileIndex ??
      lastRememberedCursor?.fileIndex ??
      0;
    const file = diffData.files[targetIdx];
    if (!file) return;

    if (!advanceToNext || !viewedFiles.has(file.path)) {
      await handleToggleFileViewed(file);
    }
    if (advanceToNext) {
      const nextUnviewedIdx = findNextUnviewedFileIndex(targetIdx, navigableFiles, viewedFiles);
      if (nextUnviewedIdx !== null) {
        jumpToFileIndex(nextUnviewedIdx);
      }
    }
  }

  function handleGlobalKeydown(event: KeyboardEvent) {
    if (settingsModalOpen) return;
    if (isEditableEventTarget(event.target)) return;
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    if (!diffData) return;

    if (event.key === '?') {
      event.preventDefault();
      helpModalOpen = !helpModalOpen;
      return;
    }
    if (helpModalOpen) return;

    switch (event.key) {
      case 'j':
      case 'ArrowDown':
        event.preventDefault();
        navigateWithFilter('next', 'line');
        break;
      case 'k':
      case 'ArrowUp':
        event.preventDefault();
        navigateWithFilter('prev', 'line');
        break;
      case 'n':
        event.preventDefault();
        navigateWithFilter('next', 'chunk');
        break;
      case 'p':
        event.preventDefault();
        navigateWithFilter('prev', 'chunk');
        break;
      case 'N':
        event.preventDefault();
        stepUnresolvedComment('next');
        break;
      case 'P':
        event.preventDefault();
        stepUnresolvedComment('prev');
        break;
      case ']':
        event.preventDefault();
        stepFile('next');
        break;
      case '[':
        event.preventDefault();
        stepFile('prev');
        break;
      case '{':
        event.preventDefault();
        if (navigableFiles.length > 0) jumpToFileIndex(0);
        break;
      case '}':
        event.preventDefault();
        if (navigableFiles.length > 0) jumpToFileIndex(navigableFiles.length - 1);
        break;
      case 'h':
      case 'ArrowLeft':
        if (diffMode === 'split' && keyboardCursor) {
          event.preventDefault();
          const switched = switchSplitCursorSide(keyboardCursor, 'left', navigableFiles);
          setKeyboardCursor(switched);
          scrollCursorIntoView(getElementId(switched, diffMode));
        }
        break;
      case 'l':
      case 'ArrowRight':
        if (diffMode === 'split' && keyboardCursor) {
          event.preventDefault();
          const switched = switchSplitCursorSide(keyboardCursor, 'right', navigableFiles);
          setKeyboardCursor(switched);
          scrollCursorIntoView(getElementId(switched, diffMode));
        }
        break;
      case 'u':
        event.preventDefault();
        handleDiffModeChange('unified');
        break;
      case 's':
        event.preventDefault();
        handleDiffModeChange('split');
        break;
      case 'v':
        event.preventDefault();
        void toggleCurrentFileViewed(false);
        break;
      case 'V':
        event.preventDefault();
        void toggleCurrentFileViewed(true);
        break;
      case 'c':
        event.preventDefault();
        triggerCommentOnCursor();
        break;
      case 'L':
        event.preventDefault();
        commentsDrawerOpen = !commentsDrawerOpen;
        break;
      case 'R':
        event.preventDefault();
        void handleReload();
        break;
    }
  }
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

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
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="h-screen flex flex-col bg-github-bg-primary text-github-text-primary"
    onclick={(event) => {
      const target = event.target;
      if (!(target instanceof HTMLElement && target.closest('.word-token'))) {
        pinnedWord = null;
      }
    }}
    onkeydown={() => {}}
  >
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

      <div class="flex flex-wrap items-center gap-3 text-xs text-github-text-secondary">
        <div
          class="inline-flex rounded border border-github-border bg-github-bg-primary p-0.5"
          role="group"
          aria-label="Diff view mode"
        >
          <button
            type="button"
            onclick={() => handleDiffModeChange('split')}
            aria-pressed={diffMode === 'split'}
            class="px-2 py-0.5 rounded text-xs cursor-pointer transition-colors {diffMode === 'split'
              ? 'bg-github-bg-tertiary text-github-text-primary font-medium'
              : 'text-github-text-secondary hover:text-github-text-primary'}"
          >
            Split
          </button>
          <button
            type="button"
            onclick={() => handleDiffModeChange('unified')}
            aria-pressed={diffMode === 'unified'}
            class="px-2 py-0.5 rounded text-xs cursor-pointer transition-colors {diffMode === 'unified'
              ? 'bg-github-bg-tertiary text-github-text-primary font-medium'
              : 'text-github-text-secondary hover:text-github-text-primary'}"
          >
            Unified
          </button>
        </div>

        <button
          type="button"
          onclick={() => {
            commentsDrawerOpen = !commentsDrawerOpen;
          }}
          aria-expanded={commentsDrawerOpen}
          aria-controls="comments-drawer"
          aria-label="Toggle comments drawer"
          class="px-2.5 py-1 rounded border flex items-center gap-1.5 cursor-pointer transition-colors {commentsDrawerOpen
            ? 'bg-github-bg-primary border-blue-500/60 text-github-text-primary'
            : 'bg-github-bg-tertiary border-github-border text-github-text-secondary hover:text-github-text-primary'}"
        >
          <span>Comments</span>
          {#if normalizedThreads.length > 0}
            <span
              class="px-1.5 py-0 rounded-full text-[10px] font-semibold {unresolvedCommentCount > 0
                ? 'bg-yellow-500/20 text-yellow-400 border border-yellow-500/30'
                : 'bg-green-500/20 text-green-400 border border-green-500/30'}"
            >
              {unresolvedCommentCount > 0 ? `${unresolvedCommentCount} open` : 'All resolved'}
            </span>
          {/if}
        </button>

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

        <button
          type="button"
          onclick={() => {
            settingsModalOpen = true;
          }}
          aria-label="Open settings"
          title="Settings"
          class="px-2.5 py-1 text-xs rounded border border-github-border bg-github-bg-tertiary text-github-text-secondary hover:text-github-text-primary cursor-pointer"
        >
          Settings
        </button>

        <button
          type="button"
          onclick={() => {
            helpModalOpen = !helpModalOpen;
          }}
          aria-label="Keyboard shortcuts"
          title="Keyboard shortcuts (?)"
          class="px-2 py-1 text-xs font-mono rounded border border-github-border bg-github-bg-tertiary text-github-text-secondary hover:text-github-text-primary cursor-pointer"
        >
          ?
        </button>
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
            threads={normalizedThreads}
            onScrollToFile={scrollToFile}
            onToggleViewed={(file) => void handleToggleFileViewed(file)}
            onToggleFolderViewed={(folderFiles, nextViewed) =>
              void handleToggleFolderViewed(folderFiles, nextViewed)}
          />
        </aside>
      {/if}

      <main class="flex-1 overflow-y-auto p-4 space-y-4">
        {#if commentError}
          <div
            role="alert"
            class="px-3 py-2 rounded-md border border-red-500/40 bg-red-500/10 text-xs text-red-300 flex items-center justify-between gap-3"
          >
            <span>{commentError}</span>
            <button
              type="button"
              onclick={() => {
                commentError = null;
              }}
              aria-label="Dismiss comment error"
              class="px-2 py-0.5 rounded border border-red-500/30 text-red-200 hover:bg-red-500/20 shrink-0"
            >
              Dismiss
            </button>
          </div>
        {/if}

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
          {#each diffData.files as file, fileIdx (file.path)}
            {@const badge = formatStatusBadge(file.status)}
            {@const isCollapsed = collapsedFiles.has(file.path)}
            {@const isViewed = viewedFiles.has(file.path)}
            {@const isKeyboardFocused = keyboardCursor?.fileIndex === fileIdx}
            {@const viewer = getViewerForFile(file)}
            {@const ViewerComponent = viewer.Component}
            <article
              id={getFileElementId(file.path)}
              data-file-path={file.path}
              onmouseenter={() => {
                hoveredFileIndex = fileIdx;
              }}
              onmouseleave={() => {
                if (hoveredFileIndex === fileIdx) hoveredFileIndex = null;
              }}
              class="border border-github-border rounded-md overflow-hidden bg-github-bg-secondary"
            >
              <div class="px-3 py-2 bg-github-bg-secondary border-b border-github-border flex items-center justify-between gap-3 {isKeyboardFocused ? 'keyboard-focused-file-header' : ''}">
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

                <div class="flex items-center gap-2 shrink-0">
                  <button
                    type="button"
                    onclick={() => void handleCopyFilePath(file.path)}
                    title="Copy file path"
                    class="px-2 py-0.5 text-xs rounded border border-github-border bg-github-bg-tertiary text-github-text-secondary hover:text-github-text-primary cursor-pointer"
                  >
                    {copiedFilePath === file.path ? 'Copied' : 'Copy'}
                  </button>
                  <button
                    type="button"
                    onclick={() => openWholeFileComment(file.path)}
                    title="Comment on {file.path}"
                    aria-label="Comment on {file.path}"
                    class="px-2 py-0.5 text-xs rounded border border-github-border bg-github-bg-tertiary text-github-text-secondary hover:text-github-text-primary cursor-pointer"
                  >
                    Comment
                  </button>
                  <label class="inline-flex items-center gap-1.5 text-xs text-github-text-secondary cursor-pointer select-none ml-1">
                    <input
                      type="checkbox"
                      checked={isViewed}
                      onchange={() => void handleToggleFileViewed(file)}
                      class="rounded border-github-border cursor-pointer"
                    />
                    <span>Viewed</span>
                  </label>
                  <div class="font-mono text-xs flex items-center gap-2 ml-1">
                    <span class="text-green-400">+{file.additions}</span>
                    <span class="text-red-400">-{file.deletions}</span>
                  </div>
                </div>
              </div>

              {#if !isCollapsed}
                <ViewerComponent
                  {file}
                  mergedChunks={mergedChunksByFile.get(file.path) ?? []}
                  {diffMode}
                  threads={threadsByFile.get(file.path) ?? []}
                  {showAuthorBadges}
                  {syntaxTheme}
                  baseCommitish={diffData.baseCommitish}
                  targetCommitish={diffData.targetCommitish}
                  {openInEditorAvailable}
                  {isExpandLoading}
                  {isCollapsed}
                  activeWholeFileComment={activeWholeFileCommentPath === file.path}
                  {highlightedThreadId}
                  fileIndex={fileIdx}
                  cursor={isKeyboardFocused ? keyboardCursor : null}
                  {pendingLineComment}
                  onExpandLines={(targetFile, chunkIndex, direction, count) =>
                    void handleExpandLines(targetFile, chunkIndex, direction, count)}
                  onExpandAllBetween={(targetFile, chunkIndex, hiddenLines) =>
                    void handleExpandAllBetween(targetFile, chunkIndex, hiddenLines)}
                  onPrefetchLineCount={(targetFile) => {
                    if (viewer.canExpandHiddenLines(targetFile)) {
                      handlePrefetchLineCount(targetFile);
                    }
                  }}
                  onAddComment={handleAddComment}
                  onReplyToThread={handleReplyToThread}
                  onToggleResolve={handleToggleResolve}
                  onRemoveThread={isJjRepo ? undefined : handleRemoveThread}
                  onOpenInEditor={(targetPath, lineNum) =>
                    void handleOpenInEditor(targetPath, lineNum)}
                  onCancelWholeFileComment={() => {
                    activeWholeFileCommentPath = null;
                  }}
                  onConsumePendingComment={(seq) => {
                    if (pendingLineComment?.seq === seq) {
                      pendingLineComment = null;
                    }
                  }}
                />
              {/if}
            </article>
          {/each}
        {/if}
      </main>

      <CommentsDrawer
        isOpen={commentsDrawerOpen}
        threads={normalizedThreads}
        {showAuthorBadges}
        {showResolvedInline}
        {syntaxTheme}
        promptContext={{
          requestedBaseCommitish: diffData.requestedBaseCommitish,
          requestedTargetCommitish: diffData.requestedTargetCommitish,
          baseMode: normalizeBaseMode(diffData.requestedBaseMode),
          resolvedBaseCommitish: diffData.baseCommitish,
          resolvedTargetCommitish: diffData.targetCommitish,
        }}
        onClose={() => {
          commentsDrawerOpen = false;
        }}
        onToggleShowResolvedInline={(next) => {
          showResolvedInline = next;
        }}
        onNavigateToThread={(thread) => void handleNavigateToThread(thread)}
        onReplyToThread={handleReplyToThread}
        onToggleResolve={handleToggleResolve}
        onRemoveThread={isJjRepo ? undefined : handleRemoveThread}
      />
    </div>
  </div>

  <SettingsModal
    isOpen={settingsModalOpen}
    settings={appearanceSettings}
    onClose={() => {
      settingsModalOpen = false;
    }}
    onSettingsChange={handleAppearanceSettingsChange}
  />

  <HelpModal
    isOpen={helpModalOpen}
    onClose={() => {
      helpModalOpen = false;
    }}
  />
{/if}
