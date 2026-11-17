<script lang="ts">
  import type {
    CommentThread,
    DiffSide,
    ExpandedLine,
    LineNumber,
  } from '../../types/diff';
  import type { DiffViewerBodyProps } from '../viewers/types';
  import { isExpandableFile } from '../utils/expandedLines';
  import { getPrismLanguageFromFilename, isWholeFileHighlightExtension } from '../utils/languageDetection';
  import { loadPrismLanguage } from '../utils/languageLoader';
  import { getElementId } from '../utils/navigation/domHelpers';
  import { type HighlightToken, isPrismLanguageLoaded, tokenizeCode } from '../utils/prism';
  import {
    computeUnifiedWordDiffMap,
    convertChunkToSideBySide,
    isDeletionLine,
    isExpandedDiffLine,
  } from '../utils/sideBySide';
  import {
    createWordHighlightHandlers,
    getWordHighlightContext,
    setWordHighlightContext,
  } from '../utils/wordHighlight';
  import { fetchFileBlobText } from '../services/api';

  import CommentForm from './CommentForm.svelte';
  import CommentThreadCard from './CommentThreadCard.svelte';
  import ExpandBar from './ExpandBar.svelte';
  import HighlightedCode from './HighlightedCode.svelte';

  const MAX_WHOLE_FILE_LINES = 2000;

  let {
    file,
    mergedChunks,
    diffMode,
    threads,
    showAuthorBadges = false,
    syntaxTheme = 'vsDark',
    baseCommitish,
    targetCommitish,
    openInEditorAvailable = false,
    isExpandLoading = false,
    isCollapsed = false,
    activeWholeFileComment = false,
    highlightedThreadId = null,
    fileIndex,
    cursor = null,
    pendingLineComment = null,
    onExpandLines,
    onExpandAllBetween,
    onPrefetchLineCount,
    onAddComment,
    onReplyToThread,
    onToggleResolve,
    onRemoveThread,
    onOpenInEditor,
    onCancelWholeFileComment,
    onConsumePendingComment,
  }: DiffViewerBodyProps = $props();

  interface DragSelection {
    chunkIndex: number;
    side: DiffSide;
    startLine: number;
    endLine: number;
  }

  interface CommentingSelection {
    side: DiffSide;
    line: LineNumber;
  }

  let rootEl = $state<HTMLDivElement | null>(null);
  let dragState = $state<DragSelection | null>(null);
  let commentingLine = $state<CommentingSelection | null>(null);
  let selectionAnchor = $state<{ side: DiffSide; lineNumber: number } | null>(null);
  let isVisible = $state(false);
  let wholeFileOldTokens = $state<HighlightToken[][] | null>(null);
  let wholeFileNewTokens = $state<HighlightToken[][] | null>(null);
  let localHoveredWord = $state<string | null>(null);
  let localPinnedWord = $state<string | null>(null);

  if (!getWordHighlightContext()) {
    const fallbackHandlers = createWordHighlightHandlers({
      getEffectiveWord: () => localPinnedWord ?? localHoveredWord,
      getPinnedWord: () => localPinnedWord,
      setHoveredWord: (word) => {
        localHoveredWord = word;
      },
      setPinnedWord: (word) => {
        localPinnedWord = word;
      },
    });
    setWordHighlightContext(fallbackHandlers);
    $effect(() => () => fallbackHandlers.dispose());
  }

  const isStdinDiff = $derived(baseCommitish === 'stdin' || targetCommitish === 'stdin');
  const canExpand = $derived(isExpandableFile(file) && !isStdinDiff);
  const canOpenEditor = $derived(
    openInEditorAvailable && file.path !== 'Commit Message' && Boolean(onOpenInEditor),
  );
  const detectedLanguage = $derived(getPrismLanguageFromFilename(file.path));
  const useWholeFileTokens = $derived(
    isWholeFileHighlightExtension(file.path) && !isStdinDiff && file.path !== 'Commit Message',
  );

  let lastSelectionResetKey: string | null = null;
  let lastPendingCommentSeq = -1;

  // Reset transient selection state only when the file or compared commits actually change.
  $effect(() => {
    const nextKey = `${file.path}\0${baseCommitish ?? ''}\0${targetCommitish ?? ''}`;
    if (lastSelectionResetKey !== null && nextKey !== lastSelectionResetKey) {
      dragState = null;
      commentingLine = null;
      selectionAnchor = null;
    }
    lastSelectionResetKey = nextKey;
  });

  // Open an inline comment form when triggered from the keyboard shortcut ("c").
  $effect(() => {
    if (
      pendingLineComment &&
      pendingLineComment.filePath === file.path &&
      pendingLineComment.seq !== lastPendingCommentSeq
    ) {
      lastPendingCommentSeq = pendingLineComment.seq;
      commentingLine = { side: pendingLineComment.side, line: pendingLineComment.line };
      onConsumePendingComment?.(pendingLineComment.seq);
    }
  });

  function isLineCursor(
    chunkIndex: number,
    lineIndex: number | undefined,
    side: 'left' | 'right',
  ): boolean {
    if (
      fileIndex === undefined ||
      lineIndex === undefined ||
      !cursor ||
      cursor.fileIndex !== fileIndex ||
      cursor.chunkIndex !== chunkIndex ||
      cursor.lineIndex !== lineIndex
    ) {
      return false;
    }
    return diffMode === 'unified' ? true : cursor.side === side;
  }

  // Prefetch line counts once a collapsible file scrolls into view (or immediately when IntersectionObserver is unavailable).
  $effect(() => {
    if (!canExpand || isCollapsed) {
      isVisible = false;
      return;
    }
    if (typeof IntersectionObserver === 'undefined') {
      isVisible = true;
      return;
    }
    const el = rootEl;
    if (!el) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting) {
          isVisible = true;
        }
      },
      { rootMargin: '200px' },
    );
    observer.observe(el);
    return () => {
      observer.disconnect();
    };
  });

  $effect(() => {
    if (isVisible && !isCollapsed && canExpand) {
      onPrefetchLineCount?.(file);
    }
  });

  // Load Prism grammar and optional whole-file tokens for SFC files (.svelte/.vue/.astro).
  $effect(() => {
    // Clear prior revision tokens synchronously before async blob loads start so stale text never renders.
    wholeFileOldTokens = null;
    wholeFileNewTokens = null;
    if (isCollapsed || !useWholeFileTokens) {
      return;
    }

    let cancelled = false;
    const lang = detectedLanguage;
    const oldPath = file.oldPath || file.path;
    const needOld = file.status !== 'added' && Boolean(baseCommitish);
    const needNew = file.status !== 'deleted' && Boolean(targetCommitish);

    const readyPromise = isPrismLanguageLoaded(lang)
      ? Promise.resolve()
      : loadPrismLanguage(lang).catch(() => void 0);

    if (needOld && baseCommitish) {
      Promise.all([
        readyPromise,
        fetchFileBlobText(oldPath, baseCommitish).catch(() => null),
      ])
        .then(([, content]) => {
          if (cancelled) return;
          if (content === null || content.split('\n').length > MAX_WHOLE_FILE_LINES) {
            wholeFileOldTokens = null;
            return;
          }
          wholeFileOldTokens = tokenizeCode(content, lang);
        })
        .catch(() => {
          if (!cancelled) wholeFileOldTokens = null;
        });
    }

    if (needNew && targetCommitish) {
      Promise.all([
        readyPromise,
        fetchFileBlobText(file.path, targetCommitish).catch(() => null),
      ])
        .then(([, content]) => {
          if (cancelled) return;
          if (content === null || content.split('\n').length > MAX_WHOLE_FILE_LINES) {
            wholeFileNewTokens = null;
            return;
          }
          wholeFileNewTokens = tokenizeCode(content, lang);
        })
        .catch(() => {
          if (!cancelled) wholeFileNewTokens = null;
        });
    }

    return () => {
      cancelled = true;
    };
  });

  function getPrecomputedLineTokens(
    side: DiffSide,
    lineNumber: number | undefined,
    expectedCode: string,
  ): HighlightToken[] | null {
    if (!useWholeFileTokens || lineNumber === undefined) return null;
    const tokenLines = side === 'old' ? wholeFileOldTokens : wholeFileNewTokens;
    const lineTokens = tokenLines?.[lineNumber - 1];
    if (!lineTokens) return null;
    const joined = lineTokens.map((token) => token.content).join('');
    if (joined !== expectedCode && joined !== expectedCode.replace(/\r$/, '')) {
      return null;
    }
    return lineTokens;
  }

  function getThreadAnchorLine(thread: CommentThread): number {
    return Array.isArray(thread.line) ? thread.line[1] : thread.line;
  }

  // Compute rendered line sets so every thread renders once (either inline on its line or in the top unanchored tray).
  const threadPlacement = $derived.by(() => {
    const renderedOldLines = new Set<number>();
    const renderedNewLines = new Set<number>();

    for (const chunk of mergedChunks) {
      for (const line of chunk.lines) {
        if (line.oldLineNumber !== undefined) {
          renderedOldLines.add(line.oldLineNumber);
        }
        if (line.newLineNumber !== undefined) {
          renderedNewLines.add(line.newLineNumber);
        }
      }
    }

    const sorted = [...threads].sort((a, b) => a.createdAt.localeCompare(b.createdAt));
    const oldMap = new Map<number, CommentThread[]>();
    const newMap = new Map<number, CommentThread[]>();
    const unanchored: CommentThread[] = [];

    for (const thread of sorted) {
      const anchor = getThreadAnchorLine(thread);
      const side = thread.side ?? 'new';
      if (side === 'old' && renderedOldLines.has(anchor)) {
        const list = oldMap.get(anchor);
        if (list) list.push(thread);
        else oldMap.set(anchor, [thread]);
      } else if (side === 'new' && renderedNewLines.has(anchor)) {
        const list = newMap.get(anchor);
        if (list) list.push(thread);
        else newMap.set(anchor, [thread]);
      } else {
        unanchored.push(thread);
      }
    }

    return { oldMap, newMap, unanchored };
  });

  function getLineSelectionRange(
    chunkIndex: number,
    side: DiffSide,
    lineNumber: number | undefined,
  ): { isDragging: boolean; isFirst: boolean; isLast: boolean; isCommenting: boolean } {
    if (!lineNumber) {
      return { isDragging: false, isFirst: false, isLast: false, isCommenting: false };
    }

    if (dragState && dragState.chunkIndex === chunkIndex && dragState.side === side) {
      const min = Math.min(dragState.startLine, dragState.endLine);
      const max = Math.max(dragState.startLine, dragState.endLine);
      if (lineNumber >= min && lineNumber <= max) {
        return {
          isDragging: true,
          isFirst: lineNumber === min,
          isLast: lineNumber === max,
          isCommenting: false,
        };
      }
    }

    if (commentingLine && commentingLine.side === side) {
      const start = Array.isArray(commentingLine.line)
        ? commentingLine.line[0]
        : commentingLine.line;
      const end = Array.isArray(commentingLine.line)
        ? commentingLine.line[1]
        : commentingLine.line;
      if (lineNumber >= start && lineNumber <= end) {
        return { isDragging: false, isFirst: false, isLast: false, isCommenting: true };
      }
    }

    return { isDragging: false, isFirst: false, isLast: false, isCommenting: false };
  }

  function formatSelectionClasses(
    sel: ReturnType<typeof getLineSelectionRange>,
  ): string {
    if (sel.isDragging) {
      return `drag-selected${sel.isFirst ? ' drag-selected-first' : ''}${sel.isLast ? ' drag-selected-last' : ''}`;
    }
    if (sel.isCommenting) {
      return 'comment-selected';
    }
    return '';
  }

  function chunkContainsLine(chunkIndex: number, side: DiffSide, lineNumber: number): boolean {
    const chunk = mergedChunks[chunkIndex];
    if (!chunk) return false;
    return chunk.lines.some(
      (line) => (side === 'old' ? line.oldLineNumber : line.newLineNumber) === lineNumber,
    );
  }

  function toggleCommentingLine(side: DiffSide, nextLine: LineNumber) {
    if (
      commentingLine &&
      commentingLine.side === side &&
      JSON.stringify(commentingLine.line) === JSON.stringify(nextLine)
    ) {
      commentingLine = null;
    } else {
      commentingLine = { side, line: nextLine };
    }
  }

  function startOrExtendSelection(
    event: MouseEvent,
    chunkIndex: number,
    side: DiffSide,
    lineNumber: number | undefined,
  ) {
    if (lineNumber === undefined || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();

    if (
      event.shiftKey &&
      selectionAnchor &&
      selectionAnchor.side === side &&
      chunkContainsLine(chunkIndex, side, selectionAnchor.lineNumber)
    ) {
      const min = Math.min(selectionAnchor.lineNumber, lineNumber);
      const max = Math.max(selectionAnchor.lineNumber, lineNumber);
      commentingLine = {
        side,
        line: min === max ? min : [min, max],
      };
      selectionAnchor = { side, lineNumber };
      return;
    }

    selectionAnchor = { side, lineNumber };
    dragState = {
      chunkIndex,
      side,
      startLine: lineNumber,
      endLine: lineNumber,
    };
  }

  function handleCommentButtonClick(
    event: MouseEvent,
    side: DiffSide,
    lineNumber: number | undefined,
  ) {
    event.stopPropagation();
    if (event.detail !== 0 || lineNumber === undefined) return;
    selectionAnchor = { side, lineNumber };
    toggleCommentingLine(side, lineNumber);
  }

  function updateDragEnd(chunkIndex: number, side: DiffSide, lineNumber: number | undefined) {
    if (!dragState || lineNumber === undefined) return;
    if (dragState.chunkIndex !== chunkIndex || dragState.side !== side) return;
    if (dragState.endLine !== lineNumber) {
      dragState = { ...dragState, endLine: lineNumber };
    }
  }

  function handleWindowMouseUp() {
    if (!dragState) return;
    const { side, startLine, endLine } = dragState;
    dragState = null;

    const min = Math.min(startLine, endLine);
    const max = Math.max(startLine, endLine);
    const nextLine: LineNumber = min === max ? min : [min, max];
    toggleCommentingLine(side, nextLine);
  }

  function getSelectedCodeContent(selection: CommentingSelection): string {
    const [start, end] = Array.isArray(selection.line)
      ? selection.line
      : [selection.line, selection.line];
    const selected: string[] = [];
    for (const chunk of mergedChunks) {
      for (const line of chunk.lines) {
        const ln = selection.side === 'old' ? line.oldLineNumber : line.newLineNumber;
        if (ln !== undefined && ln >= start && ln <= end) {
          selected.push(line.content);
        }
      }
    }
    return selected.join('\n');
  }

  function formatCommentingTitle(line: LineNumber, side: DiffSide): string {
    const label = Array.isArray(line) ? `Lines ${line[0]}–${line[1]}` : `Line ${line}`;
    return side === 'old' ? `${label} (old)` : label;
  }

  async function submitInlineComment(body: string) {
    if (!commentingLine) return;
    const selection = commentingLine;
    const codeContent = getSelectedCodeContent(selection);
    await onAddComment(file.path, selection.line, body, codeContent, selection.side);
    commentingLine = null;
  }

  async function submitWholeFileComment(body: string) {
    await onAddComment(file.path, undefined, body, undefined, 'new');
    onCancelWholeFileComment?.();
  }

  function getUnifiedLineClass(line: ExpandedLine): string {
    if (isExpandedDiffLine(line)) return 'bg-github-bg-tertiary/80';
    if (line.type === 'add') return 'bg-diff-addition-bg/65';
    if (isDeletionLine(line)) return 'bg-diff-deletion-bg/65';
    return 'bg-transparent';
  }

  function getSplitCellClass(line: ExpandedLine | undefined): string {
    if (!line) return 'bg-github-bg-secondary/60';
    if (isExpandedDiffLine(line)) return 'bg-github-bg-tertiary/80';
    if (isDeletionLine(line)) return 'bg-diff-deletion-bg/65';
    if (line.type === 'add') return 'bg-diff-addition-bg/65';
    return 'bg-transparent';
  }

  function getPrefixChar(line: ExpandedLine | undefined): string {
    if (!line) return '';
    if (line.type === 'add') return '+';
    if (isDeletionLine(line)) return '-';
    return ' ';
  }

  function getPrefixClass(line: ExpandedLine | undefined): string {
    if (!line) return 'text-github-text-muted bg-github-bg-secondary';
    if (line.type === 'add') return 'text-github-accent bg-diff-addition-bg/80';
    if (isDeletionLine(line)) return 'text-github-danger bg-diff-deletion-bg/80';
    return 'text-github-text-muted bg-github-bg-secondary';
  }

  function getUnifiedThreadsForRow(line: ExpandedLine): CommentThread[] {
    const primaryNum = isDeletionLine(line) ? line.oldLineNumber : line.newLineNumber;
    const primarySide: DiffSide = isDeletionLine(line) ? 'old' : 'new';
    const primary = primaryNum ? (primarySide === 'old' ? threadPlacement.oldMap : threadPlacement.newMap).get(primaryNum) ?? [] : [];
    const secondary =
      !isDeletionLine(line) && line.type !== 'add' && line.oldLineNumber
        ? threadPlacement.oldMap.get(line.oldLineNumber) ?? []
        : [];
    if (secondary.length === 0) return primary;
    const seen = new Set<string>();
    const combined: CommentThread[] = [];
    for (const thread of [...primary, ...secondary]) {
      if (!seen.has(thread.id)) {
        seen.add(thread.id);
        combined.push(thread);
      }
    }
    return combined;
  }

  function isCommentingAnchorRow(
    side: DiffSide,
    lineNumber: number | undefined,
  ): boolean {
    if (!commentingLine || lineNumber === undefined || commentingLine.side !== side) return false;
    const anchor = Array.isArray(commentingLine.line)
      ? commentingLine.line[1]
      : commentingLine.line;
    return anchor === lineNumber;
  }
</script>

<svelte:window onmouseup={handleWindowMouseUp} />

{#snippet renderThreadCard(thread: CommentThread, isUnanchored = false)}
  <CommentThreadCard
    {thread}
    domId="comment-thread-{thread.id}"
    {isUnanchored}
    isHighlighted={highlightedThreadId === thread.id}
    {showAuthorBadges}
    {syntaxTheme}
    {onReplyToThread}
    {onToggleResolve}
    {onRemoveThread}
  />
{/snippet}

{#snippet renderInlineCommentForm(selection: CommentingSelection)}
  <CommentForm
    title="Comment on {formatCommentingTitle(selection.line, selection.side)}"
    selectedCode={getSelectedCodeContent(selection)}
    filename={file.path}
    {syntaxTheme}
    onSubmit={submitInlineComment}
    onCancel={() => {
      commentingLine = null;
    }}
  />
{/snippet}

<div
  bind:this={rootEl}
  class="bg-github-bg-primary"
  style="--line-number-width: 3.5rem;"
>
  {#if activeWholeFileComment}
    <div class="border-b border-github-border bg-github-bg-secondary/50 p-1">
      <CommentForm
        title="Comment on {file.path}"
        placeholder="Write a file-level comment... (Cmd/Ctrl+Enter to submit)"
        submitLabel="Comment"
        filename={file.path}
        {syntaxTheme}
        onSubmit={submitWholeFileComment}
        onCancel={() => onCancelWholeFileComment?.()}
      />
    </div>
  {/if}

  {#if threadPlacement.unanchored.length > 0}
    <div class="p-3 space-y-2 border-b border-github-border bg-github-bg-secondary/40">
      {#each threadPlacement.unanchored as thread (thread.id)}
        {@render renderThreadCard(thread, true)}
      {/each}
    </div>
  {/if}

  {#if mergedChunks.length === 0}
    <div class="px-4 py-4 text-xs text-github-text-muted font-mono">
      No text diff chunks for this file.
    </div>
  {:else}
    {#each mergedChunks as chunk, chunkIdx (chunkIdx)}
      {@const isFirstChunk = chunkIdx === 0}
      {@const isLastChunk = chunkIdx === mergedChunks.length - 1}
      {@const firstOriginalIndex = chunk.originalIndices[0] ?? 0}
      {@const lastOriginalIndex = chunk.originalIndices[chunk.originalIndices.length - 1] ?? 0}

      {#if isFirstChunk && chunk.hiddenLinesBefore > 0}
        <ExpandBar
          direction="down"
          hiddenLines={chunk.hiddenLinesBefore}
          isLoading={isExpandLoading}
          onExpandDown={() => onExpandLines(file, firstOriginalIndex, 'up')}
          onExpandAll={() => onExpandAllBetween(file, firstOriginalIndex, chunk.hiddenLinesBefore)}
        />
      {/if}

      {#if !isFirstChunk && chunk.hiddenLinesBefore > 0}
        <ExpandBar
          direction="both"
          hiddenLines={chunk.hiddenLinesBefore}
          isLoading={isExpandLoading}
          onExpandUp={() => onExpandLines(file, firstOriginalIndex - 1, 'down')}
          onExpandDown={() => onExpandLines(file, firstOriginalIndex, 'up')}
          onExpandAll={() => onExpandAllBetween(file, firstOriginalIndex, chunk.hiddenLinesBefore)}
        />
      {/if}

      <div class="border-b border-github-border/70 last:border-b-0">
        <div class="px-3 py-1 bg-github-bg-tertiary/70 text-github-text-muted font-mono text-xs select-none border-b border-github-border/50">
          {chunk.header}
        </div>

        {#if diffMode === 'unified'}
          {@const wordDiffMap = computeUnifiedWordDiffMap(chunk.lines as ExpandedLine[])}
          <div class="overflow-x-auto">
            <table class="w-full table-fixed border-collapse font-mono text-xs leading-5" data-diff-mode="unified">
              <colgroup>
                <col class="w-12" />
                <col class="w-12" />
                <col />
              </colgroup>
              <tbody>
                {#each chunk.lines as rawLine, lineIdx (lineIdx)}
                  {@const line = rawLine as ExpandedLine}
                  {@const lineSide: DiffSide = isDeletionLine(line) ? 'old' : 'new'}
                  {@const primaryLineNum = lineSide === 'old' ? line.oldLineNumber : line.newLineNumber}
                  {@const oldSel = getLineSelectionRange(chunkIdx, 'old', line.oldLineNumber)}
                  {@const newSel = getLineSelectionRange(chunkIdx, 'new', line.newLineNumber)}
                  {@const activeSel = newSel.isDragging || newSel.isCommenting ? newSel : oldSel}
                  {@const rowSelClass = formatSelectionClasses(activeSel)}
                  {@const precomputed = getPrecomputedLineTokens(
                    lineSide,
                    lineSide === 'old' ? line.oldLineNumber : line.newLineNumber,
                    line.content,
                  )}
                  {@const rowThreads = getUnifiedThreadsForRow(line)}
                  {@const showFormAfter =
                    isCommentingAnchorRow('new', line.newLineNumber) ||
                    isCommentingAnchorRow('old', line.oldLineNumber)}

                  <tr
                    id={fileIndex !== undefined
                      ? getElementId({ fileIndex, chunkIndex: chunkIdx, lineIndex: lineIdx, side: 'right' }, 'unified')
                      : undefined}
                    data-diff-line-row="true"
                    class="group {getUnifiedLineClass(line)} {rowSelClass} {isLineCursor(
                      chunkIdx,
                      lineIdx,
                      'right',
                    )
                      ? 'keyboard-cursor'
                      : ''}"
                    onmouseenter={() => {
                      if (dragState?.side === 'old') {
                        updateDragEnd(chunkIdx, 'old', line.oldLineNumber);
                      } else if (dragState?.side === 'new') {
                        updateDragEnd(chunkIdx, 'new', line.newLineNumber);
                      }
                    }}
                  >
                    <td
                      data-line-number={line.oldLineNumber}
                      data-side="old"
                      onmousedown={(event) =>
                        startOrExtendSelection(event, chunkIdx, 'old', line.oldLineNumber)}
                      onmouseenter={() => {
                        if (dragState?.side === 'old') {
                          updateDragEnd(chunkIdx, 'old', line.oldLineNumber);
                        }
                      }}
                      class="px-2 py-0.5 text-right select-none text-github-text-muted bg-github-bg-secondary border-r border-github-border/60 align-top {line.oldLineNumber
                        ? 'cursor-pointer hover:text-github-text-primary'
                        : ''}"
                    >
                      {line.oldLineNumber ?? ''}
                    </td>

                    <td
                      data-line-number={line.newLineNumber}
                      data-side="new"
                      onmousedown={(event) => {
                        const targetSide: DiffSide = isDeletionLine(line) ? 'old' : 'new';
                        const targetNum = isDeletionLine(line) ? line.oldLineNumber : line.newLineNumber;
                        startOrExtendSelection(event, chunkIdx, targetSide, targetNum);
                      }}
                      onmouseenter={() => {
                        const targetSide: DiffSide = isDeletionLine(line) ? 'old' : 'new';
                        const targetNum = isDeletionLine(line) ? line.oldLineNumber : line.newLineNumber;
                        if (dragState?.side === targetSide) {
                          updateDragEnd(chunkIdx, targetSide, targetNum);
                        }
                      }}
                      class="relative px-2 py-0.5 text-right select-none text-github-text-muted bg-github-bg-secondary border-r border-github-border/60 align-top {primaryLineNum
                        ? 'cursor-pointer hover:text-github-text-primary'
                        : ''}"
                    >
                      <span>{line.newLineNumber ?? ''}</span>
                      {#if primaryLineNum}
                        <div class="hidden group-hover:flex group-focus-within:flex items-center gap-1 absolute right-1 top-1/2 -translate-y-1/2 z-10">
                          {#if canOpenEditor && !isDeletionLine(line) && line.newLineNumber}
                            <button
                              type="button"
                              data-open-in-editor-button="true"
                              onmousedown={(event) => event.stopPropagation()}
                              onclick={(event) => {
                                event.stopPropagation();
                                if (line.newLineNumber) {
                                  onOpenInEditor?.(file.path, line.newLineNumber);
                                }
                              }}
                              title="Open in editor at line {line.newLineNumber}"
                              aria-label="Open {file.path} line {line.newLineNumber} in editor"
                              class="w-5 h-5 flex items-center justify-center rounded text-[10px] border border-github-border bg-github-bg-tertiary text-github-text-primary hover:border-github-accent cursor-pointer"
                            >
                              ↗
                            </button>
                          {/if}
                          <button
                            type="button"
                            data-comment-button="true"
                            onmousedown={(event) =>
                              startOrExtendSelection(event, chunkIdx, lineSide, primaryLineNum)}
                            onclick={(event) =>
                              handleCommentButtonClick(event, lineSide, primaryLineNum)}
                            title="Add comment on line {primaryLineNum} (drag to select range)"
                            aria-label="Add comment on {lineSide} line {primaryLineNum}"
                            class="w-5 h-5 flex items-center justify-center rounded text-xs font-bold cursor-pointer"
                            style="background-color: var(--color-yellow-btn-bg); color: var(--color-yellow-btn-text); border: 1px solid var(--color-yellow-btn-border);"
                          >
                            +
                          </button>
                        </div>
                      {/if}
                    </td>

                    <td class="p-0 align-top">
                      <div class="flex items-start min-h-[20px]">
                        <span class="w-5 text-center shrink-0 select-none border-r border-github-border/50 {getPrefixClass(line)}">
                          {getPrefixChar(line)}
                        </span>
                        <HighlightedCode
                          code={line.content}
                          filename={file.path}
                          {syntaxTheme}
                          precomputedTokens={precomputed}
                          diffSegments={wordDiffMap.get(lineIdx)}
                          class="flex-1 px-3 py-0.5 text-github-text-primary whitespace-pre-wrap break-all select-text"
                        />
                      </div>
                    </td>
                  </tr>

                  {#each rowThreads as thread (thread.id)}
                    <tr class="bg-github-bg-secondary/70">
                      <td colspan="3" class="p-2 border-t border-github-border/70">
                        {@render renderThreadCard(thread)}
                      </td>
                    </tr>
                  {/each}

                  {#if showFormAfter && commentingLine}
                    <tr class="bg-github-bg-secondary/70">
                      <td colspan="3" class="p-0 border-t border-github-border/70">
                        {@render renderInlineCommentForm(commentingLine)}
                      </td>
                    </tr>
                  {/if}
                {/each}
              </tbody>
            </table>
          </div>
        {:else}
          {@const splitLines = convertChunkToSideBySide(chunk)}
          <div class="overflow-x-auto">
            <table class="w-full table-fixed border-collapse font-mono text-xs leading-5" data-diff-mode="split">
              <colgroup>
                <col class="w-12" />
                <col class="w-1/2" />
                <col class="w-12" />
                <col class="w-1/2" />
              </colgroup>
              <tbody>
                {#each splitLines as sideLine, rowIdx (rowIdx)}
                  {@const oldSel = getLineSelectionRange(chunkIdx, 'old', sideLine.oldLineNumber)}
                  {@const newSel = getLineSelectionRange(chunkIdx, 'new', sideLine.newLineNumber)}
                  {@const oldSelClass = formatSelectionClasses(oldSel)}
                  {@const newSelClass = formatSelectionClasses(newSel)}
                  {@const oldThreads =
                    sideLine.oldLineNumber !== undefined
                      ? threadPlacement.oldMap.get(sideLine.oldLineNumber) ?? []
                      : []}
                  {@const newThreads =
                    sideLine.newLineNumber !== undefined
                      ? threadPlacement.newMap.get(sideLine.newLineNumber) ?? []
                      : []}
                  {@const showOldForm = isCommentingAnchorRow('old', sideLine.oldLineNumber)}
                  {@const showNewForm = isCommentingAnchorRow('new', sideLine.newLineNumber)}
                  {@const isNormalRow =
                    sideLine.oldLine?.type !== 'delete' &&
                    sideLine.oldLine?.type !== 'remove' &&
                    sideLine.newLine?.type !== 'add'}

                  <tr
                    data-diff-line-row="true"
                    onmouseenter={() => {
                      if (dragState?.side === 'old') {
                        updateDragEnd(chunkIdx, 'old', sideLine.oldLineNumber);
                      } else if (dragState?.side === 'new') {
                        updateDragEnd(chunkIdx, 'new', sideLine.newLineNumber);
                      }
                    }}
                  >
                    <td
                      data-line-number={sideLine.oldLineNumber}
                      data-side="old"
                      onmousedown={(event) =>
                        startOrExtendSelection(event, chunkIdx, 'old', sideLine.oldLineNumber)}
                      onmouseenter={() => updateDragEnd(chunkIdx, 'old', sideLine.oldLineNumber)}
                      class="group/old relative px-2 py-0.5 text-right select-none text-github-text-muted bg-github-bg-secondary border-r border-github-border/60 align-top {sideLine.oldLineNumber
                        ? 'cursor-pointer hover:text-github-text-primary'
                        : ''} {oldSelClass ? `${oldSelClass} selection-edge-left` : ''}"
                    >
                      <span>{sideLine.oldLineNumber ?? ''}</span>
                      {#if sideLine.oldLineNumber}
                        <div class="hidden group-hover/old:flex group-focus-within/old:flex items-center gap-1 absolute right-1 top-1/2 -translate-y-1/2 z-10">
                          <button
                            type="button"
                            data-comment-button="true"
                            onmousedown={(event) =>
                              startOrExtendSelection(event, chunkIdx, 'old', sideLine.oldLineNumber)}
                            onclick={(event) =>
                              handleCommentButtonClick(event, 'old', sideLine.oldLineNumber)}
                            title="Add comment on old line {sideLine.oldLineNumber}"
                            aria-label="Add comment on old line {sideLine.oldLineNumber}"
                            class="w-5 h-5 flex items-center justify-center rounded text-xs font-bold cursor-pointer"
                            style="background-color: var(--color-yellow-btn-bg); color: var(--color-yellow-btn-text); border: 1px solid var(--color-yellow-btn-border);"
                          >
                            +
                          </button>
                        </div>
                      {/if}
                    </td>

                    <td
                      id={fileIndex !== undefined && sideLine.oldOriginalIndex !== undefined
                        ? getElementId(
                            {
                              fileIndex,
                              chunkIndex: chunkIdx,
                              lineIndex: sideLine.oldOriginalIndex,
                              side: 'left',
                            },
                            'split',
                          )
                        : undefined}
                      class="p-0 align-top border-r border-github-border {getSplitCellClass(
                        sideLine.oldLine,
                      )} {oldSelClass} {isLineCursor(chunkIdx, sideLine.oldOriginalIndex, 'left')
                        ? 'keyboard-cursor'
                        : ''}"
                      onmouseenter={() => updateDragEnd(chunkIdx, 'old', sideLine.oldLineNumber)}
                    >
                      {#if sideLine.oldLine}
                        <div class="flex items-start min-h-[20px]">
                          <span class="w-5 text-center shrink-0 select-none border-r border-github-border/50 {getPrefixClass(sideLine.oldLine)}">
                            {getPrefixChar(sideLine.oldLine)}
                          </span>
                          <HighlightedCode
                            code={sideLine.oldLine.content}
                            filename={file.path}
                            {syntaxTheme}
                            precomputedTokens={getPrecomputedLineTokens(
                              'old',
                              sideLine.oldLineNumber,
                              sideLine.oldLine.content,
                            )}
                            diffSegments={sideLine.oldSegments}
                            class="flex-1 px-2.5 py-0.5 text-github-text-primary whitespace-pre-wrap break-all select-text"
                          />
                        </div>
                      {/if}
                    </td>

                    <td
                      data-line-number={sideLine.newLineNumber}
                      data-side="new"
                      onmousedown={(event) =>
                        startOrExtendSelection(event, chunkIdx, 'new', sideLine.newLineNumber)}
                      onmouseenter={() => updateDragEnd(chunkIdx, 'new', sideLine.newLineNumber)}
                      class="group/new relative px-2 py-0.5 text-right select-none text-github-text-muted bg-github-bg-secondary border-r border-github-border/60 align-top {sideLine.newLineNumber
                        ? 'cursor-pointer hover:text-github-text-primary'
                        : ''} {newSelClass ? `${newSelClass} selection-edge-left` : ''}"
                    >
                      <span>{sideLine.newLineNumber ?? ''}</span>
                      {#if sideLine.newLineNumber}
                        <div class="hidden group-hover/new:flex group-focus-within/new:flex items-center gap-1 absolute right-1 top-1/2 -translate-y-1/2 z-10">
                          {#if canOpenEditor}
                            <button
                              type="button"
                              data-open-in-editor-button="true"
                              onmousedown={(event) => event.stopPropagation()}
                              onclick={(event) => {
                                event.stopPropagation();
                                if (sideLine.newLineNumber) {
                                  onOpenInEditor?.(file.path, sideLine.newLineNumber);
                                }
                              }}
                              title="Open in editor at line {sideLine.newLineNumber}"
                              aria-label="Open {file.path} line {sideLine.newLineNumber} in editor"
                              class="w-5 h-5 flex items-center justify-center rounded text-[10px] border border-github-border bg-github-bg-tertiary text-github-text-primary hover:border-github-accent cursor-pointer"
                            >
                              ↗
                            </button>
                          {/if}
                          <button
                            type="button"
                            data-comment-button="true"
                            onmousedown={(event) =>
                              startOrExtendSelection(event, chunkIdx, 'new', sideLine.newLineNumber)}
                            onclick={(event) =>
                              handleCommentButtonClick(event, 'new', sideLine.newLineNumber)}
                            title="Add comment on new line {sideLine.newLineNumber}"
                            aria-label="Add comment on new line {sideLine.newLineNumber}"
                            class="w-5 h-5 flex items-center justify-center rounded text-xs font-bold cursor-pointer"
                            style="background-color: var(--color-yellow-btn-bg); color: var(--color-yellow-btn-text); border: 1px solid var(--color-yellow-btn-border);"
                          >
                            +
                          </button>
                        </div>
                      {/if}
                    </td>

                    <td
                      id={fileIndex !== undefined && sideLine.newOriginalIndex !== undefined
                        ? getElementId(
                            {
                              fileIndex,
                              chunkIndex: chunkIdx,
                              lineIndex: sideLine.newOriginalIndex,
                              side: 'right',
                            },
                            'split',
                          )
                        : undefined}
                      class="p-0 align-top {getSplitCellClass(
                        sideLine.newLine,
                      )} {newSelClass} {isLineCursor(chunkIdx, sideLine.newOriginalIndex, 'right')
                        ? 'keyboard-cursor'
                        : ''}"
                      onmouseenter={() => updateDragEnd(chunkIdx, 'new', sideLine.newLineNumber)}
                    >
                      {#if sideLine.newLine}
                        <div class="flex items-start min-h-[20px]">
                          <span class="w-5 text-center shrink-0 select-none border-r border-github-border/50 {getPrefixClass(sideLine.newLine)}">
                            {getPrefixChar(sideLine.newLine)}
                          </span>
                          <HighlightedCode
                            code={sideLine.newLine.content}
                            filename={file.path}
                            {syntaxTheme}
                            precomputedTokens={getPrecomputedLineTokens(
                              'new',
                              sideLine.newLineNumber,
                              sideLine.newLine.content,
                            )}
                            diffSegments={sideLine.newSegments}
                            class="flex-1 px-2.5 py-0.5 text-github-text-primary whitespace-pre-wrap break-all select-text"
                          />
                        </div>
                      {/if}
                    </td>
                  </tr>

                  {#if oldThreads.length > 0 || newThreads.length > 0}
                    <tr class="bg-github-bg-secondary/70">
                      {#if isNormalRow}
                        <td colspan="4" class="p-2 border-t border-github-border/70">
                          <div class="space-y-2">
                            {#each [...oldThreads, ...newThreads] as thread (thread.id)}
                              {@render renderThreadCard(thread)}
                            {/each}
                          </div>
                        </td>
                      {:else}
                        <td colspan="2" class="p-2 align-top border-t border-r border-github-border/70">
                          <div class="space-y-2">
                            {#each oldThreads as thread (thread.id)}
                              {@render renderThreadCard(thread)}
                            {/each}
                          </div>
                        </td>
                        <td colspan="2" class="p-2 align-top border-t border-github-border/70">
                          <div class="space-y-2">
                            {#each newThreads as thread (thread.id)}
                              {@render renderThreadCard(thread)}
                            {/each}
                          </div>
                        </td>
                      {/if}
                    </tr>
                  {/if}

                  {#if (showOldForm || showNewForm) && commentingLine}
                    <tr class="bg-github-bg-secondary/70">
                      {#if isNormalRow}
                        <td colspan="4" class="p-0 border-t border-github-border/70">
                          {@render renderInlineCommentForm(commentingLine)}
                        </td>
                      {:else}
                        <td colspan="2" class="p-0 align-top border-t border-r border-github-border/70">
                          {#if showOldForm}
                            {@render renderInlineCommentForm(commentingLine)}
                          {/if}
                        </td>
                        <td colspan="2" class="p-0 align-top border-t border-github-border/70">
                          {#if showNewForm}
                            {@render renderInlineCommentForm(commentingLine)}
                          {/if}
                        </td>
                      {/if}
                    </tr>
                  {/if}
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </div>

      {#if isLastChunk && chunk.hiddenLinesAfter > 0}
        <ExpandBar
          direction="up"
          hiddenLines={chunk.hiddenLinesAfter}
          isLoading={isExpandLoading}
          onExpandUp={() => onExpandLines(file, lastOriginalIndex, 'down')}
          onExpandAll={() =>
            onExpandLines(file, lastOriginalIndex, 'down', chunk.hiddenLinesAfter)}
        />
      {/if}
    {/each}
  {/if}
</div>
