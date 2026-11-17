<script lang="ts">
  import { onDestroy } from 'svelte';

  import { type CommentThread } from '../../types/diff';
  import {
    type CommentPromptDiffContext,
    formatAllCommentThreadsPrompt,
  } from '../../utils/commentFormatting';
  import { copyTextToClipboard } from '../utils/clipboard';

  import CommentThreadCard from './CommentThreadCard.svelte';

  interface Props {
    isOpen: boolean;
    threads: CommentThread[];
    showAuthorBadges?: boolean;
    showResolvedInline: boolean;
    syntaxTheme?: string;
    promptContext?: CommentPromptDiffContext;
    onClose: () => void;
    onToggleShowResolvedInline: (next: boolean) => void;
    onNavigateToThread: (thread: CommentThread) => void;
    onReplyToThread: (threadId: string, body: string) => Promise<void>;
    onToggleResolve?: (threadId: string, currentResolved: boolean) => Promise<void> | void;
    onRemoveThread?: (threadId: string) => void;
  }

  let {
    isOpen,
    threads,
    showAuthorBadges = false,
    showResolvedInline,
    syntaxTheme = 'vsDark',
    promptContext,
    onClose,
    onToggleShowResolvedInline,
    onNavigateToThread,
    onReplyToThread,
    onToggleResolve,
    onRemoveThread,
  }: Props = $props();

  let filter = $state<'unresolved' | 'all' | 'resolved'>('unresolved');
  let isCopiedAll = $state(false);
  let copyError = $state<string | null>(null);
  let copiedAllTimer: ReturnType<typeof setTimeout> | null = null;

  onDestroy(() => {
    if (copiedAllTimer) clearTimeout(copiedAllTimer);
  });

  const unresolvedCount = $derived(threads.filter((t) => !t.resolved).length);
  const resolvedCount = $derived(threads.length - unresolvedCount);

  const sortedThreads = $derived.by(() => {
    return [...threads].sort((a, b) => {
      const aResolved = Boolean(a.resolved);
      const bResolved = Boolean(b.resolved);
      if (aResolved !== bResolved) return aResolved ? 1 : -1;
      const fileCompare = a.file.localeCompare(b.file);
      if (fileCompare !== 0) return fileCompare;
      const aLine = Array.isArray(a.line) ? a.line[0] : a.line;
      const bLine = Array.isArray(b.line) ? b.line[0] : b.line;
      if (aLine !== bLine) return aLine - bLine;
      return a.createdAt.localeCompare(b.createdAt);
    });
  });

  const visibleThreads = $derived.by(() => {
    if (filter === 'unresolved') return sortedThreads.filter((t) => !t.resolved);
    if (filter === 'resolved') return sortedThreads.filter((t) => Boolean(t.resolved));
    return sortedThreads;
  });

  function formatLocationLabel(thread: CommentThread): string {
    const linePart = Array.isArray(thread.line)
      ? `L${thread.line[0]}-L${thread.line[1]}`
      : `L${thread.line}`;
    return `${thread.file}:${linePart}${thread.side === 'old' ? ' (old)' : ''}`;
  }

  async function handleCopyAllPrompts() {
    const targetThreads =
      filter === 'unresolved' && unresolvedCount > 0 ? visibleThreads : sortedThreads;
    const prompt = formatAllCommentThreadsPrompt(targetThreads, promptContext);
    if (!prompt) return;
    try {
      await copyTextToClipboard(prompt);
      copyError = null;
      if (copiedAllTimer) clearTimeout(copiedAllTimer);
      isCopiedAll = true;
      copiedAllTimer = setTimeout(() => {
        isCopiedAll = false;
        copiedAllTimer = null;
      }, 2000);
    } catch (err) {
      copyError = err instanceof Error ? err.message : 'Failed to copy prompts';
    }
  }
</script>

