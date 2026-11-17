import { describe, expect, it } from 'vitest';

import type { DiffFile, FileExpandedState } from '../../types/diff';

import {
  addExpandRange,
  consolidateBetweenChunkRanges,
  getHiddenLinesAfter,
  getHiddenLinesBefore,
  getMergedChunksForFile,
  splitBlobLines,
} from './expandedLines';

const twoChunkFile: DiffFile = {
  path: 'src/app.ts',
  status: 'modified',
  additions: 2,
  deletions: 0,
  chunks: [
    {
      header: '@@ -5,2 +5,3 @@',
      oldStart: 5,
      oldLines: 2,
      newStart: 5,
      newLines: 3,
      lines: [
        { type: 'normal', content: 'line 5', oldLineNumber: 5, newLineNumber: 5 },
        { type: 'add', content: 'added a', newLineNumber: 6 },
        { type: 'normal', content: 'line 6', oldLineNumber: 6, newLineNumber: 7 },
      ],
    },
    {
      header: '@@ -10,2 +11,2 @@',
      oldStart: 10,
      oldLines: 2,
      newStart: 11,
      newLines: 2,
      lines: [
        { type: 'normal', content: 'line 10', oldLineNumber: 10, newLineNumber: 11 },
        { type: 'normal', content: 'line 11', oldLineNumber: 11, newLineNumber: 12 },
      ],
    },
  ],
};

const sampleLines = Array.from({ length: 14 }, (_, idx) => `line ${idx + 1}`);

describe('expandedLines utilities', () => {
  it('splits blob lines while dropping trailing empty line after final newline', () => {
    expect(splitBlobLines('a\nb\n')).toEqual(['a', 'b']);
    expect(splitBlobLines('a\nb')).toEqual(['a', 'b']);
  });

  it('computes hidden lines before/after chunks and respects unknown total line counts', () => {
    const chunk0 = twoChunkFile.chunks[0]!;
    const chunk1 = twoChunkFile.chunks[1]!;

    expect(getHiddenLinesBefore(twoChunkFile, chunk0, 0, undefined)).toBe(4);
    expect(getHiddenLinesBefore(twoChunkFile, chunk1, 1, undefined)).toBe(3);
    expect(getHiddenLinesAfter(twoChunkFile, chunk0, 0, undefined)).toBe(0);
    expect(getHiddenLinesAfter(twoChunkFile, chunk1, 1, undefined)).toBe(-1);

    const stateWithTotal: FileExpandedState = {
      oldContent: sampleLines,
      newContent: sampleLines,
      oldTotalLines: 14,
      newTotalLines: 15,
      expandedRanges: [],
    };
    expect(getHiddenLinesAfter(twoChunkFile, chunk1, 1, stateWithTotal)).toBe(3);

    const addedFile: DiffFile = { ...twoChunkFile, status: 'added' };
    expect(getHiddenLinesBefore(addedFile, chunk0, 0, stateWithTotal)).toBe(0);
    expect(getHiddenLinesAfter(addedFile, chunk1, 1, stateWithTotal)).toBe(0);
  });

  it('merges adjacent chunks once gap is expanded and prevents duplicate overlapping lines', () => {
    // Expand down 2 lines from chunk 0 (lines 7, 8) and up 2 lines from chunk 1 (clamped to line 9)
    let ranges = addExpandRange([], 0, 'down', 2);
    ranges = addExpandRange(ranges, 1, 'up', 2);

    const state: FileExpandedState = {
      oldContent: sampleLines,
      newContent: sampleLines,
      oldTotalLines: 14,
      expandedRanges: ranges,
    };

    const merged = getMergedChunksForFile(twoChunkFile, state);
    expect(merged).toHaveLength(1);
    expect(merged[0]?.originalIndices).toEqual([0, 1]);
    expect(merged[0]?.hiddenLinesBefore).toBe(4);
    expect(merged[0]?.hiddenLinesAfter).toBe(3);

    const oldNums = merged[0]?.lines
      .map((line) => line.oldLineNumber)
      .filter((num): num is number => num !== undefined);
    expect(oldNums).toEqual([5, 6, 7, 8, 9, 10, 11]);
  });

  it('consolidates gap ranges cleanly via consolidateBetweenChunkRanges', () => {
    const initial = addExpandRange([], 0, 'down', 1);
    const consolidated = consolidateBetweenChunkRanges(initial, 1, 2);
    expect(consolidated).toEqual([{ chunkIndex: 1, direction: 'up', count: 3 }]);
  });
});
