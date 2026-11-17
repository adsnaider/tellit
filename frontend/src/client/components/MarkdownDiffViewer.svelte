<script lang="ts">
  import { fetchFileBlobText } from '../services/api';
  import { extractFrontmatter, getFrontmatterLines } from '../utils/frontmatter';
  import { computeFrontmatterDiff } from '../utils/frontmatterDiff';
  import {
    buildMarkdownPreviewBlocks,
    buildMarkdownRenderBlocks,
    isPlainPreviewBlock,
    type MarkdownPreviewBlockType,
    stripFrontmatterBlocks,
  } from '../utils/markdownParser';
  import type { DiffViewerBodyProps } from '../viewers/types';

  import CommentForm from './CommentForm.svelte';
  import CommentThreadCard from './CommentThreadCard.svelte';
  import FrontmatterTable from './FrontmatterTable.svelte';
  import MarkdownContent from './MarkdownContent.svelte';
  import TextDiffViewer from './TextDiffViewer.svelte';

  type PreviewMode = 'diff' | 'diff-preview' | 'full-preview';

  let props: DiffViewerBodyProps = $props();

  let mode = $state<PreviewMode>('diff');
  let baseContent = $state<string | null>(null);
  let targetContent = $state<string | null>(null);
  let isPreviewLoading = $state(false);
  let previewError = $state<string | null>(null);
  let partialFailureLabel = $state<string | null>(null);
  let loadedSourcesKey = $state<string | null>(null);

  function isFetchableRef(ref: string | undefined): ref is string {
    return Boolean(ref && ref !== 'stdin');
  }

  const baseSource = $derived(
    props.file.status === 'added' || !isFetchableRef(props.baseCommitish)
      ? null
      : { path: props.file.oldPath || props.file.path, ref: props.baseCommitish },
  );
  const targetSource = $derived(
    props.file.status === 'deleted' || !isFetchableRef(props.targetCommitish)
      ? null
      : { path: props.file.path, ref: props.targetCommitish },
  );
  const previewSourcesKey = $derived.by(() => {
    const baseKey = baseSource ? `${baseSource.ref}:${baseSource.path}` : '';
    const targetKey = targetSource ? `${targetSource.ref}:${targetSource.path}` : '';
    return baseKey || targetKey ? `${baseKey}|${targetKey}` : null;
  });

  $effect(() => {
    const currentKey = previewSourcesKey;
    const currentBase = baseSource;
    const currentTarget = targetSource;

    if (!currentKey || (!currentBase && !currentTarget)) {
      baseContent = null;
      targetContent = null;
      loadedSourcesKey = null;
      previewError = null;
      partialFailureLabel = null;
      isPreviewLoading = false;
      return;
    }

    if (props.isCollapsed) {
      return;
    }

    const controller = new AbortController();
    let cancelled = false;

    if (currentKey !== loadedSourcesKey) {
      baseContent = null;
      targetContent = null;
      loadedSourcesKey = null;
    }
    isPreviewLoading = true;
    previewError = null;

    void Promise.allSettled([
      currentBase ? fetchFileBlobText(currentBase.path, currentBase.ref, controller.signal) : Promise.resolve(null),
      currentTarget
        ? fetchFileBlobText(currentTarget.path, currentTarget.ref, controller.signal)
        : Promise.resolve(null),
    ]).then(([baseRes, targetRes]) => {
      if (cancelled || controller.signal.aborted) return;

      const nextBase = baseRes.status === 'fulfilled' ? baseRes.value : null;
      const nextTarget = targetRes.status === 'fulfilled' ? targetRes.value : null;

      const failures: string[] = [];
      if (currentBase && baseRes.status === 'rejected') {
        failures.push(baseRes.reason instanceof Error ? baseRes.reason.message : 'base fetch failed');
      }
      if (currentTarget && targetRes.status === 'rejected') {
        failures.push(
          targetRes.reason instanceof Error ? targetRes.reason.message : 'target fetch failed',
        );
      }

      baseContent = nextBase;
      targetContent = nextTarget;
      loadedSourcesKey = currentKey;

      const baseFailed = currentBase !== null && baseRes.status === 'rejected';
      const targetFailed = currentTarget !== null && targetRes.status === 'rejected';
      const allFailed =
        (!currentBase || baseFailed) && (!currentTarget || targetFailed) && failures.length > 0;

      previewError = allFailed ? failures.join('; ') : null;
      if (allFailed) {
        partialFailureLabel = null;
      } else if (baseFailed) {
        partialFailureLabel = 'Base content unavailable — showing partial preview.';
      } else if (targetFailed) {
        partialFailureLabel = 'Target content unavailable — showing partial preview.';
      } else {
        partialFailureLabel = null;
      }

      isPreviewLoading = false;
    });

    return () => {
      cancelled = true;
      controller.abort();
    };
  });

  const fullPreviewContent = $derived(
    props.file.status === 'deleted' ? baseContent : targetContent,
  );
  const hasFullPreview = $derived(
    previewSourcesKey !== null &&
      previewSourcesKey === loadedSourcesKey &&
      fullPreviewContent !== null,
  );

  $effect(() => {
    if (mode === 'full-preview' && !isPreviewLoading && !hasFullPreview) {
      mode = 'diff-preview';
    }
  });

  // Switch to the Code tab automatically when keyboard shortcut "c" targets a line in this file.
  $effect(() => {
    if (props.pendingLineComment?.filePath === props.file.path && mode !== 'diff') {
      mode = 'diff';
    }
  });

  const previewBlocks = $derived(buildMarkdownPreviewBlocks(props.mergedChunks));
  const diffStartsAtFileTop = $derived.by(() => {
    const firstChunk = props.mergedChunks[0];
    return firstChunk !== undefined && (firstChunk.oldStart <= 1 || firstChunk.newStart <= 1);
  });

  const frontmatterInfo = $derived.by(() => {
    const baseAvailable = baseContent !== null;
    const targetAvailable = targetContent !== null;
    const baseData = baseContent !== null ? extractFrontmatter(baseContent).data : null;
    const targetData = targetContent !== null ? extractFrontmatter(targetContent).data : null;

    const canDiff =
      props.file.status === 'added'
        ? targetAvailable
        : props.file.status === 'deleted'
          ? baseAvailable
          : baseAvailable && targetAvailable;

    if (canDiff) {
      const stripBaseLines =
        !diffStartsAtFileTop || props.file.status === 'added'
          ? []
          : getFrontmatterLines(baseContent ?? '');
      const stripTargetLines =
        !diffStartsAtFileTop || props.file.status === 'deleted'
          ? []
          : getFrontmatterLines(targetContent ?? '');
      const entries = computeFrontmatterDiff(
        props.file.status === 'added' ? null : baseData,
        props.file.status === 'deleted' ? null : targetData,
      );
      const hasChanges = entries.some((entry) => entry.status !== 'unchanged');
      return {
        mode: hasChanges ? ('diff' as const) : ('none' as const),
        entries,
        snapshotData: null as Record<string, unknown> | null,
        snapshotLabel: undefined as string | undefined,
        stripBaseLines,
        stripTargetLines,
      };
    }

    if (targetAvailable && targetData !== null) {
      return {
        mode: 'snapshot' as const,
        entries: [],
        snapshotData: targetData,
        snapshotLabel: 'Frontmatter (target only — base unavailable)',
        stripBaseLines: [] as string[],
        stripTargetLines: [] as string[],
      };
    }

    if (baseAvailable && baseData !== null) {
      return {
        mode: 'snapshot' as const,
        entries: [],
        snapshotData: baseData,
        snapshotLabel: 'Frontmatter (base only — target unavailable)',
        stripBaseLines: [] as string[],
        stripTargetLines: [] as string[],
      };
    }

    return {
      mode: 'none' as const,
      entries: [],
      snapshotData: null as Record<string, unknown> | null,
      snapshotLabel: undefined as string | undefined,
      stripBaseLines: [] as string[],
      stripTargetLines: [] as string[],
    };
  });

  const renderBlocks = $derived(
    buildMarkdownRenderBlocks(
      stripFrontmatterBlocks(
        previewBlocks,
        frontmatterInfo.stripBaseLines,
        frontmatterInfo.stripTargetLines,
      ),
    ),
  );

  const fullDocumentParts = $derived(
    fullPreviewContent !== null ? extractFrontmatter(fullPreviewContent) : null,
  );

  function getBlockBorderClass(type: MarkdownPreviewBlockType): string {
    if (type === 'add' || type === 'change') return 'border-l-4 border-diff-addition-border';
    if (type === 'delete') return 'border-l-4 border-diff-deletion-border';
    return 'border-l-4 border-transparent';
  }

  function getCodeLineClass(type: MarkdownPreviewBlockType): string {
    if (type === 'add' || type === 'change') {
      return 'border-l-4 border-diff-addition-border bg-diff-addition-bg';
    }
    if (type === 'delete') {
      return 'border-l-4 border-diff-deletion-border bg-diff-deletion-bg';
    }
    return 'border-l-4 border-transparent';
  }

  async function submitWholeFileComment(body: string) {
    await props.onAddComment(props.file.path, undefined, body, undefined, 'new');
    props.onCancelWholeFileComment?.();
  }