{#if isOpen}
  <aside
    id="comments-drawer"
    aria-label="Review comments drawer"
    class="w-96 max-w-[92vw] shrink-0 bg-github-bg-secondary border-l border-github-border flex flex-col min-h-0"
  >
    <div class="px-3 py-2.5 border-b border-github-border flex items-center justify-between gap-2">
      <div class="flex items-center gap-2 min-w-0">
        <h2 class="text-xs font-semibold text-github-text-primary">Comments ({threads.length})</h2>
        {#if unresolvedCount > 0}
          <span class="px-1.5 py-0.5 rounded-full text-[10px] font-semibold bg-yellow-500/15 text-yellow-400 border border-yellow-500/30">
            {unresolvedCount} open
          </span>
        {:else if threads.length > 0}
          <span class="px-1.5 py-0.5 rounded-full text-[10px] font-semibold bg-green-500/15 text-green-400 border border-green-500/30">
            All resolved
          </span>
        {/if}
      </div>

      <div class="flex items-center gap-1.5 shrink-0">
        {#if visibleThreads.length > 0 || sortedThreads.length > 0}
          <button
            type="button"
            onclick={() => void handleCopyAllPrompts()}
            class="px-2 py-0.5 text-[11px] rounded border border-github-border bg-github-bg-tertiary text-github-text-secondary hover:text-github-text-primary cursor-pointer"
            title="Copy formatted agent prompts"
          >
            {isCopiedAll ? 'Copied!' : filter === 'unresolved' && unresolvedCount > 0 ? 'Copy open' : 'Copy all'}
          </button>
        {/if}
        <button
          type="button"
          onclick={onClose}
          aria-label="Close comments drawer"
          class="px-1.5 py-0.5 text-xs text-github-text-muted hover:text-github-text-primary cursor-pointer"
        >
          ✕
        </button>
      </div>
    </div>

    <div class="px-3 py-2 border-b border-github-border/70 flex items-center justify-between gap-2 text-xs">
      <div class="inline-flex rounded border border-github-border overflow-hidden">
        <button
          type="button"
          onclick={() => {
            filter = 'unresolved';
          }}
          class="px-2 py-0.5 cursor-pointer {filter === 'unresolved'
            ? 'bg-github-bg-tertiary text-github-text-primary font-medium'
            : 'bg-github-bg-primary text-github-text-secondary'}"
        >
          Open ({unresolvedCount})
        </button>
        <button
          type="button"
          onclick={() => {
            filter = 'all';
          }}
          class="px-2 py-0.5 border-l border-github-border cursor-pointer {filter === 'all'
            ? 'bg-github-bg-tertiary text-github-text-primary font-medium'
            : 'bg-github-bg-primary text-github-text-secondary'}"
        >
          All ({threads.length})
        </button>
        <button
          type="button"
          onclick={() => {
            filter = 'resolved';
          }}
          class="px-2 py-0.5 border-l border-github-border cursor-pointer {filter === 'resolved'
            ? 'bg-github-bg-tertiary text-github-text-primary font-medium'
            : 'bg-github-bg-primary text-github-text-secondary'}"
        >
          Resolved ({resolvedCount})
        </button>
      </div>

      <label class="inline-flex items-center gap-1 text-[11px] text-github-text-secondary cursor-pointer select-none">
        <input
          type="checkbox"
          checked={showResolvedInline}
          onchange={(event) => onToggleShowResolvedInline(event.currentTarget.checked)}
          class="rounded border-github-border cursor-pointer"
        />
        <span>Inline resolved</span>
      </label>
    </div>

    {#if copyError}
      <div role="alert" class="mx-3 mt-2 px-2 py-1 text-xs rounded border border-red-500/40 bg-red-500/10 text-red-300">
        {copyError}
      </div>
    {/if}

    <div class="flex-1 overflow-y-auto p-3 space-y-3">
      {#if visibleThreads.length === 0}
        <p class="text-xs text-github-text-muted py-4 text-center">
          {threads.length === 0
            ? 'No review comments on this revision yet. Click or drag across line numbers to add one.'
            : filter === 'unresolved'
              ? 'All review threads are resolved.'
              : 'No matching threads.'}
        </p>
      {:else}
        {#each visibleThreads as thread (thread.id)}
          <div class="space-y-1">
            <div class="flex items-center justify-between gap-2 text-[11px]">
              <button
                type="button"
                onclick={() => onNavigateToThread(thread)}
                class="font-mono text-left text-github-accent hover:underline truncate cursor-pointer"
                title="Jump to {formatLocationLabel(thread)}"
              >
                {formatLocationLabel(thread)}
              </button>
              <button
                type="button"
                onclick={() => onNavigateToThread(thread)}
                class="shrink-0 px-1.5 py-0.5 rounded border border-github-border bg-github-bg-tertiary text-github-text-secondary hover:text-github-text-primary cursor-pointer"
              >
                Jump
              </button>
            </div>
            <CommentThreadCard
              {thread}
              {showAuthorBadges}
              {syntaxTheme}
              {onReplyToThread}
              {onToggleResolve}
              {onRemoveThread}
            />
          </div>
        {/each}
      {/if}
    </div>
  </aside>
{/if}
