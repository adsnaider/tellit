import { describe, expect, it } from 'vitest';

import type { JjChange, JjGraphRow } from '../../types/diff';

import { computeGraphLayout, getReviewStatusDot, makeStandaloneCommitRow } from './jjGraph';

function makeChange(id: string): JjChange {
  return {
    changeId: id,
    shortChangeId: id.slice(0, 8),
    commitId: `${id}commit`,
    shortCommitId: id.slice(0, 7),
    isWorkingCopy: false,
    timestamp: '2026-11-16 15:00:00',
    description: 'desc',
    commentCount: 0,
    unresolvedCommentCount: 0,
  };
}

describe('jjGraph utilities', () => {
  it('computes node kinds, vertical stems, corners, and elision connectors', () => {
    const rows: JjGraphRow[] = [
      {
        type: 'commit',
        key: 'r0',
        graphCols: ['@', ' '],
        downCols: [0],
        change: { ...makeChange('aaaa1111'), isWorkingCopy: true },
      },
      {
        type: 'connector',
        key: 'r1',
        graphCols: ['├', '╮'],
      },
      {
        type: 'commit',
        key: 'r2',
        graphCols: ['│', '○'],
        downCols: [0, 1],
        change: makeChange('bbbb2222'),
      },
      {
        type: 'connector',
        key: 'r3',
        graphCols: ['~', '│'],
        isElided: true,
        bridgesElided: true,
      },
      {
        type: 'commit',
        key: 'r4',
        graphCols: ['◆', '╯'],
        downCols: [],
        change: { ...makeChange('cccc3333'), isImmutable: true },
      },
    ];

    const layout = computeGraphLayout(rows);
    expect(layout).toHaveLength(5);
    expect(layout[0]?.cols[0]).toMatchObject({ nodeKind: 'working', up: false, down: true });
    expect(layout[1]?.cols[0]).toMatchObject({ up: true, down: true, right: true });
    expect(layout[1]?.cols[1]).toMatchObject({ down: true, left: true });
    expect(layout[2]?.cols[1]).toMatchObject({ nodeKind: 'mutable', up: true, down: true });
    expect(layout[3]?.cols[0]).toMatchObject({
      isElided: true,
      up: true,
      down: true,
      dashedDown: true,
    });
    expect(layout[4]?.cols[0]).toMatchObject({ nodeKind: 'immutable', up: true, down: false });
  });

  it('orders review status dot precedence: unresolved > approved > clean-rebase > resolved', () => {
    expect(
      getReviewStatusDot({
        commentCount: 3,
        unresolvedCommentCount: 1,
        approved: true,
      }),
    ).toMatchObject({
      kind: 'unresolved',
      title: '1 unresolved comment',
    });

    expect(
      getReviewStatusDot({
        commentCount: 2,
        unresolvedCommentCount: 0,
        approved: true,
        cleanRebaseApproved: true,
      }),
    ).toMatchObject({
      kind: 'approved',
      title: 'Approved (all comments resolved)',
    });

    expect(
      getReviewStatusDot({
        commentCount: 0,
        unresolvedCommentCount: 0,
        cleanRebaseApproved: true,
        cleanRebaseSourceNumber: 2,
      }),
    ).toMatchObject({
      kind: 'clean-rebase',
      title: 'Approved (Clean Rebase from #2)',
    });

    expect(
      getReviewStatusDot({
        commentCount: 1,
        unresolvedCommentCount: 0,
      }),
    ).toMatchObject({
      kind: 'resolved',
      title: 'All comments resolved',
    });

    expect(
      getReviewStatusDot({
        commentCount: 0,
        unresolvedCommentCount: 0,
        cleanRebaseApproved: true,
        cleanRebaseSourceCommitNumber: 3,
      }),
    ).toMatchObject({
      kind: 'clean-rebase',
      title: 'Approved (Clean Rebase from #3)',
    });

    expect(getReviewStatusDot({ commentCount: 0, unresolvedCommentCount: 0 })).toBeNull();
  });

  it('synthesizes standalone commit graph rows for working copy, immutable, and mutable changes', () => {
    expect(makeStandaloneCommitRow({ ...makeChange('w1'), isWorkingCopy: true }).graphCols).toEqual(
      ['@'],
    );
    expect(makeStandaloneCommitRow({ ...makeChange('i1'), isImmutable: true }).graphCols).toEqual([
      '◆',
    ]);
    expect(makeStandaloneCommitRow(makeChange('m1')).graphCols).toEqual(['○']);
  });
});
