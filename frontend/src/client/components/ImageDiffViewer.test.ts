import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { DiffFile } from '../../types/diff';

import ImageDiffViewer from './ImageDiffViewer.svelte';

describe('ImageDiffViewer.svelte', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(global.fetch).mockResolvedValue(
      new Response(new Blob([new Uint8Array(2048)]), { status: 200 }),
    );
  });

  it('renders added, deleted, and renamed images with blob URLs and metadata', async () => {
    const addedFile: DiffFile = {
      path: 'assets/logo.png',
      status: 'added',
      additions: 1,
      deletions: 0,
      chunks: [],
    };

    const { rerender } = render(ImageDiffViewer, {
      file: addedFile,
      mergedChunks: [],
      diffMode: 'split',
      threads: [],
      baseCommitish: 'base1',
      targetCommitish: 'head1',
      onExpandLines: vi.fn(),
      onExpandAllBetween: vi.fn(),
      onAddComment: vi.fn().mockResolvedValue(undefined),
      onReplyToThread: vi.fn().mockResolvedValue(undefined),
    });

    expect(screen.getByText('Added Image')).toBeInTheDocument();
    const addedImg = screen.getByRole('img');
    expect(addedImg).toHaveAttribute('src', '/api/blob/assets%2Flogo.png?ref=head1');
    Object.defineProperty(addedImg, 'naturalWidth', { configurable: true, value: 320 });
    Object.defineProperty(addedImg, 'naturalHeight', { configurable: true, value: 180 });
    await fireEvent.load(addedImg);
    await waitFor(() => {
      expect(screen.getByText('W: 320px | H: 180px | 2.0 KB')).toBeInTheDocument();
    });

    await rerender({
      file: {
        path: 'assets/new.png',
        oldPath: 'assets/old.png',
        status: 'deleted',
        additions: 0,
        deletions: 1,
        chunks: [],
      },
    });
    expect(screen.getByText('Deleted Image')).toBeInTheDocument();
    expect(screen.getByRole('img')).toHaveAttribute('src', '/api/blob/assets%2Fold.png?ref=base1');
  });

  it('supports Side-by-side, Swipe slider, and Onion-skin opacity modes for modified images', async () => {
    const modifiedFile: DiffFile = {
      path: 'assets/banner.png',
      oldPath: 'assets/banner.png',
      status: 'modified',
      additions: 1,
      deletions: 1,
      chunks: [],
    };

    const { container } = render(ImageDiffViewer, {
      file: modifiedFile,
      mergedChunks: [],
      diffMode: 'split',
      threads: [],
      baseCommitish: 'main',
      targetCommitish: 'feature',
      onExpandLines: vi.fn(),
      onExpandAllBetween: vi.fn(),
      onAddComment: vi.fn().mockResolvedValue(undefined),
      onReplyToThread: vi.fn().mockResolvedValue(undefined),
    });

    expect(screen.getByText('Modified Image')).toBeInTheDocument();
    expect(screen.getAllByRole('img')).toHaveLength(2);
    expect(screen.getByRole('button', { name: 'Side-by-side' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );

    // Switch to Swipe mode and move slider
    await fireEvent.click(screen.getByRole('button', { name: 'Swipe' }));
    const swipeSlider = screen.getByLabelText('Swipe comparison slider');
    await fireEvent.input(swipeSlider, { target: { value: '75' } });
    expect(screen.getByText('After (75%)')).toBeInTheDocument();
    const imgsInSwipe = container.querySelectorAll('img');
    expect(imgsInSwipe[1]?.getAttribute('style')).toContain('inset(0 25% 0 0)');

    // Switch to Onion-skin mode and change opacity
    await fireEvent.click(screen.getByRole('button', { name: 'Onion-skin' }));
    const opacitySlider = screen.getByLabelText('Onion-skin opacity slider');
    await fireEvent.input(opacitySlider, { target: { value: '30' } });
    expect(screen.getByText('Current (30%)')).toBeInTheDocument();
    const imgsInOnion = container.querySelectorAll('img');
    expect(imgsInOnion[1]?.getAttribute('style')).toContain('opacity: 0.3');
  });
});
