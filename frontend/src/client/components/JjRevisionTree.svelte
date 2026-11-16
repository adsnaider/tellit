<script lang="ts">
  import { type JjChange, type JjGraphRow } from '../../types/diff';
  import {
    computeGraphLayout,
    type GraphCellLayout,
    getReviewStatusDot,
    makeStandaloneCommitRow,
  } from '../utils/jjGraph';

  const REVISIONS_TREE_STORAGE_KEY = 'tellit.revisionsTreeOpen';

  const COL_WIDTH = 11;
  const COMMIT_ROW_HEIGHT = 28;
  const CONNECTOR_ROW_HEIGHT = 16;
  const STEM_CLEARANCE = 5;

  interface Props {
    changes: JjChange[];
    graphRows: JjGraphRow[];
    selectedChangeId: string;
    onSelectChange: (changeId: string) => void;
    onQuerySubmit?: (query: string) => Promise<boolean> | boolean;
    queryError?: string | null;
    onClearQueryError?: () => void;
    loading?: boolean;
    embedded?: boolean;
  }

  let {
    changes,
    graphRows,
    selectedChangeId,
    onSelectChange,
    onQuerySubmit,
    queryError = null,
    onClearQueryError,
    loading = false,
    embedded = false,
  }: Props = $props();

  function getInitialOpen(): boolean {
    if (typeof window === 'undefined') return true;
    try {
      const saved = window.localStorage.getItem(REVISIONS_TREE_STORAGE_KEY);
      if (saved === 'false') return false;
      if (saved === 'true') return true;
    } catch {
      // Ignore storage access failures.
    }
    return true;
  }

  let isOpen = $state(getInitialOpen());
  let isSearchOpen = $state(false);
  let searchQuery = $state('');

  const effectiveGraphRows = $derived<JjGraphRow[]>(
    graphRows.length > 0 || changes.length === 0
      ? graphRows
      : changes.map(makeStandaloneCommitRow),
  );

  const layout = $derived(computeGraphLayout(effectiveGraphRows));
  const maxCols = $derived(
    Math.max(1, ...effectiveGraphRows.map((row) => row.graphCols.length)),
  );
  const gutterWidth = $derived(maxCols * COL_WIDTH);
  const selectedChange = $derived(changes.find((c) => c.changeId === selectedChangeId));
  const selectedChangeDot = $derived(selectedChange ? getReviewStatusDot(selectedChange) : null);

  function toggleOpen() {
    isOpen = !isOpen;
    if (typeof window !== 'undefined') {
      try {
        window.localStorage.setItem(REVISIONS_TREE_STORAGE_KEY, String(isOpen));
      } catch {
        // Ignore storage access failures.
      }
    }
  }

  function toggleSearch() {
    if (!isOpen) {
      isOpen = true;
      if (typeof window !== 'undefined') {
        try {
          window.localStorage.setItem(REVISIONS_TREE_STORAGE_KEY, 'true');
        } catch {
          // Ignore storage access failures.
        }
      }
    }
    isSearchOpen = !isSearchOpen;
    if (!isSearchOpen) {
      searchQuery = '';
    }
    onClearQueryError?.();
  }

  async function handleSearchFormSubmit(event: SubmitEvent) {
    event.preventDefault();
    const trimmed = searchQuery.trim();
    if (!trimmed || !onQuerySubmit) return;
    const ok = await onQuerySubmit(trimmed);
    if (ok) {
      searchQuery = '';
      isSearchOpen = false;
    }
  }
</script>

