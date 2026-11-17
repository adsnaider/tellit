import { describe, expect, it } from 'vitest';

import type { DiffChunk, DiffLine } from '../../types/diff';

import { computeUnifiedWordDiffMap, convertChunkToSideBySide } from './sideBySide';

describe('sideBySide and word-level pairing utilities', () => {
  it('pairs delete/add blocks in split mode and computes word-level diff segments', () => {
    const chunk: DiffChunk = {
      header: '@@ -1,4 +1,3 @@',
      oldStart: 1,
      oldLines: 4,
      newStart: 1,
      newLines: 3,
      lines: [
        { type: 'normal', content: 'const a = 1;', oldLineNumber: 1, newLineNumber: 1 },
        { type: 'delete', content: 'const status = "pending";', oldLineNumber: 2 },
        { type: 'delete', content: 'console.log("old");', oldLineNumber: 3 },
        { type: 'add', content: 'const status = "ready";', newLineNumber: 2 },
        { type: 'normal', content: 'return status;', oldLineNumber: 4, newLineNumber: 3 },
      ],
    };

    const rows = convertChunkToSideBySide(chunk);
    expect(rows).toHaveLength(4);
    expect(rows[0]).toMatchObject({ oldLineNumber: 1, newLineNumber: 1 });
    expect(rows[1]).toMatchObject({ oldLineNumber: 2, newLineNumber: 2 });
    expect(
      rows[1]?.oldSegments?.some((s) => s.type === 'removed' && s.value.includes('pending')),
    ).toBe(true);
    expect(rows[1]?.newSegments?.some((s) => s.type === 'added' && s.value.includes('ready'))).toBe(
      true,
    );
    // Extra deleted line has no right-hand line
    expect(rows[2]?.oldLineNumber).toBe(3);
    expect(rows[2]?.newLine).toBeUndefined();
    expect(rows[3]).toMatchObject({ oldLineNumber: 4, newLineNumber: 3 });
  });

  it('computes unified word-level diff map across paired delete/add runs', () => {
    const lines: DiffLine[] = [
      { type: 'delete', content: 'let count = 1;', oldLineNumber: 10 },
      { type: 'add', content: 'let count = 2;', newLineNumber: 10 },
      { type: 'normal', content: 'return count;', oldLineNumber: 11, newLineNumber: 11 },
    ];

    const map = computeUnifiedWordDiffMap(lines);
    expect(map.get(0)?.some((s) => s.type === 'removed' && s.value === '1')).toBe(true);
    expect(map.get(1)?.some((s) => s.type === 'added' && s.value === '2')).toBe(true);
    expect(map.has(2)).toBe(false);
  });
});
