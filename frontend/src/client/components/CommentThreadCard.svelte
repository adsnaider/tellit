<script lang="ts">
  import { onDestroy } from 'svelte';

  import { type CommentThread } from '../../types/diff';
  import { formatCommentThreadPrompt } from '../../utils/commentFormatting';
  import { copyTextToClipboard } from '../utils/clipboard';

  import CommentBody from './CommentBody.svelte';
  import CommentForm from './CommentForm.svelte';

  interface Props {
    thread: CommentThread;
    domId?: string;
    showAuthorBadges?: boolean;
    syntaxTheme?: string;
    isHighlighted?: boolean;
    isUnanchored?: boolean;
    onReplyToThread: (threadId: string, body: string) => Promise<void>;
    onToggleResolve?: (threadId: string, currentResolved: boolean) => Promise<void> | void;
    onRemoveThread?: (threadId: string) => void;
    onGeneratePrompt?: (thread: CommentThread) => string;
  }

  let {
    thread,
    domId,
    showAuthorBadges = false,
    syntaxTheme = 'vsDark',
    isHighlighted = false,
    isUnanchored = false,
    onReplyToThread,
    onToggleResolve,
    onRemoveThread,
    onGeneratePrompt,
  }: Props = $props();

  let isExpanded = $state(true);
  let hasInitializedExpanded = $state(false);
  let lastResolvedProp = $state<boolean | undefined>(undefined);
  let isReplying = $state(false);
  let isCopied = $state(false);
  let isResolving = $state(false);
  let actionError = $state<string | null>(null);
  let copiedTimer: ReturnType<typeof setTimeout> | null = null;

  onDestroy(() => {
    if (copiedTimer) clearTimeout(copiedTimer);
  });

  $effect(() => {
    const currentResolved = Boolean(thread.resolved);
    if (!hasInitializedExpanded || lastResolvedProp !== currentResolved) {
      isExpanded = !currentResolved;
      lastResolvedProp = currentResolved;
      hasInitializedExpanded = true;
    }
  });

  const lineLabel = $derived.by(() => {
    if (Array.isArray(thread.line)) {
      return `L${thread.line[0]}-L${thread.line[1]}`;
    }
    return `L${thread.line}`;
  });

  const replyCount = $derived(Math.max(0, thread.messages.length - 1));

  async function handleCopyPrompt(event: MouseEvent) {
    event.stopPropagation();
    const prompt = onGeneratePrompt ? onGeneratePrompt(thread) : formatCommentThreadPrompt(thread);
    try {
      await copyTextToClipboard(prompt);
      if (copiedTimer) clearTimeout(copiedTimer);
      isCopied = true;
      copiedTimer = setTimeout(() => {
        isCopied = false;
        copiedTimer = null;
      }, 2000);
    } catch (err) {
      actionError = err instanceof Error ? err.message : 'Failed to copy prompt';
    }
  }

  async function handleToggleResolveClick(event: MouseEvent) {
    event.stopPropagation();
    if (!onToggleResolve || isResolving) return;
    isResolving = true;
    actionError = null;
    try {
      await onToggleResolve(thread.id, Boolean(thread.resolved));
    } catch (err) {
      actionError = err instanceof Error ? err.message : 'Failed to update thread state';
    } finally {
      isResolving = false;
    }
  }

  async function handleSubmitReply(body: string) {
    await onReplyToThread(thread.id, body);
    isReplying = false;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  id={domId}
  data-thread-id={thread.id}
  onclick={(event) => event.stopPropagation()}
  onkeydown={() => {}}
  class="rounded-md border text-left transition-shadow {thread.resolved
    ? 'border-github-border/70 bg-github-bg-secondary/70 opacity-85'
    : 'border-yellow-600/40 border-l-4 border-l-yellow-400 bg-github-bg-secondary'} {isHighlighted
    ? 'ring-2 ring-blue-500'
    : ''}"
