import { describe, expect, it } from 'vitest';

import type { CommentThread, DiffFile } from '../../types/diff';

import {
  buildCommentIndex,
  createDiffNavigationFilters,
  findCursorForCommentThread,
  findNextMatchingPosition,
  findNextUnviewedFileIndex,
  getStartPosition,
  switchSplitCursorSide,
} from './keyboardNavigation';

const sampleFiles: DiffFile[] = [
  {
    path: 'src/a.ts',
    status: 'modified',
    additions: 1,
    deletions: 1,
    chunks: [
      {
        header: '@@ -1,3 +1,3 @@',
        oldStart: 1,
        oldLines: 3,
        newStart: 1,
        newLines: 3,
        lines: [
          { type: 'normal', content: 'const x = 1;', oldLineNumber: 1, newLineNumber: 1 },
          { type: 'delete', content: 'const oldVal = true;', oldLineNumber: 2 },
          { type: 'add', content: 'const newVal = true;', newLineNumber: 2 },
        ],
      },
    ],
  },
  {
    path: 'src/b.ts',
    status: 'added',
    additions: 1,
    deletions: 0,
    chunks: [
      {
        header: '@@ -0,0 +1,1 @@',
        oldStart: 0,
        oldLines: 0,
        newStart: 1,
        newLines: 1,
        lines: [{ type: 'add', content: 'export const b = 1;', newLineNumber: 1 }],
      },
    ],
  },
];

describe('keyboardNavigation utilities', () => {
  it('advances through lines, chunks, files, and unresolved comments', () => {
    const threads: CommentThread[] = [
      {
        id: 't1',
        file: 'src/b.ts',
        line: 1,
        side: 'new',
        createdAt: '2026-11-16T00:00:00Z',
        updatedAt: '2026-11-16T00:00:00Z',
        messages: [],
      },
    ];
    const filters = createDiffNavigationFilters(
      sampleFiles,
      buildCommentIndex(threads),
      'unified',
      new Set(),
    );

    const firstLine = findNextMatchingPosition(
      getStartPosition(null),
      'next',
      filters.line,
      sampleFiles,
      'unified',
    );
    expect(firstLine.position).toEqual({
      fileIndex: 0,
      chunkIndex: 0,
      lineIndex: 0,
      side: 'right',
    });

    const nextChunk = findNextMatchingPosition(
      firstLine.position!,
      'next',
      filters.chunk,
      sampleFiles,
      'unified',
    );
    expect(nextChunk.position?.lineIndex).toBe(1);

    const nextFile = findNextMatchingPosition(
      nextChunk.position!,
      'next',
      filters.file,
      sampleFiles,
      'unified',
    );
    expect(nextFile.position?.fileIndex).toBe(1);

    const commentPos = findNextMatchingPosition(
      getStartPosition(null),
      'next',
      filters.comment,
      sampleFiles,
      'unified',
    );
    expect(commentPos.position?.fileIndex).toBe(1);
    expect(findCursorForCommentThread(threads[0]!, sampleFiles)).toEqual({
      fileIndex: 1,
      chunkIndex: 0,
      lineIndex: 0,
      side: 'right',
    });
  });

  it('switches split side between paired delete/add rows and skips viewed files', () => {
    const movedRight = switchSplitCursorSide(
      { fileIndex: 0, chunkIndex: 0, lineIndex: 1, side: 'left' },
      'right',
      sampleFiles,
    );
    expect(movedRight).toEqual({ fileIndex: 0, chunkIndex: 0, lineIndex: 2, side: 'right' });

    const movedLeft = switchSplitCursorSide(movedRight, 'left', sampleFiles);
    expect(movedLeft).toEqual({ fileIndex: 0, chunkIndex: 0, lineIndex: 1, side: 'left' });

    expect(findNextUnviewedFileIndex(0, sampleFiles, new Set(['src/b.ts']))).toBeNull();
    expect(findNextUnviewedFileIndex(0, sampleFiles, new Set())).toBe(1);
  });
});
