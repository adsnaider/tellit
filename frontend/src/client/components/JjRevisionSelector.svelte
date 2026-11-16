<script lang="ts">
  import { type JjChange, type JjCommit, type JjGraphRow } from '../../types/diff';
  import { getReviewStatusDot } from '../utils/jjGraph';
  import {
    commitIdsMatch,
    filterInterestingCommits,
    type IndexedJjCommit,
  } from '../utils/jjSelection';

  import JjRevisionTree from './JjRevisionTree.svelte';

  interface Props {
    changes: JjChange[];
    graphRows: JjGraphRow[];
    selectedChangeId: string;
    commits: JjCommit[];
    targetCommitId: string;
    baseCommitId: string;
    isRebaseDiff?: boolean;
    rebaseIgnored?: boolean;
    rebaseConflict?: boolean;
    loadingChanges?: boolean;
    loadingCommits?: boolean;
    approving?: boolean;
    queryError?: string | null;
    onClearQueryError?: () => void;
    onSelectChange: (changeId: string) => void;
    onQueryChange: (query: string) => Promise<boolean> | boolean;
    onSelectBase: (baseCommitId: string) => void;
    onSelectTarget: (targetCommitId: string) => void;
    onToggleIgnoreRebase: () => void;
    onToggleApprove: (commitId: string, currentApproved: boolean) => void;
  }

  let {
    changes,
    graphRows,
    selectedChangeId,
    commits,
    targetCommitId,
    baseCommitId,
    isRebaseDiff = false,
    rebaseIgnored = false,
    rebaseConflict = false,
    loadingChanges = false,
    loadingCommits = false,
    approving = false,
    queryError = null,
    onClearQueryError,
    onSelectChange,
    onQueryChange,
    onSelectBase,
    onSelectTarget,
    onToggleIgnoreRebase,
    onToggleApprove,
  }: Props = $props();

  let rootEl = $state<HTMLDivElement | null>(null);
  let isChangeOpen = $state(false);
  let isCustomChangeInput = $state(false);
  let customChangeQuery = $state('');

  let isFromOpen = $state(false);
  let isToOpen = $state(false);
  let showAllFrom = $state(false);
  let showAllTo = $state(false);

  const selectedChange = $derived(changes.find((c) => c.changeId === selectedChangeId));
  const selectedChangeDot = $derived(selectedChange ? getReviewStatusDot(selectedChange) : null);

  const reversedCommits = $derived<IndexedJjCommit[]>(
    commits
      .map((commit, originalIndex) => ({
        commit,
        commitNumber: originalIndex + 1,
      }))
      .reverse(),
  );

  const interestingReversedCommits = $derived<IndexedJjCommit[]>(
    filterInterestingCommits(commits, { targetCommitId, baseCommitId }),
  );

  const hiddenCount = $derived(reversedCommits.length - interestingReversedCommits.length);
  const visibleFromCommits = $derived(
    showAllFrom || hiddenCount === 0 ? reversedCommits : interestingReversedCommits,
  );
  const visibleToCommits = $derived(
    showAllTo || hiddenCount === 0 ? reversedCommits : interestingReversedCommits,
  );

  const activeTargetIndex = $derived(
    commits.findIndex((c) => commitIdsMatch(c.commitId, targetCommitId)),
  );
  const activeTargetCommit = $derived(
    activeTargetIndex >= 0 ? commits[activeTargetIndex] : undefined,
  );
  const activeTargetDot = $derived(
    activeTargetCommit ? getReviewStatusDot(activeTargetCommit) : null,
  );

  const activeBaseIndex = $derived(
    commits.findIndex((c) => commitIdsMatch(c.commitId, baseCommitId)),
  );
  const activeBaseCommit = $derived(activeBaseIndex >= 0 ? commits[activeBaseIndex] : undefined);
  const isBaseParent = $derived(baseCommitId === 'parent');
  const activeBaseDot = $derived(
    activeBaseCommit && !isBaseParent ? getReviewStatusDot(activeBaseCommit) : null,
  );

  function closeAllMenus() {
    isChangeOpen = false;
    isFromOpen = false;
    isToOpen = false;
    showAllFrom = false;
    showAllTo = false;
  }

  function handleWindowClick(event: MouseEvent) {
    if (!rootEl) return;
    const target = event.target;
    if (target instanceof Node && !rootEl.contains(target)) {
      closeAllMenus();
    }
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      closeAllMenus();
      if (isCustomChangeInput) {
        isCustomChangeInput = false;
        customChangeQuery = '';
        onClearQueryError?.();
      }
    }
  }

  async function handleCustomChangeSubmit(event: SubmitEvent) {
    event.preventDefault();
    const trimmed = customChangeQuery.trim();
    if (!trimmed) return;
    const ok = await onQueryChange(trimmed);
    if (ok) {
      customChangeQuery = '';
      isCustomChangeInput = false;
      isChangeOpen = false;
    }
  }

  function handleApproveClick() {
    if (!activeTargetCommit) return;
    if (activeTargetCommit.cleanRebaseApproved && !activeTargetCommit.approved) {
      onToggleApprove(activeTargetCommit.commitId, false);
    } else {
      onToggleApprove(activeTargetCommit.commitId, Boolean(activeTargetCommit.approved));
    }
  }