</script>

<div class="bg-github-bg-primary">
  <div
    class="flex items-center justify-between border-b border-github-border px-4 py-2"
    role="tablist"
    aria-label="Markdown view mode"
  >
    <div class="flex items-center gap-1.5">
      <button
        type="button"
        role="tab"
        aria-selected={mode === 'diff'}
        onclick={() => {
          mode = 'diff';
        }}
        title="Code Diff"
        class="px-2 py-1 text-xs font-medium rounded transition-colors cursor-pointer {mode === 'diff'
          ? 'bg-github-bg-tertiary text-github-text-primary'
          : 'text-github-text-secondary hover:text-github-text-primary'}"
      >
        Code
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={mode === 'diff-preview'}
        onclick={() => {
          mode = 'diff-preview';
        }}
        title="Diff Preview"
        class="px-2 py-1 text-xs font-medium rounded transition-colors cursor-pointer {mode === 'diff-preview'
          ? 'bg-github-bg-tertiary text-github-text-primary'
          : 'text-github-text-secondary hover:text-github-text-primary'}"
      >
        Diff Preview
      </button>
      {#if hasFullPreview}
        <button
          type="button"
          role="tab"
          aria-selected={mode === 'full-preview'}
          onclick={() => {
            mode = 'full-preview';
          }}
          title="Full Preview"
          class="px-2 py-1 text-xs font-medium rounded transition-colors cursor-pointer {mode === 'full-preview'
            ? 'bg-github-bg-tertiary text-github-text-primary'
            : 'text-github-text-secondary hover:text-github-text-primary'}"
        >
          Full Preview
        </button>
      {/if}
    </div>
  </div>

  {#if mode === 'diff'}
    <TextDiffViewer
      file={props.file}
      mergedChunks={props.mergedChunks}
      diffMode={props.diffMode}
      threads={props.threads}
      showAuthorBadges={props.showAuthorBadges}
      syntaxTheme={props.syntaxTheme}
      baseCommitish={props.baseCommitish}
      targetCommitish={props.targetCommitish}
      openInEditorAvailable={props.openInEditorAvailable}
      isExpandLoading={props.isExpandLoading}
      isCollapsed={props.isCollapsed}
      activeWholeFileComment={props.activeWholeFileComment}
      highlightedThreadId={props.highlightedThreadId}
      fileIndex={props.fileIndex}
      cursor={props.cursor}
      pendingLineComment={props.pendingLineComment}
      onExpandLines={props.onExpandLines}
      onExpandAllBetween={props.onExpandAllBetween}
      onPrefetchLineCount={props.onPrefetchLineCount}
      onAddComment={props.onAddComment}
      onReplyToThread={props.onReplyToThread}
      onToggleResolve={props.onToggleResolve}
      onRemoveThread={props.onRemoveThread}
      onOpenInEditor={props.onOpenInEditor}
      onCancelWholeFileComment={props.onCancelWholeFileComment}
      onConsumePendingComment={props.onConsumePendingComment}
      onCursorSelectLine={props.onCursorSelectLine}
    />
  {:else}
    {#if props.activeWholeFileComment}
      <div class="border-b border-github-border bg-github-bg-secondary/50 p-1">
        <CommentForm
          title="Comment on {props.file.path}"
          placeholder="Write a file-level comment... (Cmd/Ctrl+Enter to submit)"
          submitLabel="Comment"
          filename={props.file.path}
          syntaxTheme={props.syntaxTheme}
          onSubmit={submitWholeFileComment}
          onCancel={() => props.onCancelWholeFileComment?.()}
        />
      </div>
    {/if}

    {#if props.threads.length > 0}
      <div class="p-3 space-y-2 border-b border-github-border bg-github-bg-secondary/40">
        {#each props.threads as thread (thread.id)}
          <CommentThreadCard
            {thread}
            domId="comment-thread-{thread.id}"
            isUnanchored={true}
            isHighlighted={props.highlightedThreadId === thread.id}
            showAuthorBadges={props.showAuthorBadges}
            syntaxTheme={props.syntaxTheme}
            onReplyToThread={props.onReplyToThread}
            onToggleResolve={props.onToggleResolve}
            onRemoveThread={props.onRemoveThread}
          />
        {/each}
      </div>
    {/if}

    {#if mode === 'diff-preview'}
      <div class="p-4 space-y-4">
        {#if partialFailureLabel}
          <div class="text-sm text-github-text-muted">{partialFailureLabel}</div>
        {/if}

        {#if frontmatterInfo.mode === 'diff'}
          <FrontmatterTable mode="diff" entries={frontmatterInfo.entries} label="Frontmatter" />
        {:else if frontmatterInfo.mode === 'snapshot'}
          <FrontmatterTable
            mode="snapshot"
            data={frontmatterInfo.snapshotData}
            label={frontmatterInfo.snapshotLabel}
          />
        {/if}

        {#each renderBlocks as block, blockIdx (blockIdx)}
          {#if block.kind === 'fenced-code'}
            <div class="px-4">
              <div class="markdown-preview-code overflow-x-auto rounded border border-github-border bg-github-bg-secondary text-sm font-mono text-github-text-primary">
                {#each block.lines as line, lineIdx (lineIdx)}
                  <div class="{getCodeLineClass(line.type)} whitespace-pre px-3 py-1">
                    {line.content || ' '}
                  </div>
                {/each}
              </div>
            </div>
          {:else}
            {@const trimmed = block.lines.join('\n').trim()}
            {#if trimmed}
              <div class="px-4 {getBlockBorderClass(block.type)}">
                {#if isPlainPreviewBlock(block.lines)}
                  <pre class="whitespace-pre-wrap text-sm text-github-text-primary font-mono">{trimmed}</pre>
                {:else}
                  <MarkdownContent content={trimmed} syntaxTheme={props.syntaxTheme} />
                {/if}
              </div>
            {/if}
          {/if}
        {/each}
      </div>
    {:else}
      <div class="p-4 space-y-4">
        {#if isPreviewLoading}
          <div class="text-sm text-github-text-muted">Loading preview...</div>
        {/if}
        {#if previewError}
          <div class="text-sm text-github-danger">{previewError}</div>
        {/if}
        {#if partialFailureLabel}
          <div class="text-sm text-github-text-muted">{partialFailureLabel}</div>
        {/if}
        {#if !isPreviewLoading && !previewError && fullDocumentParts}
          <FrontmatterTable mode="snapshot" data={fullDocumentParts.data} />
          <MarkdownContent content={fullDocumentParts.content} syntaxTheme={props.syntaxTheme} />
        {:else if !isPreviewLoading && !previewError && fullPreviewContent === null}
          <div class="text-sm text-github-text-muted">Preview unavailable.</div>
        {/if}
      </div>
    {/if}
  {/if}
</div>
