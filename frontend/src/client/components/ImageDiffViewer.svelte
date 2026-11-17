<script lang="ts">
  import type { DiffViewerBodyProps } from '../viewers/types';

  import CommentForm from './CommentForm.svelte';
  import CommentThreadCard from './CommentThreadCard.svelte';

  interface ImageInfo {
    width?: number;
    height?: number;
    size?: number;
  }

  type CompareMode = 'side-by-side' | 'swipe' | 'onion-skin';

  interface StaticBlobWindow extends Window {
    __DIFIT_STATIC_BLOB_URLS__?: Record<string, string>;
  }

  let props: DiffViewerBodyProps = $props();

  let compareMode = $state<CompareMode>('side-by-side');
  let swipePercent = $state(50);
  let onionOpacity = $state(50);
  let oldImageInfo = $state<ImageInfo>({});
  let newImageInfo = $state<ImageInfo>({});
  let oldLoadFailed = $state(false);
  let newLoadFailed = $state(false);

  const baseRef = $derived(props.baseCommitish || 'HEAD~1');
  const targetRef = $derived(props.targetCommitish || 'HEAD');
  const oldPath = $derived(props.file.oldPath || props.file.path);
  const isDeleted = $derived(props.file.status === 'deleted');
  const isAdded = $derived(props.file.status === 'added');
  const isModified = $derived(
    props.file.status === 'modified' || props.file.status === 'renamed',
  );

  function imageBlobUrl(path: string, ref: string): string {
    if (typeof window !== 'undefined') {
      const staticMap = (window as StaticBlobWindow).__DIFIT_STATIC_BLOB_URLS__;
      const key = `${ref.slice(0, 7)}:${path}`;
      if (staticMap?.[key]) return staticMap[key];
    }
    return `/api/blob/${encodeURIComponent(path)}?ref=${encodeURIComponent(ref)}`;
  }

  const oldSrc = $derived(imageBlobUrl(oldPath, baseRef));
  const newSrc = $derived(imageBlobUrl(props.file.path, targetRef));

  let activeMetaAbort: AbortController | null = null;

  $effect(() => {
    void oldSrc;
    void newSrc;
    oldImageInfo = {};
    newImageInfo = {};
    oldLoadFailed = false;
    newLoadFailed = false;
    activeMetaAbort?.abort();
    const controller = new AbortController();
    activeMetaAbort = controller;
    return () => {
      controller.abort();
    };
  });

  async function handleImageLoad(img: HTMLImageElement, side: 'old' | 'new') {
    const width = img.naturalWidth;
    const height = img.naturalHeight;
    const controller = activeMetaAbort;
    try {
      const res = await fetch(img.src, controller ? { signal: controller.signal } : undefined);
      const blob = await res.blob();
      if (controller?.signal.aborted) return;
      const info: ImageInfo = { width, height, size: blob.size };
      if (side === 'old') oldImageInfo = info;
      else newImageInfo = info;
    } catch {
      if (controller?.signal.aborted) return;
      const info: ImageInfo = { width, height };
      if (side === 'old') oldImageInfo = info;
      else newImageInfo = info;
    }
  }

  function formatFileSize(bytes?: number): string {
    if (!bytes) return '';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function formatMetaLine(info: ImageInfo): string {
    const parts: string[] = [];
    if (info.width && info.height) {
      parts.push(`W: ${info.width}px | H: ${info.height}px`);
    }
    const formattedSize = formatFileSize(info.size);
    if (formattedSize) {
      parts.push(formattedSize);
    }
    return parts.join(' | ');
  }

  const checkerboardStyle =
    'background-image: linear-gradient(45deg, #ccc 25%, transparent 25%, transparent 75%, #ccc 75%), linear-gradient(45deg, #ccc 25%, transparent 25%, transparent 75%, #ccc 75%); background-size: 20px 20px; background-position: 0 0, 10px 10px; background-color: white;';

  async function submitWholeFileComment(body: string) {
    await props.onAddComment(props.file.path, undefined, body, undefined, 'new');
    props.onCancelWholeFileComment?.();
  }
</script>

{#snippet imageCard(
  heading: string,
  src: string,
  alt: string,
  side: 'old' | 'new',
  failed: boolean,
  info: ImageInfo,
  inlineBlock: boolean,
)}
  <div class="border border-github-border rounded-md p-4 bg-github-bg-secondary {inlineBlock ? 'inline-block' : ''}">
    <div class="text-github-text-muted text-sm mb-2">{heading}</div>
    {#if !failed}
      <img
        {src}
        {alt}
        class="max-w-full max-h-96 border border-github-border rounded mx-auto"
        style={checkerboardStyle}
        onload={(e) => void handleImageLoad(e.currentTarget as HTMLImageElement, side)}
        onerror={() => {
          if (side === 'old') oldLoadFailed = true;
          else newLoadFailed = true;
        }}
      />
    {:else}
      <div class="text-github-text-muted text-sm mt-2">Image could not be loaded</div>
    {/if}
    {#if formatMetaLine(info)}
      <div class="text-github-text-muted text-sm mt-2">{formatMetaLine(info)}</div>
    {/if}
  </div>
{/snippet}

<div class="bg-github-bg-primary">
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

  <div class="p-4">
    {#if isDeleted}
      <div class="text-center">
        <div class="mb-2">
          <span class="text-github-danger font-medium">Deleted Image</span>
        </div>
        {@render imageCard(
          'Previous version:',
          oldSrc,
          `Previous version of ${oldPath}`,
          'old',
          oldLoadFailed,
          oldImageInfo,
          true,
        )}
      </div>
    {:else if isAdded}
      <div class="text-center">
        <div class="mb-2">
          <span class="text-github-accent font-medium">Added Image</span>
        </div>
        {@render imageCard(
          'New file:',
          newSrc,
          `New image ${props.file.path}`,
          'new',
          newLoadFailed,
          newImageInfo,
          true,
        )}
      </div>
    {:else if isModified}
      <div class="flex flex-wrap items-center justify-between gap-3 mb-4">
        <span class="text-github-text-primary font-medium text-sm">Modified Image</span>
        <div
          class="inline-flex rounded border border-github-border bg-github-bg-secondary p-0.5 text-xs"
          role="group"
          aria-label="Image comparison mode"
        >
          {#each [
            { id: 'side-by-side', label: 'Side-by-side' },
            { id: 'swipe', label: 'Swipe' },
            { id: 'onion-skin', label: 'Onion-skin' },
          ] as modeOpt (modeOpt.id)}
            <button
              type="button"
              onclick={() => {
                compareMode = modeOpt.id as CompareMode;
              }}
              aria-pressed={compareMode === modeOpt.id}
              class="px-2.5 py-1 rounded transition-colors cursor-pointer {compareMode === modeOpt.id
                ? 'bg-github-bg-tertiary text-github-text-primary font-medium'
                : 'text-github-text-secondary hover:text-github-text-primary'}"
            >
              {modeOpt.label}
            </button>
          {/each}
        </div>
      </div>

      {#if compareMode === 'side-by-side'}
        <div class={props.diffMode === 'split' ? 'grid grid-cols-2 gap-4' : 'space-y-6'}>
          <div class="text-center">
            {@render imageCard(
              'Previous version:',
              oldSrc,
              `Previous version of ${oldPath}`,
              'old',
              oldLoadFailed,
              oldImageInfo,
              props.diffMode !== 'split',
            )}
          </div>
          <div class="text-center">
            {@render imageCard(
              'Current version:',
              newSrc,
              `Current version of ${props.file.path}`,
              'new',
              newLoadFailed,
              newImageInfo,
              props.diffMode !== 'split',
            )}
          </div>
        </div>
      {:else if compareMode === 'swipe'}
        <div class="space-y-3 text-center">
          {#if oldLoadFailed || newLoadFailed}
            <div class="text-github-text-muted text-sm py-6">Image could not be loaded</div>
          {:else}
            <div class="relative inline-block border border-github-border rounded-md bg-github-bg-secondary overflow-hidden">
              <img
                src={oldSrc}
                alt="Previous version of {oldPath}"
                class="block max-w-full max-h-96 select-none"
                style={checkerboardStyle}
                onload={(e) => void handleImageLoad(e.currentTarget as HTMLImageElement, 'old')}
                onerror={() => {
                  oldLoadFailed = true;
                }}
              />
              <img
                src={newSrc}
                alt="Current version of {props.file.path}"
                class="block max-w-full max-h-96 select-none absolute inset-0 w-full h-full object-contain"
                style="clip-path: inset(0 {100 - swipePercent}% 0 0);"
                onload={(e) => void handleImageLoad(e.currentTarget as HTMLImageElement, 'new')}
                onerror={() => {
                  newLoadFailed = true;
                }}
              />
              <div
                aria-hidden="true"
                class="absolute inset-y-0 w-0.5 bg-blue-400 pointer-events-none"
                style="left: {swipePercent}%;"
              ></div>
            </div>
          {/if}
          <div class="max-w-md mx-auto flex items-center gap-3 text-xs text-github-text-secondary">
            <span>After ({swipePercent}%)</span>
            <input
              type="range"
              min="0"
              max="100"
              bind:value={swipePercent}
              aria-label="Swipe comparison slider"
              class="flex-1 accent-github-accent"
            />
            <span>Before</span>
          </div>
        </div>
      {:else}
        <div class="space-y-3 text-center">
          {#if oldLoadFailed || newLoadFailed}
            <div class="text-github-text-muted text-sm py-6">Image could not be loaded</div>
          {:else}
            <div class="relative inline-block border border-github-border rounded-md bg-github-bg-secondary overflow-hidden">
              <img
                src={oldSrc}
                alt="Previous version of {oldPath}"
                class="block max-w-full max-h-96 select-none"
                style={checkerboardStyle}
                onload={(e) => void handleImageLoad(e.currentTarget as HTMLImageElement, 'old')}
                onerror={() => {
                  oldLoadFailed = true;
                }}
              />
              <img
                src={newSrc}
                alt="Current version of {props.file.path}"
                class="block max-w-full max-h-96 select-none absolute inset-0 w-full h-full object-contain"
                style="opacity: {onionOpacity / 100};"
                onload={(e) => void handleImageLoad(e.currentTarget as HTMLImageElement, 'new')}
                onerror={() => {
                  newLoadFailed = true;
                }}
              />
            </div>
          {/if}
          <div class="max-w-md mx-auto flex items-center gap-3 text-xs text-github-text-secondary">
            <span>Previous</span>
            <input
              type="range"
              min="0"
              max="100"
              bind:value={onionOpacity}
              aria-label="Onion-skin opacity slider"
              class="flex-1 accent-github-accent"
            />
            <span>Current ({onionOpacity}%)</span>
          </div>
        </div>
      {/if}
    {/if}
  </div>
</div>