</script>

<svelte:window onclick={handleWindowClick} onkeydown={handleWindowKeydown} />

{#snippet renderCommitOptionRow(item: IndexedJjCommit, isSelected: boolean, isTarget: boolean, onPick: () => void)}
  {@const dot = getReviewStatusDot(item.commit)}
  <button
    type="button"
    aria-label={`#${item.commitNumber}: ${item.commit.shortCommitId} ${item.commit.description}`}
    onclick={onPick}
    class="w-full px-2.5 py-1.5 text-left text-xs font-mono flex items-center justify-between gap-2 transition-colors cursor-pointer {isSelected
      ? 'bg-github-bg-tertiary text-github-accent font-medium'
      : 'text-github-text-primary hover:bg-github-bg-tertiary'}"
  >
    <div class="flex items-center gap-1.5 min-w-0 truncate">
      <span class="text-github-text-muted shrink-0 text-[10px]">#{item.commitNumber}</span>
      <span class="font-semibold text-github-text-primary shrink-0">
        {item.commit.shortCommitId}
      </span>
      {#if item.commit.timestamp}
        <span class="text-github-text-muted shrink-0 text-[10px]">
          [{item.commit.timestamp.slice(5, 16)}]
        </span>
      {/if}
      <span class="text-github-text-secondary truncate text-[11px]">{item.commit.description}</span>
      {#if isTarget}
        <span class="text-[10px] text-github-text-muted italic shrink-0">(head)</span>
      {/if}
    </div>
    {#if dot}
      <span class="inline-block rounded-full shrink-0 {dot.classes}" title={dot.title}></span>
    {/if}
  </button>
{/snippet}

{#snippet renderVisibilityFooter(showAll: boolean, onToggle: () => void)}
  {#if hiddenCount > 0}
    <div class="border-t border-github-border/50 mt-1 pt-1">
      <button
        type="button"
        onclick={onToggle}
        class="w-full px-2.5 py-1.5 text-left text-xs text-github-accent hover:bg-github-bg-tertiary flex items-center justify-between gap-2 cursor-pointer font-sans"
      >
        {#if showAll}
          <span>
            Hide {hiddenCount} unreviewed {hiddenCount === 1 ? 'revision' : 'revisions'}
          </span>
        {:else}
          <span>Show all {commits.length} revisions</span>
          <span class="text-[10px] text-github-text-muted">{hiddenCount} hidden</span>
        {/if}
      </button>
    </div>
  {/if}
{/snippet}

<div bind:this={rootEl} class="flex flex-wrap items-center gap-2">
  {#if loadingChanges && changes.length === 0}
    <span class="text-xs text-github-text-secondary">Loading changes...</span>
  {:else if changes.length > 0}
    <div class="relative">
      <div class="flex items-center gap-1.5 bg-github-bg-tertiary border border-github-border rounded px-2 py-1 text-xs">
        <span class="font-semibold text-github-text-secondary">Change:</span>
        {#if isCustomChangeInput}
          <form onsubmit={(event) => void handleCustomChangeSubmit(event)} class="flex items-center gap-1">
            <input
              type="text"
              bind:value={customChangeQuery}
              placeholder="e.g. kkmpptxz or @-"
              aria-label="Enter change ID or revset"
              class="bg-github-bg-secondary text-github-text-primary text-xs font-mono px-1.5 py-0.5 rounded border border-github-border outline-none w-[150px]"
            />
            <button
              type="submit"
              class="bg-github-accent text-white px-1.5 py-0.5 rounded text-[11px] font-semibold hover:opacity-90 cursor-pointer"
            >
              Go
            </button>
            <button
              type="button"
              onclick={() => {
                isCustomChangeInput = false;
                customChangeQuery = '';
                onClearQueryError?.();
              }}
              aria-label="Cancel change lookup"
              class="text-github-text-muted hover:text-github-text-primary px-0.5 cursor-pointer"
            >
              ✕
            </button>
            {#if queryError}
              <span class="text-red-400 text-[10px] whitespace-nowrap">{queryError}</span>
            {/if}
          </form>
        {:else}
          <div class="flex items-center gap-1">
            <button
              type="button"
              aria-label="Select Jujutsu change"
              aria-expanded={isChangeOpen}
              onclick={() => {
                const next = !isChangeOpen;
                closeAllMenus();
                isChangeOpen = next;
              }}
              class="flex items-center gap-1.5 text-github-text-primary text-xs font-mono outline-none cursor-pointer max-w-[220px] truncate hover:text-github-accent"
            >
              <span class="truncate">
                {#if selectedChange}
                  {selectedChange.isWorkingCopy ? '@ ' : ''}{selectedChange.shortChangeId} {selectedChange.description.slice(0, 22)}
                {:else}
                  Select change...
                {/if}
              </span>
              {#if selectedChangeDot}
                <span
                  class="inline-block rounded-full shrink-0 {selectedChangeDot.classes}"
                  title={selectedChangeDot.title}
                ></span>
              {/if}
              <span class="text-github-text-muted shrink-0">▾</span>
            </button>

            <button
              type="button"
              onclick={() => {
                closeAllMenus();
                onClearQueryError?.();
                isCustomChangeInput = true;
              }}
              aria-label="Enter specific change ID"
              title="Enter specific change ID"
              class="text-github-text-muted hover:text-github-text-primary px-1 cursor-pointer"
            >
              Find
            </button>
          </div>
        {/if}
      </div>

      {#if isChangeOpen}
        <div class="absolute left-0 top-full mt-1 z-50 bg-github-bg-secondary border border-github-border rounded-md shadow-xl py-1 w-80">
          <div class="px-2.5 py-1 text-[11px] font-semibold text-github-text-muted uppercase tracking-wider border-b border-github-border/50">
            Changes
          </div>
          <JjRevisionTree
            embedded
            {changes}
            {graphRows}
            {selectedChangeId}
            onSelectChange={(changeId) => {
              onSelectChange(changeId);
              isChangeOpen = false;
            }}
          />
          <div class="border-t border-github-border/50 mt-1 pt-1">
            <button
              type="button"
              onclick={() => {
                isChangeOpen = false;
                onClearQueryError?.();
                isCustomChangeInput = true;
              }}
              class="w-full px-2.5 py-1.5 text-left text-xs text-github-accent hover:bg-github-bg-tertiary flex items-center gap-1.5 cursor-pointer font-sans"
            >
              <span>Enter Change ID / Rev...</span>
            </button>
          </div>
        </div>
      {/if}
    </div>
  {/if}

  {#if loadingCommits && commits.length === 0}
    <span class="text-xs text-github-text-secondary">Loading commits...</span>
  {:else if commits.length > 0}
    <div class="flex flex-wrap items-center gap-1.5 text-xs font-mono">
      <div class="relative">
        <button
          type="button"
          aria-label="Diff base revision (From)"
          aria-expanded={isFromOpen}
          onclick={() => {
            const next = !isFromOpen;
            closeAllMenus();
            isFromOpen = next;
          }}
          class="flex items-center gap-1.5 bg-github-bg-tertiary border border-github-border rounded px-2 py-1 text-github-text-primary hover:text-github-accent cursor-pointer max-w-[190px] truncate"
          title="Select diff base revision (From)"
        >
          <span class="truncate">
            {#if isBaseParent || !activeBaseCommit}
              Parent
            {:else}
              #{activeBaseIndex + 1}: {activeBaseCommit.shortCommitId}
            {/if}
          </span>
          {#if activeBaseDot}
            <span class="inline-block rounded-full shrink-0 {activeBaseDot.classes}" title={activeBaseDot.title}></span>
          {/if}
          <span class="text-github-text-muted shrink-0">▾</span>
        </button>

        {#if isFromOpen}
          <div class="absolute left-0 top-full mt-1 z-50 bg-github-bg-secondary border border-github-border rounded-md shadow-xl py-1 w-80">
            <div class="px-2.5 py-1 text-[11px] font-semibold text-github-text-muted uppercase tracking-wider border-b border-github-border/50">
              Diff Base (From)
            </div>
            <div class="max-h-60 overflow-y-auto">
              <button
                type="button"
                onclick={() => {
                  onSelectBase('parent');
                  closeAllMenus();
                }}
                class="w-full px-2.5 py-1.5 text-left text-xs font-mono flex items-center justify-between gap-2 transition-colors cursor-pointer {isBaseParent
                  ? 'bg-github-bg-tertiary text-github-accent font-medium'
                  : 'text-github-text-primary hover:bg-github-bg-tertiary'}"
              >
                <div class="flex items-center gap-1.5 min-w-0">
                  <span class="font-semibold text-github-text-primary">Parent</span>
                  <span class="text-github-text-secondary text-[11px]">(Entire Change)</span>
                </div>
              </button>

              {#each visibleFromCommits as item (item.commit.commitId)}
                {@render renderCommitOptionRow(
                  item,
                  !isBaseParent && commitIdsMatch(item.commit.commitId, baseCommitId),
                  commitIdsMatch(item.commit.commitId, targetCommitId),
                  () => {
                    onSelectBase(item.commit.commitId);
                    closeAllMenus();
                  },
                )}
              {/each}
            </div>
            {@render renderVisibilityFooter(showAllFrom, () => {
              showAllFrom = !showAllFrom;
            })}
          </div>
        {/if}
      </div>

      <span class="text-github-text-muted shrink-0" aria-hidden="true">-&gt;</span>

      <div class="relative">
        <button
          type="button"
          aria-label="Diff head revision (To)"
          aria-expanded={isToOpen}
          onclick={() => {
            const next = !isToOpen;
            closeAllMenus();
            isToOpen = next;
          }}
          class="flex items-center gap-1.5 bg-github-bg-tertiary border border-github-border rounded px-2 py-1 text-github-text-primary hover:text-github-accent cursor-pointer max-w-[240px] truncate"
          title="Select diff head revision (To)"
        >
          <span class="truncate">
            {#if activeTargetCommit}
              #{activeTargetIndex + 1}: {activeTargetCommit.shortCommitId}{activeTargetCommit.description ? `: ${activeTargetCommit.description.slice(0, 16)}` : ''}
            {:else}
              Select commit...
            {/if}
          </span>
          {#if activeTargetDot}
            <span class="inline-block rounded-full shrink-0 {activeTargetDot.classes}" title={activeTargetDot.title}></span>
          {/if}
          <span class="text-github-text-muted shrink-0">▾</span>
        </button>

        {#if isToOpen}
          <div class="absolute left-0 top-full mt-1 z-50 bg-github-bg-secondary border border-github-border rounded-md shadow-xl py-1 w-80">
            <div class="px-2.5 py-1 text-[11px] font-semibold text-github-text-muted uppercase tracking-wider border-b border-github-border/50">
              Diff Head / Commits ({showAllTo || hiddenCount === 0
                ? commits.length
                : `${interestingReversedCommits.length} of ${commits.length}`})
            </div>
            <div class="max-h-60 overflow-y-auto">
              {#each visibleToCommits as item (item.commit.commitId)}
                {@render renderCommitOptionRow(
                  item,
                  commitIdsMatch(item.commit.commitId, targetCommitId),
                  false,
                  () => {
                    onSelectTarget(item.commit.commitId);
                    closeAllMenus();
                  },
                )}
              {/each}
            </div>
            {@render renderVisibilityFooter(showAllTo, () => {
              showAllTo = !showAllTo;
            })}
          </div>
        {/if}
      </div>

      {#if isBaseParent}
        <span
          class="flex items-center gap-1 px-2 py-1 rounded text-xs font-sans text-github-text-muted/60 border border-github-border/40 bg-github-bg-secondary/40 cursor-not-allowed select-none"
          title="Upstream rebase filtering is available when comparing two patchsets (select an earlier commit in From)"
        >
          <span>Rebase filter (interdiff only)</span>
        </span>
      {:else if isRebaseDiff}
        <button
          type="button"
          onclick={onToggleIgnoreRebase}
          class="flex items-center gap-1 px-2 py-1 rounded text-xs font-sans font-medium border transition-colors cursor-pointer {rebaseConflict
            ? 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30 hover:bg-yellow-500/25'
            : rebaseIgnored
              ? 'bg-blue-500/15 text-blue-400 border-blue-500/30 hover:bg-blue-500/25'
              : 'bg-github-bg-tertiary text-github-text-secondary border-github-border hover:text-github-text-primary'}"
          title={rebaseConflict
            ? 'Rebase conflict detected: showing full diff including upstream changes.'
            : rebaseIgnored
              ? 'Upstream rebase changes are hidden. Click to show full diff.'
              : 'Showing full diff including upstream changes. Click to hide rebase changes.'}
        >
          {#if rebaseConflict}
            <span>Rebase conflict</span>
          {:else if rebaseIgnored}
            <span>Rebase hidden</span>
          {:else}
            <span>Show rebase</span>
          {/if}
        </button>
      {:else}
        <span
          class="flex items-center gap-1 px-2 py-1 rounded text-xs font-sans font-medium text-github-text-muted border border-github-border/50 bg-github-bg-tertiary/50 cursor-not-allowed select-none"
          title={activeBaseIndex >= 0 && activeTargetIndex >= 0
            ? `Patchsets #${activeBaseIndex + 1} and #${activeTargetIndex + 1} share the same parent commit: no upstream rebase changes to hide.`
            : 'These patchsets share the same parent commit: no upstream rebase changes to hide.'}
        >
          <span>No rebase (same base)</span>
        </span>
      {/if}

      {#if activeTargetCommit}
        <button
          type="button"
          disabled={approving}
          onclick={handleApproveClick}
          class="flex items-center gap-1.5 px-2.5 py-1 rounded text-xs font-sans font-medium border transition-all cursor-pointer disabled:opacity-60 {activeTargetCommit.approved
            ? 'bg-green-500/15 text-green-400 border-green-500/40 hover:bg-green-500/25'
            : activeTargetCommit.cleanRebaseApproved
              ? 'bg-emerald-500/15 text-emerald-400 border-emerald-500/40 hover:bg-emerald-500/25'
              : 'bg-github-bg-tertiary text-github-text-secondary border-github-border hover:text-github-text-primary hover:border-github-accent'}"
          title={activeTargetCommit.approved
            ? 'Approved (click to revoke approval)'
            : activeTargetCommit.cleanRebaseApproved
              ? `Approved via clean rebase${activeTargetCommit.cleanRebaseSourceCommitNumber ? ` from #${activeTargetCommit.cleanRebaseSourceCommitNumber}` : ''} (click to record direct approval)`
              : 'Approve this revision'}
        >
          <span>
            {activeTargetCommit.approved
              ? 'Approved'
              : activeTargetCommit.cleanRebaseApproved
                ? 'Approved (Clean Rebase)'
                : 'Approve'}
          </span>
        </button>
      {/if}
    </div>
  {/if}
</div>
