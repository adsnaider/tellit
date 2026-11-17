<script lang="ts">
  import { createSuggestionTemplate, hasSuggestionBlock } from '../../utils/suggestionUtils';

  import CommentBody from './CommentBody.svelte';

  interface Props {
    title?: string;
    placeholder?: string;
    submitLabel?: string;
    initialValue?: string;
    selectedCode?: string;
    filename?: string;
    syntaxTheme?: string;
    embedded?: boolean;
    onSubmit: (body: string) => Promise<void> | void;
    onCancel: () => void;
  }

  let {
    title = 'Add a comment',
    placeholder = 'Leave a comment... (Cmd/Ctrl+Enter to submit)',
    submitLabel = 'Comment',
    initialValue = '',
    selectedCode,
    filename,
    syntaxTheme = 'vsDark',
    embedded = false,
    onSubmit,
    onCancel,
  }: Props = $props();

  let body = $state('');
  let initializedFromProp = $state(false);
  let mode = $state<'edit' | 'preview'>('edit');
  let isSubmitting = $state(false);
  let submitError = $state<string | null>(null);
  let textareaEl = $state<HTMLTextAreaElement | null>(null);

  $effect(() => {
    if (!initializedFromProp) {
      body = initialValue;
      initializedFromProp = true;
    }
  });

  const hasSuggestion = $derived(hasSuggestionBlock(body));
  const effectiveMode = $derived(hasSuggestion ? mode : 'edit');
  const rowCount = $derived(Math.max(3, body.split('\n').length));

  function insertSuggestion() {
    if (selectedCode === undefined) return;
    const snippet = createSuggestionTemplate(selectedCode);
    body = body.trim() ? `${body.trimEnd()}\n\n${snippet}` : snippet;
    submitError = null;
    textareaEl?.focus();
  }

  async function submitForm() {
    const trimmed = body.trim();
    if (!trimmed || isSubmitting) return;
    isSubmitting = true;
    submitError = null;
    try {
      await onSubmit(trimmed);
      body = '';
      mode = 'edit';
    } catch (err) {
      submitError = err instanceof Error && err.message.trim() ? err.message : 'Failed to submit comment';
    } finally {
      isSubmitting = false;
    }
  }

  function handleFormSubmit(event: SubmitEvent) {
    event.preventDefault();
    void submitForm();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      void submitForm();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      onCancel();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<form
  onsubmit={handleFormSubmit}
  onclick={(event) => event.stopPropagation()}
  onkeydown={() => {}}
  data-empty={!body.trim()}
  class={embedded
    ? 'bg-transparent'
    : 'm-2 mx-3 rounded-md border border-yellow-600/50 border-l-4 border-l-yellow-400 bg-github-bg-tertiary p-3'}
>
  <div class="flex items-center justify-between gap-2 mb-2">
    <span class="text-xs font-semibold" style="color: var(--color-yellow-path-text)">
      {title}
    </span>

    <div class="flex items-center gap-2">
      {#if hasSuggestion}
        <div class="flex items-center border border-github-border rounded-md overflow-hidden">
          <button
            type="button"
            onclick={() => {
              mode = 'edit';
            }}
            class="text-xs px-2.5 py-1 transition-colors cursor-pointer {effectiveMode === 'edit'
              ? 'bg-github-bg-tertiary text-github-text-primary'
              : 'bg-github-bg-secondary text-github-text-secondary'}"
          >
            Edit
          </button>
          <button
            type="button"
            onclick={() => {
              mode = 'preview';
            }}
            class="text-xs px-2.5 py-1 border-l border-github-border transition-colors cursor-pointer {effectiveMode === 'preview'
              ? 'bg-github-bg-tertiary text-github-text-primary'
              : 'bg-github-bg-secondary text-github-text-secondary'}"
          >
            Preview
          </button>
        </div>
      {:else if selectedCode !== undefined}
        <button
          type="button"
          onclick={insertSuggestion}
          disabled={isSubmitting}
          class="text-xs px-2 py-1 rounded border border-github-border bg-github-bg-secondary text-github-text-secondary hover:text-github-text-primary cursor-pointer disabled:opacity-50"
          title="Insert a code suggestion block"
        >
          + Suggestion
        </button>
      {/if}
    </div>
  </div>

  {#if hasSuggestion && effectiveMode === 'preview'}
    <div class="min-h-[60px] mb-2 bg-github-bg-secondary border border-github-border rounded px-3 py-2">
      <CommentBody {body} originalCode={selectedCode} {filename} {syntaxTheme} />
    </div>
  {:else}
    <textarea
      bind:this={textareaEl}
      bind:value={body}
      oninput={() => {
        if (submitError) submitError = null;
      }}
      onkeydown={handleKeydown}
      {placeholder}
      rows={rowCount}
      disabled={isSubmitting}
      aria-label={title}
      class="w-full min-h-[64px] mb-2 resize-y bg-github-bg-secondary border border-github-border rounded px-3 py-2 text-github-text-primary text-sm leading-6 focus:outline-none focus:border-blue-500 disabled:opacity-50"
    ></textarea>
  {/if}

  {#if submitError}
    <div
      role="alert"
      class="mb-2 rounded border border-red-500/40 bg-red-500/10 px-2.5 py-1.5 text-xs text-red-300 break-words"
    >
      {submitError}
    </div>
  {/if}

  <div class="flex items-center justify-end gap-2">
    <button
      type="button"
      data-comment-cancel="true"
      onclick={onCancel}
      disabled={isSubmitting}
      class="text-xs px-3 py-1.5 bg-github-bg-secondary text-github-text-primary border border-github-border rounded hover:opacity-85 cursor-pointer disabled:opacity-50"
    >
      Cancel
    </button>
    <button
      type="submit"
      disabled={!body.trim() || isSubmitting}
      class="text-xs px-3 py-1.5 rounded font-medium cursor-pointer disabled:opacity-50"
      style="background-color: var(--color-yellow-btn-bg); color: var(--color-yellow-btn-text); border: 1px solid var(--color-yellow-btn-border);"
    >
      {isSubmitting ? 'Submitting...' : submitLabel}
    </button>
  </div>
</form>
