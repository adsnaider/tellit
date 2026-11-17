import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { CommentThread, DiffFile } from '../../types/diff';
import { getMergedChunksForFile } from '../utils/expandedLines';
import { loadPrismLanguage } from '../utils/languageLoader';

import TextDiffViewer from './TextDiffViewer.svelte';

const sampleTsFile: DiffFile = {
  path: 'src/example.ts',
  status: 'modified',
  additions: 2,
  deletions: 1,
  chunks: [
    {
      header: '@@ -9,3 +9,4 @@',
      oldStart: 9,
      oldLines: 3,
      newStart: 9,
      newLines: 4,
      lines: [
        { type: 'normal', content: 'const totalCount = 1;', oldLineNumber: 9, newLineNumber: 9 },
        { type: 'delete', content: 'const status = "pending";', oldLineNumber: 10 },
        { type: 'add', content: 'const status = "ready";', newLineNumber: 10 },
        { type: 'add', content: 'return totalCount;', newLineNumber: 11 },
        { type: 'normal', content: '}', oldLineNumber: 11, newLineNumber: 12 },
      ],
    },
  ],
};

const sampleThreads: CommentThread[] = [
  {
    id: 'thread-line-11',
    file: 'src/example.ts',
    line: 11,
    side: 'new',
    createdAt: '2026-11-16T10:00:00.000Z',
    updatedAt: '2026-11-16T10:00:00.000Z',
    messages: [
      {
        id: 'm1',
        body: 'Verify `totalCount` overflow',
        author: 'Reviewer',
        createdAt: '2026-11-16T10:00:00.000Z',
        updatedAt: '2026-11-16T10:00:00.000Z',
      },
    ],
  },
  {
    id: 'thread-unanchored',
    file: 'src/example.ts',
    line: 1,
    side: 'new',
    createdAt: '2026-11-16T09:00:00.000Z',
    updatedAt: '2026-11-16T09:00:00.000Z',
    messages: [
      {
        id: 'm0',
        body: 'Whole-file note',
        author: 'Reviewer',
        createdAt: '2026-11-16T09:00:00.000Z',
        updatedAt: '2026-11-16T09:00:00.000Z',
      },
    ],
  },
];