>
  <div class="flex flex-wrap items-center justify-between gap-2 px-3 py-1.5 border-b border-github-border/60 bg-github-bg-tertiary/60 text-xs">
    <div class="flex flex-wrap items-center gap-1.5 min-w-0">
      <span class="font-mono font-semibold text-github-text-primary">
        {isUnanchored && thread.line === 1 ? 'File' : lineLabel}
      </span>
      {#if thread.side === 'old'}
        <span class="px-1 py-0 text-[10px] font-mono rounded bg-red-500/15 text-red-300 border border-red-500/30">
          old
        </span>
      {/if}
      {#if thread.isOutdated}
        <span class="px-1.5 py-0 text-[10px] rounded bg-yellow-500/15 text-yellow-300 border border-yellow-500/30">
          Outdated
        </span>
      {/if}
      {#if thread.resolved}
        <span class="px-1.5 py-0 text-[10px] rounded bg-green-500/15 text-green-400 border border-green-500/30">
          Resolved
        </span>
        {#if !isExpanded}
          <button
            type="button"
            onclick={() => {
              isExpanded = true;
            }}
            class="text-github-accent hover:underline cursor-pointer"
          >
            Show ({thread.messages.length} {thread.messages.length === 1 ? 'message' : 'messages'})
          </button>
        {/if}
      {:else if replyCount > 0}
        <span class="text-github-text-muted">
          {replyCount} {replyCount === 1 ? 'reply' : 'replies'}
        </span>
      {/if}
    </div>

    <div class="flex items-center gap-1.5 shrink-0">
      <button
        type="button"
        onclick={(event) => void handleCopyPrompt(event)}
        title="Copy agent prompt for this thread"
        class="px-2 py-0.5 rounded border border-github-border bg-github-bg-secondary text-github-text-secondary hover:text-github-text-primary cursor-pointer"
      >
        {isCopied ? 'Copied' : 'Copy prompt'}
      </button>

      {#if onToggleResolve}
        <button
          type="button"
          disabled={isResolving}
          onclick={(event) => void handleToggleResolveClick(event)}
          class="px-2 py-0.5 rounded border border-github-border bg-github-bg-secondary cursor-pointer disabled:opacity-50 {thread.resolved
            ? 'text-yellow-400 hover:bg-yellow-500/10'
            : 'text-green-400 hover:bg-green-500/10'}"
        >
          {thread.resolved ? 'Reopen' : 'Resolve'}
        </button>
      {/if}

      {#if onRemoveThread}
        <button
          type="button"
          onclick={(event) => {
            event.stopPropagation();
            onRemoveThread(thread.id);
          }}
          class="px-2 py-0.5 rounded border border-github-border bg-github-bg-secondary text-red-400 hover:bg-red-500/10 cursor-pointer"
          title="Delete thread"
        >
          Delete
        </button>
      {/if}

      {#if thread.resolved && isExpanded}
        <button
          type="button"
          onclick={() => {
            isExpanded = false;
          }}
          class="px-1.5 py-0.5 text-github-text-muted hover:text-github-text-primary cursor-pointer"
        >
          Hide
        </button>
      {/if}
    </div>
  </div>

  {#if actionError}
    <div
      role="alert"
      class="mx-3 mt-2 rounded border border-red-500/40 bg-red-500/10 px-2.5 py-1 text-xs text-red-300"
    >
      {actionError}
    </div>
  {/if}

  {#if isExpanded}
    <div class="divide-y divide-github-border/50">
      {#each thread.messages as message (message.id)}
        <div class="px-3 py-2.5">
          {#if showAuthorBadges && message.author}
            <div class="mb-1.5">
              <span class="inline-flex items-center rounded-full border border-github-border bg-github-bg-primary px-2 py-0.5 text-[11px] font-medium text-github-text-primary">
                {message.author}
              </span>
            </div>
          {/if}
          <CommentBody
            body={message.body}
            originalCode={thread.codeContent}
            filename={thread.file}
            {syntaxTheme}
          />
        </div>
      {/each}
    </div>

    <div class="px-3 py-2 border-t border-github-border/50 bg-github-bg-tertiary/30">
      {#if isReplying}
        <CommentForm
          embedded
          title="Reply to thread"
          placeholder="Write a reply... (Cmd/Ctrl+Enter to submit)"
          submitLabel="Reply"
          selectedCode={thread.codeContent}
          filename={thread.file}
          {syntaxTheme}
          onSubmit={handleSubmitReply}
          onCancel={() => {
            isReplying = false;
          }}
        />
      {:else}
        <button
          type="button"
          onclick={() => {
            isReplying = true;
          }}
          class="text-xs px-2.5 py-1 rounded border border-github-border bg-github-bg-secondary text-github-text-secondary hover:text-github-text-primary cursor-pointer"
        >
          Reply...
        </button>
      {/if}
    </div>
  {/if}
</div>