{#snippet renderGraphCell(cell: GraphCellLayout, colIdx: number, rowHeight: number, isSelected: boolean)}
  {@const xLeft = colIdx * COL_WIDTH}
  {@const xRight = xLeft + COL_WIDTH}
  {@const cx = xLeft + COL_WIDTH / 2}
  {@const cy = rowHeight / 2}
  {@const isPureCorner =
    !cell.nodeKind &&
    !cell.isElided &&
    !cell.diagonal &&
    Number(cell.up) + Number(cell.down) + Number(cell.left) + Number(cell.right) === 2 &&
    (cell.up || cell.down) &&
    (cell.left || cell.right)}
  {@const hasNodeOrElision = Boolean(cell.nodeKind || cell.isElided)}
  {@const upEnd = hasNodeOrElision ? Math.max(0, cy - STEM_CLEARANCE) : cy}
  {@const downStart = hasNodeOrElision ? Math.min(rowHeight, cy + STEM_CLEARANCE) : cy}

  <g>
    {#if isPureCorner}
      <path
        d={`M ${cx} ${cell.up ? 0 : rowHeight} Q ${cx} ${cy} ${cell.left ? xLeft : xRight} ${cy}`}
        fill="none"
        class="stroke-github-text-muted/70"
        stroke-width="1.5"
        stroke-linecap="round"
      />
    {:else}
      {#if cell.up}
        <line
          x1={cx}
          y1={0}
          x2={cx}
          y2={upEnd}
          class="stroke-github-text-muted/70"
          stroke-width="1.5"
          stroke-dasharray={cell.dashedDown && !hasNodeOrElision ? '2 2' : undefined}
        />
      {/if}
      {#if cell.down}
        <line
          x1={cx}
          y1={downStart}
          x2={cx}
          y2={rowHeight}
          class="stroke-github-text-muted/70"
          stroke-width="1.5"
          stroke-dasharray={cell.dashedDown ? '2 2' : undefined}
        />
      {/if}
      {#if cell.left}
        <line
          x1={xLeft}
          y1={cy}
          x2={cx}
          y2={cy}
          class="stroke-github-text-muted/70"
          stroke-width="1.5"
        />
      {/if}
      {#if cell.right}
        <line
          x1={cx}
          y1={cy}
          x2={xRight}
          y2={cy}
          class="stroke-github-text-muted/70"
          stroke-width="1.5"
        />
      {/if}
    {/if}

    {#if cell.diagonal === '/'}
      <line
        x1={xLeft}
        y1={rowHeight}
        x2={xRight}
        y2={0}
        class="stroke-github-text-muted/70"
        stroke-width="1.5"
      />
    {:else if cell.diagonal === '\\'}
      <line
        x1={xLeft}
        y1={0}
        x2={xRight}
        y2={rowHeight}
        class="stroke-github-text-muted/70"
        stroke-width="1.5"
      />
    {/if}

    {#if cell.isElided}
      <text
        x={cx}
        y={cy}
        text-anchor="middle"
        dominant-baseline="central"
        class="fill-github-text-muted font-mono text-[11px]"
        data-elided="true"
      >
        ~
      </text>
    {/if}

    {#if cell.nodeKind === 'working'}
      <text
        x={cx}
        y={cy}
        text-anchor="middle"
        dominant-baseline="central"
        class="fill-github-accent font-mono text-[11px] font-bold"
        data-node-kind="working"
      >
        @
      </text>
    {:else if cell.nodeKind === 'mutable'}
      <circle
        cx={cx}
        cy={cy}
        r="3.5"
        class={isSelected
          ? 'fill-github-bg-tertiary stroke-github-accent'
          : 'fill-github-bg-secondary stroke-github-text-secondary'}
        stroke-width="1.6"
        data-node-kind="mutable"
      />
    {:else if cell.nodeKind === 'immutable'}
      <polygon
        points={`${cx},${cy - 4} ${cx + 4},${cy} ${cx},${cy + 4} ${cx - 4},${cy}`}
        class="fill-github-text-muted stroke-github-text-muted"
        stroke-width="1"
        data-node-kind="immutable"
      />
    {:else if cell.nodeKind === 'conflict'}
      <path
        d={`M ${cx - 3} ${cy - 3} L ${cx + 3} ${cy + 3} M ${cx + 3} ${cy - 3} L ${cx - 3} ${cy + 3}`}
        class="stroke-red-400"
        stroke-width="1.75"
        stroke-linecap="round"
        data-node-kind="conflict"
      />
    {:else if cell.nodeKind === 'hidden'}
      <circle
        cx={cx}
        cy={cy}
        r="3.5"
        class="fill-none stroke-github-text-muted"
        stroke-width="1.5"
        stroke-dasharray="2 1.5"
        data-node-kind="hidden"
      />
    {/if}
  </g>
{/snippet}

{#snippet renderTreeList(listClass: string)}
  {#if loading && effectiveGraphRows.length === 0}
    <div class="{listClass} px-3 py-2 text-xs text-github-text-secondary">
      Loading revisions...
    </div>
  {:else if effectiveGraphRows.length === 0}
    <div class="{listClass} px-3 py-2 text-xs text-github-text-muted">
      No revisions found
    </div>
  {:else}
    <ul class={listClass} aria-label="Jujutsu revisions">
      {#each effectiveGraphRows as row, rowIdx (row.key)}
        {@const rowLayout = layout[rowIdx]}
        {#if rowLayout}
          {#if row.type === 'connector'}
            <li
              aria-hidden="true"
              class="h-4 flex items-center px-2 select-none pointer-events-none"
            >
              <svg width={gutterWidth} height={CONNECTOR_ROW_HEIGHT} class="shrink-0">
                {#each rowLayout.cols as cell, colIdx (colIdx)}
                  {@render renderGraphCell(cell, colIdx, CONNECTOR_ROW_HEIGHT, false)}
                {/each}
              </svg>
            </li>
          {:else}
            {@const change = row.change}
            {@const isSelected = change.changeId === selectedChangeId}
            {@const isMuted = Boolean(
              change.isEmpty && change.hasDescription === false && !isSelected,
            )}
            {@const displayId = change.shortChangeId || change.changeId.slice(0, 8)}
            {@const shortestLen = change.shortestChangeId
              ? Math.min(change.shortestChangeId.length, displayId.length)
              : 0}
            {@const idPrefix = shortestLen > 0 ? displayId.slice(0, shortestLen) : displayId}
            {@const idSuffix = shortestLen > 0 ? displayId.slice(shortestLen) : ''}
            {@const dot = getReviewStatusDot(change)}
            <li>
              <button
                type="button"
                aria-current={isSelected ? 'true' : undefined}
                aria-label={`${displayId}${change.bookmarks?.length ? ` ${change.bookmarks.join(' ')}` : ''}${change.isConflict ? ' (conflict)' : ''}${change.isEmpty ? ' (empty)' : ''} ${change.description}`}
                onclick={() => onSelectChange(change.changeId)}
                class="w-full h-7 px-2 text-left flex items-center gap-1.5 transition-colors cursor-pointer {isSelected
                  ? 'bg-github-bg-tertiary text-github-text-primary font-medium'
                  : isMuted
                    ? 'opacity-55 hover:opacity-90 hover:bg-github-bg-tertiary/60 text-github-text-secondary'
                    : 'text-github-text-secondary hover:bg-github-bg-tertiary/60 hover:text-github-text-primary'}"
              >
                <svg
                  width={gutterWidth}
                  height={COMMIT_ROW_HEIGHT}
                  class="shrink-0"
                  aria-hidden="true"
                >
                  {#each rowLayout.cols as cell, colIdx (colIdx)}
                    {@render renderGraphCell(cell, colIdx, COMMIT_ROW_HEIGHT, isSelected)}
                  {/each}
                </svg>

                <span class="font-mono text-[11px] shrink-0">
                  <span class="font-bold text-github-accent">{idPrefix}</span>{#if idSuffix}<span class="text-github-text-muted">{idSuffix}</span>{/if}
                </span>

                {#if change.bookmarks?.length}
                  {#each change.bookmarks as bookmark (bookmark)}
                    <span
                      class="px-1 py-0 text-[10px] font-mono rounded bg-github-accent/15 text-github-accent border border-github-accent/30 shrink-0 max-w-[90px] truncate"
                      title={bookmark}
                    >
                      {bookmark}
                    </span>
                  {/each}
                {/if}

                {#if change.isConflict}
                  <span class="px-1 py-0 text-[10px] font-mono rounded bg-red-500/15 text-red-400 border border-red-500/30 shrink-0">
                    conflict
                  </span>
                {/if}

                {#if change.isEmpty}
                  <span class="text-[10px] text-github-text-muted shrink-0">(empty)</span>
                {/if}

                <span
                  class="text-xs truncate min-w-0 flex-1 {change.hasDescription === false
                    ? 'text-github-text-muted italic'
                    : isSelected
                      ? 'text-github-text-primary'
                      : 'text-github-text-secondary'}"
                >
                  {change.description}
                </span>

                {#if dot}
                  <span
                    class="inline-block rounded-full shrink-0 {dot.classes}"
                    title={dot.title}
                    data-status-dot={dot.kind}
                  ></span>
                {/if}
              </button>
            </li>
          {/if}
        {/if}
      {/each}
    </ul>
  {/if}
{/snippet}

{#if embedded}
  {@render renderTreeList('max-h-60 overflow-y-auto overflow-x-auto')}
{:else}
  <section class="shrink-0 flex flex-col" aria-label="Revisions">
    <div class="flex items-center justify-between px-2.5 py-2 border-b border-github-border bg-github-bg-secondary">
      <button
        type="button"
        onclick={toggleOpen}
        aria-expanded={isOpen}
        class="flex items-center gap-1.5 min-w-0 flex-1 text-left text-xs font-semibold text-github-text-primary hover:text-github-accent cursor-pointer"
      >
        <span class="text-github-text-muted w-3 text-center shrink-0">
          {isOpen ? '▾' : '▸'}
        </span>
        <span>Revisions</span>
        {#if !isOpen && selectedChange}
          <span class="ml-1 flex items-center gap-1 font-mono text-[11px] font-normal text-github-text-secondary truncate">
            <span>{selectedChange.shortChangeId}</span>
            {#if selectedChangeDot}
              <span
                class="inline-block rounded-full shrink-0 {selectedChangeDot.classes}"
                title={selectedChangeDot.title}
              ></span>
            {/if}
          </span>
        {/if}
      </button>

      {#if onQuerySubmit}
        <button
          type="button"
          onclick={toggleSearch}
          class="text-github-text-muted hover:text-github-text-primary px-1.5 py-0.5 text-xs rounded hover:bg-github-bg-tertiary cursor-pointer shrink-0"
          title="Enter Change ID or revset"
          aria-label="Search revision"
        >
          Find
        </button>
      {/if}
    </div>

    {#if isOpen && isSearchOpen && onQuerySubmit}
      <div class="border-b border-github-border bg-github-bg-tertiary/50 px-2 py-1.5">
        <form onsubmit={(event) => void handleSearchFormSubmit(event)} class="flex items-center gap-1">
          <input
            type="text"
            bind:value={searchQuery}
            placeholder="e.g. kkmpptxz or @-"
            aria-label="Jump to change ID or revset"
            class="flex-1 min-w-0 bg-github-bg-secondary text-github-text-primary text-xs font-mono px-1.5 py-0.5 rounded border border-github-border outline-none focus:border-github-accent"
          />
          <button
            type="submit"
            class="bg-github-accent text-white px-1.5 py-0.5 rounded text-[11px] font-semibold hover:opacity-90 cursor-pointer shrink-0"
          >
            Go
          </button>
          <button
            type="button"
            onclick={() => {
              isSearchOpen = false;
              searchQuery = '';
              onClearQueryError?.();
            }}
            class="text-github-text-muted hover:text-github-text-primary px-1 text-xs cursor-pointer shrink-0"
            title="Cancel"
            aria-label="Cancel revision search"
          >
            ✕
          </button>
        </form>
        {#if queryError}
          <div class="mt-1 text-[10px] text-red-400">{queryError}</div>
        {/if}
      </div>
    {/if}

    {#if isOpen}
      {@render renderTreeList('max-h-[38vh] overflow-y-auto overflow-x-auto border-b border-github-border')}
    {/if}
  </section>
{/if}
