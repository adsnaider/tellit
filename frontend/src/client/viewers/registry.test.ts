import { describe, expect, it } from 'vitest';

import type { DiffFile } from '../../types/diff';

import ImageDiffViewer from '../components/ImageDiffViewer.svelte';
import MarkdownDiffViewer from '../components/MarkdownDiffViewer.svelte';
import TextDiffViewer from '../components/TextDiffViewer.svelte';
import { getViewerForFile } from './registry';

function makeFile(path: string, status: DiffFile['status'] = 'modified'): DiffFile {
  return { path, status, additions: 1, deletions: 0, chunks: [] };
}

describe('viewer registry', () => {
  it('routes markdown, image, and text files to their Svelte 5 viewers', () => {
    const mdViewer = getViewerForFile(makeFile('README.md'));
    expect(mdViewer.id).toBe('markdown');
    expect(mdViewer.Component).toBe(MarkdownDiffViewer);
    expect(mdViewer.canExpandHiddenLines(makeFile('README.md'))).toBe(true);

    const markdownExtViewer = getViewerForFile(makeFile('docs/intro.MARKDOWN'));
    expect(markdownExtViewer.id).toBe('markdown');

    const imgViewer = getViewerForFile(makeFile('assets/icon.png'));
    expect(imgViewer.id).toBe('image');
    expect(imgViewer.Component).toBe(ImageDiffViewer);
    expect(imgViewer.canExpandHiddenLines(makeFile('assets/icon.png'))).toBe(false);

    const tsViewer = getViewerForFile(makeFile('src/main.ts'));
    expect(tsViewer.id).toBe('text');
    expect(tsViewer.Component).toBe(TextDiffViewer);

    const commitMsgViewer = getViewerForFile(makeFile('Commit Message', 'added'));
    expect(commitMsgViewer.id).toBe('text');
    expect(commitMsgViewer.canExpandHiddenLines(makeFile('Commit Message', 'added'))).toBe(false);
  });
});
