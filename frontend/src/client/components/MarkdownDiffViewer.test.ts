import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { DiffFile } from '../../types/diff';
import { getMergedChunksForFile } from '../utils/expandedLines';

import MarkdownDiffViewer from './MarkdownDiffViewer.svelte';

function createMarkdownFile(overrides: Partial<DiffFile> = {}): DiffFile {
  return {
    path: 'docs/guide.md',
    oldPath: 'docs/guide.md',
    status: 'modified',
    additions: 3,
    deletions: 1,
    chunks: [
      {
        header: '@@ -1,5 +1,6 @@',
        oldStart: 1,
        oldLines: 5,
        newStart: 1,
        newLines: 6,
        lines: [
          { type: 'normal', content: '---', oldLineNumber: 1, newLineNumber: 1 },
          { type: 'delete', content: 'title: Old Title', oldLineNumber: 2 },
          { type: 'add', content: 'title: New Title', newLineNumber: 2 },
          { type: 'normal', content: '---', oldLineNumber: 3, newLineNumber: 3 },
          { type: 'normal', content: '# Intro', oldLineNumber: 4, newLineNumber: 4 },
          { type: 'add', content: '- [x] Verified **GFM**', newLineNumber: 5 },
          { type: 'add', content: 'Visit [site](HTTPS://example.com)', newLineNumber: 6 },
        ],
      },
    ],
    ...overrides,
  };
}

describe('MarkdownDiffViewer.svelte', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('switches between Code, Diff Preview (with FrontmatterTable & GFM), and Full Preview tabs', async () => {
    vi.mocked(global.fetch).mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
      if (url.includes('ref=base1')) {
        return new Response('---\ntitle: Old Title\n---\n\n# Intro\n', { status: 200 });
      }
      if (url.includes('ref=head1')) {
        return new Response(
          '---\ntitle: New Title\n---\n\n# Intro\n\n- [x] Verified **GFM**\n\nVisit [site](HTTPS://example.com)\n\n```ts\nconst x = 1;\n\nconst y = 2;\n```\n',
          { status: 200 },
        );
      }
      if (url.includes('ref=head2')) {
        return new Response('---\ntitle: V2 Title\n---\n\n# Reloaded\n', { status: 200 });
      }
      return new Response('', { status: 404 });
    });

    const file = createMarkdownFile();
    const { container, rerender } = render(MarkdownDiffViewer, {
      file,
      mergedChunks: getMergedChunksForFile(file, undefined),
      diffMode: 'unified',
      threads: [],
      baseCommitish: 'base1',
      targetCommitish: 'head1',
      onExpandLines: vi.fn(),
      onExpandAllBetween: vi.fn(),
      onAddComment: vi.fn().mockResolvedValue(undefined),
      onReplyToThread: vi.fn().mockResolvedValue(undefined),
    });

    // Starts in Code diff view
    expect(container.querySelector('[data-diff-mode="unified"]')).toBeInTheDocument();

    // Full Preview tab appears once target blob fetch settles
    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Full Preview' })).toBeInTheDocument();
    });

    // Switch to Diff Preview
    await fireEvent.click(screen.getByRole('tab', { name: 'Diff Preview' }));
    expect(screen.getByText('Frontmatter')).toBeInTheDocument();
    expect(screen.getByText('Old Title')).toBeInTheDocument();
    expect(screen.getByText('New Title')).toBeInTheDocument();
    expect(screen.getByText('GFM')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'site' })).toHaveAttribute('target', '_blank');

    // Switch to Full Preview: ts fence alias loads typescript grammar and preserves blank lines
    await fireEvent.click(screen.getByRole('tab', { name: 'Full Preview' }));
    expect(screen.getByRole('heading', { name: 'Intro' })).toBeInTheDocument();
    expect(screen.getByText('Verified')).toBeInTheDocument();
    await waitFor(() => {
      expect(
        container.querySelector('.markdown-preview-code [data-token-types*="keyword"]'),
      ).not.toBeNull();
    });
    expect(container.querySelector('.markdown-preview-code .min-h-5')).not.toBeNull();

    // Switching commit stays on Full Preview across reload instead of resetting to Diff Preview
    await rerender({ targetCommitish: 'head2' });
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Reloaded' })).toBeInTheDocument();
    });
    expect(screen.getByRole('tab', { name: 'Full Preview' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
  });

  it('falls back gracefully to snapshot frontmatter when base fetch fails and skips blob fetches for stdin', async () => {
    vi.mocked(global.fetch).mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
      if (url.includes('ref=base1')) {
        return new Response('Missing', { status: 404, statusText: 'Not Found' });
      }
      return new Response('---\ntitle: OnlyTarget\n---\n\n# Body\n', { status: 200 });
    });

    const file = createMarkdownFile();
    const { rerender } = render(MarkdownDiffViewer, {
      file,
      mergedChunks: getMergedChunksForFile(file, undefined),
      diffMode: 'unified',
      threads: [],
      baseCommitish: 'base1',
      targetCommitish: 'head1',
      onExpandLines: vi.fn(),
      onExpandAllBetween: vi.fn(),
      onAddComment: vi.fn().mockResolvedValue(undefined),
      onReplyToThread: vi.fn().mockResolvedValue(undefined),
    });

    await fireEvent.click(screen.getByRole('tab', { name: 'Diff Preview' }));
    await waitFor(() => {
      expect(screen.getByText(/Base content unavailable/i)).toBeInTheDocument();
      expect(screen.getByText(/target only/i)).toBeInTheDocument();
    });

    vi.clearAllMocks();
    await rerender({ baseCommitish: 'stdin', targetCommitish: 'stdin' });
    expect(global.fetch).not.toHaveBeenCalled();
  });
});
