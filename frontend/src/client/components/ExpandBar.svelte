<script lang="ts">
  import { DEFAULT_EXPAND_COUNT } from '../utils/expandedLines';

  interface Props {
    direction: 'up' | 'down' | 'both';
    hiddenLines: number;
    isLoading?: boolean;
    onExpandUp?: () => void;
    onExpandDown?: () => void;
    onExpandAll?: () => void;
  }

  let {
    direction,
    hiddenLines,
    isLoading = false,
    onExpandUp,
    onExpandDown,
    onExpandAll,
  }: Props = $props();

  const showOnlyExpandAll = $derived(hiddenLines <= DEFAULT_EXPAND_COUNT);
  const lineLabel = $derived(`${hiddenLines} ${hiddenLines === 1 ? 'line' : 'lines'} hidden`);
</script>

{#if hiddenLines > 0}
  <div class="flex items-center justify-between gap-3 px-3 py-1 bg-github-bg-tertiary/70 border-y border-github-border/70 text-xs text-github-text-secondary select-none">
    <div class="flex items-center gap-1.5">
      {#if isLoading}
        <span class="font-mono text-github-text-muted">Loading...</span>
      {:else if showOnlyExpandAll}
        <button
          type="button"
          onclick={onExpandAll}
          disabled={!onExpandAll}
          class="px-2 py-0.5 rounded border border-github-border bg-github-bg-secondary text-github-text-primary hover:bg-github-bg-primary cursor-pointer disabled:opacity-50"
          aria-label="Expand all {hiddenLines} hidden lines"
        >
          Expand all ({hiddenLines})
        </button>
      {:else}
        {#if direction === 'up' || direction === 'both'}
          <button
            type="button"
            onclick={onExpandUp}
            disabled={!onExpandUp}
            class="px-2 py-0.5 rounded border border-github-border bg-github-bg-secondary text-github-text-primary hover:bg-github-bg-primary cursor-pointer disabled:opacity-50"
            aria-label="Expand {DEFAULT_EXPAND_COUNT} hidden lines above"
          >
            ↑ Expand {DEFAULT_EXPAND_COUNT}
          </button>
        {/if}
        {#if direction === 'down' || direction === 'both'}
          <button
            type="button"
            onclick={onExpandDown}
            disabled={!onExpandDown}
            class="px-2 py-0.5 rounded border border-github-border bg-github-bg-secondary text-github-text-primary hover:bg-github-bg-primary cursor-pointer disabled:opacity-50"
            aria-label="Expand {DEFAULT_EXPAND_COUNT} hidden lines below"
          >
            ↓ Expand {DEFAULT_EXPAND_COUNT}
          </button>
        {/if}
        {#if onExpandAll}
          <button
            type="button"
            onclick={onExpandAll}
            class="px-2 py-0.5 rounded border border-github-border bg-github-bg-secondary text-github-text-secondary hover:text-github-text-primary cursor-pointer"
            aria-label="Expand all {hiddenLines} hidden lines"
          >
            All ({hiddenLines})
          </button>
        {/if}
      {/if}
    </div>
    <span class="font-mono text-[11px] text-github-text-muted">{lineLabel}</span>
  </div>
{/if}