describe('TextDiffViewer.svelte', () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    await loadPrismLanguage('typescript');
  });

  it('renders unified diff with Prism tokens, word-level diff, word-click highlighting, and unanchored + inline threads', async () => {
    const mergedChunks = getMergedChunksForFile(sampleTsFile, {
      oldTotalLines: 14,
      newTotalLines: 15,
      expandedRanges: [],
    });

    const { container } = render(TextDiffViewer, {
      file: sampleTsFile,
      mergedChunks,
      diffMode: 'unified',
      threads: sampleThreads,
      showAuthorBadges: true,
      syntaxTheme: 'vsDark',
      baseCommitish: 'base1',
      targetCommitish: 'head1',
      openInEditorAvailable: true,
      onExpandLines: vi.fn(),
      onExpandAllBetween: vi.fn(),
      onAddComment: vi.fn(),
      onReplyToThread: vi.fn(),
      onToggleResolve: vi.fn(),
      onOpenInEditor: vi.fn(),
    });

    expect(container.querySelector('[data-diff-mode="unified"]')).toBeInTheDocument();
    // Prism tokenizes TypeScript keyword "const"
    expect(container.querySelector('[data-token-types*="keyword"]')).not.toBeNull();
    // Word-level diff highlights "pending" / "ready"
    expect(container.querySelector('.word-diff-removed')).toHaveTextContent('pending');
    expect(container.querySelector('.word-diff-added')).toHaveTextContent('ready');

    // Clicking a word token highlights all matching word tokens
    const wordTokens = container.querySelectorAll('.word-token[data-word="totalCount"]');
    expect(wordTokens.length).toBeGreaterThanOrEqual(2);
    await fireEvent.click(wordTokens[0]!);
    for (const token of wordTokens) {
      expect(token).toHaveClass('word-highlight');
    }

    // Both whole-file unanchored thread and inline line-11 thread render
    expect(screen.getByText('Whole-file note')).toBeInTheDocument();
    expect(screen.getByText('File')).toBeInTheDocument();
    expect(screen.getByText(/overflow/)).toBeInTheDocument();
    expect(screen.getByText('L11')).toBeInTheDocument();
  });

  it('supports drag selection across line numbers in split mode and submits range comments with suggestion preview', async () => {
    const onAddComment = vi.fn().mockResolvedValue(undefined);
    const onOpenInEditor = vi.fn();
    const mergedChunks = getMergedChunksForFile(sampleTsFile, undefined);

    const { container } = render(TextDiffViewer, {
      file: sampleTsFile,
      mergedChunks,
      diffMode: 'split',
      threads: [],
      baseCommitish: 'base1',
      targetCommitish: 'head1',
      openInEditorAvailable: true,
      onExpandLines: vi.fn(),
      onExpandAllBetween: vi.fn(),
      onAddComment,
      onReplyToThread: vi.fn(),
      onOpenInEditor,
    });

    expect(container.querySelector('[data-diff-mode="split"]')).toBeInTheDocument();

    // Drag across new-side line numbers 10..12
    const line10Cell = container.querySelector('td[data-side="new"][data-line-number="10"]')!;
    const line12Cell = container.querySelector('td[data-side="new"][data-line-number="12"]')!;

    await fireEvent.mouseDown(line10Cell, { button: 0 });
    await fireEvent.mouseEnter(line12Cell);
    await fireEvent.mouseUp(window);

    expect(screen.getByText('Comment on Lines 10–12')).toBeInTheDocument();

    // Insert suggestion template and submit
    await fireEvent.click(screen.getByRole('button', { name: '+ Suggestion' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Preview' }));
    expect(screen.getByText('Suggested change')).toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: 'Comment' }));
    await waitFor(() => {
      expect(onAddComment).toHaveBeenCalledWith(
        'src/example.ts',
        [10, 12],
        expect.stringContaining('```suggestion'),
        expect.stringContaining('return totalCount;'),
        'new',
      );
    });

    // Open in editor button works on new lines
    const editorBtn = screen.getByRole('button', { name: 'Open src/example.ts line 10 in editor' });
    await fireEvent.click(editorBtn);
    expect(onOpenInEditor).toHaveBeenCalledWith('src/example.ts', 10);
  });

  it('invokes expand callbacks from ExpandBar controls and renders Commit Message threads', async () => {
    const onExpandLines = vi.fn();
    const onExpandAllBetween = vi.fn();
    const onReplyToThread = vi.fn().mockResolvedValue(undefined);
    const onToggleResolve = vi.fn();

    const mergedChunks = getMergedChunksForFile(sampleTsFile, {
      oldTotalLines: 14,
      newTotalLines: 15,
      expandedRanges: [],
    });

    render(TextDiffViewer, {
      file: sampleTsFile,
      mergedChunks,
      diffMode: 'unified',
      threads: sampleThreads,
      baseCommitish: 'base1',
      targetCommitish: 'head1',
      onExpandLines,
      onExpandAllBetween,
      onAddComment: vi.fn(),
      onReplyToThread,
      onToggleResolve,
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Expand all 8 hidden lines' }));
    expect(onExpandAllBetween).toHaveBeenCalledWith(sampleTsFile, 0, 8);

    await fireEvent.click(screen.getByRole('button', { name: 'Expand all 3 hidden lines' }));
    expect(onExpandLines).toHaveBeenCalledWith(sampleTsFile, 0, 'down', 3);

    // Reply and resolve on inline thread
    await fireEvent.click(screen.getAllByRole('button', { name: 'Reply...' })[0]!);
    const replyInput = screen.getByLabelText('Reply to thread');
    await fireEvent.input(replyInput, { target: { value: 'Looks safe now' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Reply' }));
    await waitFor(() => {
      expect(onReplyToThread).toHaveBeenCalledWith('thread-unanchored', 'Looks safe now');
    });

    await fireEvent.click(screen.getAllByRole('button', { name: 'Resolve' })[0]!);
    expect(onToggleResolve).toHaveBeenCalledWith('thread-unanchored', false);
  });

  it('clears stale whole-file tokens when switching commits and validates expected line text', async () => {
    const svelteV1: DiffFile = {
      path: 'src/Card.svelte',
      status: 'modified',
      additions: 1,
      deletions: 1,
      chunks: [
        {
          header: '@@ -1,1 +1,1 @@',
          oldStart: 1,
          oldLines: 1,
          newStart: 1,
          newLines: 1,
          lines: [{ type: 'add', content: '<div>v1 line</div>', newLineNumber: 1 }],
        },
      ],
    };
    const svelteV2: DiffFile = {
      ...svelteV1,
      chunks: [
        {
          header: '@@ -1,1 +1,1 @@',
          oldStart: 1,
          oldLines: 1,
          newStart: 1,
          newLines: 1,
          lines: [{ type: 'add', content: '<section>v2 updated</section>', newLineNumber: 1 }],
        },
      ],
    };

    const tooManyLines = Array.from({ length: 2005 }, (_, i) => `<span>line ${i}</span>`).join(
      '\n',
    );
    vi.mocked(global.fetch).mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
      if (url.includes('ref=head1')) {
        return new Response('<div>v1 line</div>\n', { status: 200 });
      }
      if (url.includes('ref=head2')) {
        return new Response(tooManyLines, { status: 200 });
      }
      return new Response('', { status: 200 });
    });

    const { container, rerender } = render(TextDiffViewer, {
      file: svelteV1,
      mergedChunks: getMergedChunksForFile(svelteV1, undefined),
      diffMode: 'unified',
      threads: [],
      baseCommitish: 'base1',
      targetCommitish: 'head1',
      onExpandLines: vi.fn(),
      onExpandAllBetween: vi.fn(),
      onAddComment: vi.fn(),
      onReplyToThread: vi.fn(),
    });

    await waitFor(() => {
      expect(container.querySelector('[data-token-types*="tag"]')).not.toBeNull();
    });

    await rerender({
      file: svelteV2,
      mergedChunks: getMergedChunksForFile(svelteV2, undefined),
      targetCommitish: 'head2',
    });

    await waitFor(() => {
      const text = container.textContent ?? '';
      expect(text).toContain('v2 updated');
      expect(text).not.toContain('v1 line');
    });
  });

  it('keeps an open CommentForm anchored when merging chunks via expansion and supports keyboard + button and uppercase HTTPS links', async () => {
    const twoChunkFile: DiffFile = {
      path: 'src/multi.ts',
      status: 'modified',
      additions: 2,
      deletions: 0,
      chunks: [
        {
          header: '@@ -1,1 +1,2 @@',
          oldStart: 1,
          oldLines: 1,
          newStart: 1,
          newLines: 2,
          lines: [
            { type: 'normal', content: 'const first = 1;', oldLineNumber: 1, newLineNumber: 1 },
            { type: 'add', content: 'const addedTop = 2;', newLineNumber: 2 },
          ],
        },
        {
          header: '@@ -4,1 +5,2 @@',
          oldStart: 4,
          oldLines: 1,
          newStart: 5,
          newLines: 2,
          lines: [
            { type: 'normal', content: 'const second = 5;', oldLineNumber: 4, newLineNumber: 5 },
            { type: 'add', content: 'const addedBottom = 6;', newLineNumber: 6 },
          ],
        },
      ],
    };

    const onAddComment = vi.fn().mockResolvedValue(undefined);
    const httpsThread: CommentThread = {
      id: 'thread-https',
      file: 'src/multi.ts',
      line: 5,
      side: 'new',
      createdAt: '2026-11-16T10:00:00.000Z',
      updatedAt: '2026-11-16T10:00:00.000Z',
      messages: [
        {
          id: 'm-https',
          body: 'See [docs](HTTPS://example.com/Docs)',
          createdAt: '2026-11-16T10:00:00.000Z',
          updatedAt: '2026-11-16T10:00:00.000Z',
        },
      ],
    };

    const initialState = {
      oldContent: ['const first = 1;', 'const gap2 = 2;', 'const gap3 = 3;', 'const second = 5;'],
      newContent: [
        'const first = 1;',
        'const addedTop = 2;',
        'const gap2 = 2;',
        'const gap3 = 3;',
        'const second = 5;',
        'const addedBottom = 6;',
      ],
      oldTotalLines: 4,
      newTotalLines: 6,
      expandedRanges: [],
    };

    const { rerender } = render(TextDiffViewer, {
      file: twoChunkFile,
      mergedChunks: getMergedChunksForFile(twoChunkFile, initialState),
      diffMode: 'unified',
      threads: [httpsThread],
      baseCommitish: 'base1',
      targetCommitish: 'head1',
      onExpandLines: vi.fn(),
      onExpandAllBetween: vi.fn(),
      onAddComment,
      onReplyToThread: vi.fn(),
    });

    const link = screen.getByRole('link', { name: 'docs' });
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');

    // Keyboard activation (detail === 0) on the + comment button for new line 6 in chunk 1
    await fireEvent.click(screen.getByRole('button', { name: 'Add comment on new line 6' }));
    expect(screen.getByLabelText('Comment on Line 6')).toBeInTheDocument();

    // Expand all hidden lines between chunk 0 and chunk 1 so they merge into a single chunk
    const mergedState = {
      ...initialState,
      expandedRanges: [{ chunkIndex: 1, direction: 'up' as const, count: 2 }],
    };
    await rerender({
      mergedChunks: getMergedChunksForFile(twoChunkFile, mergedState),
    });

    expect(screen.getByLabelText('Comment on Line 6')).toBeInTheDocument();
    await fireEvent.input(screen.getByLabelText('Comment on Line 6'), {
      target: { value: 'Preserved after merge' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Comment' }));

    await waitFor(() => {
      expect(onAddComment).toHaveBeenCalledWith(
        'src/multi.ts',
        6,
        'Preserved after merge',
        'const addedBottom = 6;',
        'new',
      );
    });
  });
});
