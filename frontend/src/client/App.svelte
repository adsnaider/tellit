<script lang="ts">
  import { onMount } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';

  import { type DiffFile, type DiffResponse, type JjChange } from '../types/diff';
  import {
    fetchDiff,
    fetchJjChanges,
    fetchJjDiff,
    fetchJjStatus,
  } from './services/api';
  import {
    APPEARANCE_STORAGE_KEY,
    applyResolvedTheme,
    type ColorVisionMode,
    getStoredAppearanceSettings,
    resolveThemePreference,
    type ThemePreference,
  } from './utils/appearanceTheme';
  import { getFileElementId } from './utils/domUtils';
  import { pickInitialChange } from './utils/jjSelection';

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

  let loading = $state(true);
  let error = $state<string | null>(null);
  let isJjRepo = $state(false);
  let repoRoot = $state<string | undefined>(undefined);
  let jjChanges = $state<JjChange[]>([]);
  let selectedChangeId = $state('');
  let diffData = $state<DiffResponse | null>(null);
  let ignoreWhitespace = $state(true);
  let sidebarOpen = $state(getInitialSidebarOpen());
  let themePreference = $state<ThemePreference>(getInitialThemePreference());
  let colorVision = $state<ColorVisionMode>(getInitialColorVision());

  let revQuery = $state('');
  let revQueryError = $state<string | null>(null);
  let collapsedFiles = new SvelteSet<string>();

  let activeRequestController: AbortController | null = null;
  let activeRequestId = 0;

  function startNewRequest(): { signal: AbortSignal; requestId: number } {
    activeRequestController?.abort();
    const controller = new AbortController();
    activeRequestController = controller;
    const requestId = ++activeRequestId;
    return { signal: controller.signal, requestId };
  }

  const selectedChange = $derived(
    jjChanges.find((change) => change.changeId === selectedChangeId),
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

  async function bootstrapApp(signal: AbortSignal, requestId: number) {
    loading = true;
    error = null;
    revQueryError = null;

    try {
      const [jjStatus, initialDiff] = await Promise.all([
        fetchJjStatus(signal),
        fetchDiff({ ignoreWhitespace, signal }),
      ]);
      if (signal.aborted || requestId !== activeRequestId) return;

      const isStdinDiff =
        initialDiff.targetCommitish === 'stdin' || initialDiff.baseCommitish === 'stdin';
      const useJjMode = jjStatus.isJj && !isStdinDiff;
      isJjRepo = useJjMode;
      repoRoot = useJjMode ? jjStatus.repoRoot : undefined;

      if (useJjMode) {
        const { changes } = await fetchJjChanges(undefined, signal);
        if (signal.aborted || requestId !== activeRequestId) return;

        jjChanges = changes;
        const initialChange = pickInitialChange(changes, initialDiff.targetCommitish);
        if (initialChange) {
          const jjDiff = await fetchJjDiff({ target: initialChange.commitId, signal });
          if (signal.aborted || requestId !== activeRequestId) return;
          selectedChangeId = initialChange.changeId;
          diffData = jjDiff;
        } else {
          diffData = initialDiff;
        }
      } else {
        diffData = initialDiff;
      }
    } catch (err) {
      if (isAbortError(err) || signal.aborted || requestId !== activeRequestId) return;
      error = err instanceof Error ? err.message : 'Failed to load diff';
    } finally {
      if (!signal.aborted && requestId === activeRequestId) {
        loading = false;
      }
    }
  }

  onMount(() => {
    const { signal, requestId } = startNewRequest();
    void bootstrapApp(signal, requestId);

    return () => {
      activeRequestController?.abort();
    };
  });

  async function selectJjChange(change: JjChange) {
    const { signal, requestId } = startNewRequest();
    revQueryError = null;
    error = null;

    try {
      const nextDiff = await fetchJjDiff({
        target: change.commitId,
        signal,
      });
      if (signal.aborted || requestId !== activeRequestId) return;
      selectedChangeId = change.changeId;
      diffData = nextDiff;
      collapsedFiles.clear();
    } catch (err) {
      if (isAbortError(err) || signal.aborted || requestId !== activeRequestId) return;
      error = err instanceof Error ? err.message : 'Failed to load Jujutsu diff';
    }
  }

  async function handleRevQuerySubmit(event: SubmitEvent) {
    event.preventDefault();
    const query = revQuery.trim();
    if (!query) return;

    const { signal, requestId } = startNewRequest();
    revQueryError = null;
    error = null;

    try {
      const { changes } = await fetchJjChanges(query, signal);
      if (signal.aborted || requestId !== activeRequestId) return;

      const found = changes[0];
      if (!found) {
        revQueryError = 'Revision not found';
        return;
      }

      const nextDiff = await fetchJjDiff({
        target: found.commitId,
        signal,
      });
      if (signal.aborted || requestId !== activeRequestId) return;

      if (!jjChanges.some((c) => c.changeId === found.changeId)) {
        jjChanges = [found, ...jjChanges];
      }
      selectedChangeId = found.changeId;
      diffData = nextDiff;
      collapsedFiles.clear();
      revQuery = '';
    } catch (err) {
      if (isAbortError(err) || signal.aborted || requestId !== activeRequestId) return;
      revQueryError = err instanceof Error ? err.message : 'Query failed';
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
      diffData = nextDiff;
    } catch (err) {
      if (isAbortError(err) || signal.aborted || requestId !== activeRequestId) return;
      error = err instanceof Error ? err.message : 'Failed to reload diff';
    }
  }

  function handleThemeChange(nextTheme: ThemePreference) {
    themePreference = nextTheme;
    persistThemePreference(nextTheme);
  }

  function toggleFileCollapsed(path: string) {
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

  function formatStatusBadge(status: DiffFile['status']): { label: string; classes: string } {
    switch (status) {
      case 'added':
        return {
          label: 'A',
          classes: 'bg-green-500/15 text-green-400 border-green-500/30',
        };
      case 'deleted':
        return {
          label: 'D',
          classes: 'bg-red-500/15 text-red-400 border-red-500/30',
        };
      case 'renamed':
        return {
          label: 'R',
          classes: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30',
        };
      case 'modified':
        return {
          label: 'M',
          classes: 'bg-blue-500/15 text-blue-400 border-blue-500/30',
        };
    }
  }

  function getChangeDotInfo(change: JjChange): { classes: string; title: string } | null {
    if (change.unresolvedCommentCount > 0) {
      const n = change.unresolvedCommentCount;
      return {
        classes: 'bg-yellow-400',
        title: `${n} unresolved comment${n === 1 ? '' : 's'}`,
      };
    }
    if (change.approved) {
      return {
        classes: 'bg-green-500',
        title: change.commentCount > 0 ? 'Approved (all comments resolved)' : 'Approved',
      };
    }
    if (change.cleanRebaseApproved) {
      return {
        classes: 'bg-emerald-400 border border-emerald-200/60',
        title: 'Approved (Clean Rebase)',
      };
    }
    if (change.commentCount > 0) {
      return {
        classes: 'bg-gray-400',
        title: 'All comments resolved',
      };
    }
    return null;
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
      <div class="flex items-center gap-3 min-w-0">
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
          <span
            class="px-2 py-0.5 text-xs font-mono rounded bg-github-bg-tertiary border border-github-border text-github-text-secondary truncate max-w-md"
            title={repoRoot ?? 'Jujutsu repository'}
          >
            {#if selectedChange}
              {selectedChange.isWorkingCopy ? '@ ' : ''}{selectedChange.shortChangeId}: {selectedChange.description}
            {:else}
              jj:{diffData.commit}
            {/if}
          </span>
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
      </div>

      <div class="flex items-center gap-4 text-xs text-github-text-secondary">
        <div class="flex items-center gap-2 font-mono" aria-label="Diff summary">
          <span>{diffData.files.length} {diffData.files.length === 1 ? 'file' : 'files'}</span>
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

    <div class="flex flex-1 min-h-0 overflow-hidden">
      {#if sidebarOpen}
        <aside
          id="sidebar-panel"
          class="w-72 shrink-0 bg-github-bg-secondary border-r border-github-border flex flex-col min-h-0"
        >
          {#if isJjRepo}
            <section class="border-b border-github-border flex flex-col max-h-[45%]" aria-label="Revisions">
              <div class="px-3 py-2 border-b border-github-border/60 flex items-center justify-between">
                <span class="text-xs font-semibold uppercase tracking-wider text-github-text-muted">
                  Revisions ({jjChanges.length})
                </span>
              </div>

              <form onsubmit={(event) => void handleRevQuerySubmit(event)} class="px-2.5 py-1.5 border-b border-github-border/60 flex items-center gap-1.5">
                <input
                  type="text"
                  bind:value={revQuery}
                  placeholder="Change ID or revset..."
                  aria-label="Jump to change ID or revset"
                  class="flex-1 min-w-0 bg-github-bg-primary border border-github-border rounded px-2 py-1 text-xs font-mono text-github-text-primary"
                />
                <button
                  type="submit"
                  class="px-2 py-1 text-xs font-medium rounded bg-github-accent text-white hover:opacity-90"
                >
                  Go
                </button>
              </form>
              {#if revQueryError}
                <div class="px-2.5 py-1 text-[11px] text-red-400 border-b border-github-border/60">
                  {revQueryError}
                </div>
              {/if}

              <ul class="overflow-y-auto divide-y divide-github-border/40" aria-label="Jujutsu changes">
                {#each jjChanges as change (change.changeId)}
                  {@const isSelected = change.changeId === selectedChangeId}
                  {@const dot = getChangeDotInfo(change)}
                  {@const displayId = change.shortChangeId || change.changeId.slice(0, 8)}
                  {@const prefixLen = change.shortestChangeId
                    ? Math.min(change.shortestChangeId.length, displayId.length)
                    : displayId.length}
                  <li>
                    <button
                      type="button"
                      aria-current={isSelected ? 'true' : undefined}
                      aria-label="{displayId} {change.description}"
                      onclick={() => void selectJjChange(change)}
                      class="w-full px-3 py-1.5 text-left flex items-center gap-1.5 text-xs transition-colors {isSelected
                        ? 'bg-github-bg-tertiary text-github-text-primary font-medium'
                        : 'text-github-text-secondary hover:bg-github-bg-tertiary/60 hover:text-github-text-primary'}"
                    >
                      <span class="font-mono w-3 text-center shrink-0 text-github-accent">
                        {change.isWorkingCopy ? '@' : change.isImmutable ? '◆' : '○'}
                      </span>
                      <span class="font-mono shrink-0">
                        <span class="font-bold text-github-accent">{displayId.slice(0, prefixLen)}</span><span class="text-github-text-muted">{displayId.slice(prefixLen)}</span>
                      </span>
                      {#if change.bookmarks?.length}
                        {#each change.bookmarks as bookmark (bookmark)}
                          <span class="px-1 text-[10px] font-mono rounded bg-github-accent/15 text-github-accent border border-github-accent/30 shrink-0 max-w-20 truncate">
                            {bookmark}
                          </span>
                        {/each}
                      {/if}
                      {#if change.isEmpty}
                        <span class="text-[10px] text-github-text-muted shrink-0">(empty)</span>
                      {/if}
                      <span class="truncate flex-1 min-w-0 {change.hasDescription === false ? 'italic text-github-text-muted' : ''}">
                        {change.description}
                      </span>
                      {#if dot}
                        <span
                          class="inline-block w-2 h-2 rounded-full shrink-0 {dot.classes}"
                          title={dot.title}
                        ></span>
                      {/if}
                    </button>
                  </li>
                {/each}
              </ul>
            </section>
          {/if}

          <section class="flex-1 min-h-0 flex flex-col" aria-label="Changed files">
            <div class="px-3 py-2 border-b border-github-border/60">
              <span class="text-xs font-semibold uppercase tracking-wider text-github-text-muted">
                Files ({diffData.files.length})
              </span>
            </div>
            <ul class="flex-1 overflow-y-auto py-1">
              {#each diffData.files as file (file.path)}
                {@const badge = formatStatusBadge(file.status)}
                <li>
                  <button
                    type="button"
                    onclick={() => scrollToFile(file.path)}
                    class="w-full px-3 py-1.5 text-left flex items-center justify-between gap-2 text-xs text-github-text-secondary hover:bg-github-bg-tertiary hover:text-github-text-primary"
                  >
                    <span class="flex items-center gap-2 min-w-0">
                      <span class="inline-flex items-center justify-center w-4 h-4 text-[10px] font-mono font-semibold rounded border shrink-0 {badge.classes}">
                        {badge.label}
                      </span>
                      <span class="font-mono truncate">{file.path}</span>
                    </span>
                    <span class="font-mono text-[11px] shrink-0 flex items-center gap-1">
                      {#if file.additions > 0}
                        <span class="text-green-400">+{file.additions}</span>
                      {/if}
                      {#if file.deletions > 0}
                        <span class="text-red-400">-{file.deletions}</span>
                      {/if}
                    </span>
                  </button>
                </li>
              {/each}
            </ul>
          </section>
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
                </button>
                <div class="font-mono text-xs flex items-center gap-2 shrink-0">
                  <span class="text-green-400">+{file.additions}</span>
                  <span class="text-red-400">-{file.deletions}</span>
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
