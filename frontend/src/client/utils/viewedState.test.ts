import { beforeEach, describe, expect, it } from 'vitest';

import type { DiffFile } from '../../types/diff';

import { hydrateViewedState, setMultipleFilesViewed, toggleSingleFileViewed } from './viewedState';

function makeFile(path: string, content: string, overrides: Partial<DiffFile> = {}): DiffFile {
  return {
    path,
    status: 'modified',
    additions: 1,
    deletions: 0,
    chunks: [
      {
        header: '@@ -0,0 +1,1 @@',
        oldStart: 0,
        oldLines: 0,
        newStart: 1,
        newLines: 1,
        lines: [{ type: 'add', content, newLineNumber: 1 }],
      },
    ],
    ...overrides,
  };
}

describe('viewedState helpers', () => {
  const context = {
    baseCommitish: 'abc1111',
    targetCommitish: 'def2222',
    currentCommitHash: 'def2222',
    repositoryId: 'repo-test',
  };

  beforeEach(() => {
    localStorage.clear();
  });

  it('auto-marks generated, deleted, and pattern-matched files on hydrate', async () => {
    const files: DiffFile[] = [
      makeFile('src/app.ts', 'const x = 1;'),
      makeFile('package-lock.json', '{}', { isGenerated: true }),
      makeFile('src/old.ts', 'old', { status: 'deleted' }),
      makeFile('docs/notes.snap', 'snap'),
    ];

    const state = await hydrateViewedState(context, files, ['*.snap']);
    expect([...state.viewedPaths].sort()).toEqual([
      'docs/notes.snap',
      'package-lock.json',
      'src/old.ts',
    ]);
    expect(state.viewedPaths.has('src/app.ts')).toBe(false);
  });

  it('persists per-file and batch toggles and detects changedSinceViewed across revisions', async () => {
    const fileV1 = makeFile('src/app.ts', 'const v = 1;');
    const initial = await hydrateViewedState(context, [fileV1]);
    expect(initial.viewedPaths.has('src/app.ts')).toBe(false);

    const toggled = await toggleSingleFileViewed(context, initial.records, fileV1);
    expect(toggled.isViewed).toBe(true);

    // Same file content in a newer commit stays viewed via the repository hash index
    const nextContext = { ...context, targetCommitish: 'fff3333', currentCommitHash: 'fff3333' };
    const unchangedHydrate = await hydrateViewedState(nextContext, [fileV1]);
    expect(unchangedHydrate.viewedPaths.has('src/app.ts')).toBe(true);
    expect(unchangedHydrate.changedSinceViewedPaths.has('src/app.ts')).toBe(false);

    // Modified file content in a newer commit is un-marked and flagged changedSinceViewed
    const fileV2 = makeFile('src/app.ts', 'const v = 2;');
    const changedHydrate = await hydrateViewedState(
      { ...context, targetCommitish: 'ggg4444', currentCommitHash: 'ggg4444' },
      [fileV2],
    );
    expect(changedHydrate.viewedPaths.has('src/app.ts')).toBe(false);
    expect(changedHydrate.changedSinceViewedPaths.has('src/app.ts')).toBe(true);

    // Batch folder toggle marks and unmarks multiple files
    const fileA = makeFile('src/a.ts', 'a');
    const fileB = makeFile('src/b.ts', 'b');
    const batchAdded = await setMultipleFilesViewed(context, [], [fileA, fileB], true);
    expect(batchAdded).toHaveLength(2);
    const batchRemoved = await setMultipleFilesViewed(context, batchAdded, [fileA, fileB], false);
    expect(batchRemoved).toHaveLength(0);
  });
});
