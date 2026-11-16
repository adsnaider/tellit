import { describe, expect, it } from 'vitest';

import type { JjChange, JjCommit } from '../../types/diff';

import {
  commitIdsMatch,
  filterInterestingCommits,
  isEmptyUndescribedChange,
  pickInitialChange,
} from './jjSelection';

function makeChange(
  overrides: Partial<JjChange> & Pick<JjChange, 'changeId' | 'commitId'>,
): JjChange {
  return {
    shortChangeId: overrides.changeId.slice(0, 8),
    shortCommitId: overrides.commitId.slice(0, 7),
    isWorkingCopy: false,
    isEmpty: false,
    hasDescription: true,
    timestamp: '2026-11-16 15:00:00',
    description: 'change',
    commentCount: 0,
    unresolvedCommentCount: 0,
    ...overrides,
  };
}

function makeCommit(overrides: Partial<JjCommit> & Pick<JjCommit, 'commitId'>): JjCommit {
  return {
    shortCommitId: overrides.commitId.slice(0, 7),
    timestamp: '2026-11-16 15:00:00',
    description: 'commit',
    isWorkingCopy: false,
    commentCount: 0,
    unresolvedCommentCount: 0,
    ...overrides,
  };
}

describe('jjSelection helpers', () => {
  it('matches full and unambiguous prefix commit IDs of length >= 6', () => {
    expect(commitIdsMatch('44235e975acae6', '44235e9')).toBe(true);
    expect(commitIdsMatch('44235', '44235e9')).toBe(false);
    expect(commitIdsMatch('abc1234', 'def5678')).toBe(false);
    expect(commitIdsMatch(undefined, '44235e9')).toBe(false);
  });

  it('identifies empty undescribed changes without comments', () => {
    const emptyWorkingCopy = makeChange({
      changeId: 'mkyoqzmz1111',
      commitId: '44235e975aca',
      isWorkingCopy: true,
      isEmpty: true,
      hasDescription: false,
      description: '(no description set)',
    });
    expect(isEmptyUndescribedChange(emptyWorkingCopy)).toBe(true);
    expect(isEmptyUndescribedChange({ ...emptyWorkingCopy, commentCount: 1 })).toBe(false);
    expect(isEmptyUndescribedChange({ ...emptyWorkingCopy, hasDescription: true })).toBe(false);
  });

  it('walks from an empty undescribed working copy to its non-empty parent', () => {
    const parent = makeChange({
      changeId: 'ltppwqry2222',
      commitId: '335eb579f263',
      description: 'Add dprint configuration',
    });
    const workingCopy = makeChange({
      changeId: 'mkyoqzmz1111',
      commitId: '44235e975aca',
      isWorkingCopy: true,
      isEmpty: true,
      hasDescription: false,
      parentCommitIds: ['335eb579f263'],
    });

    expect(pickInitialChange([workingCopy, parent])).toEqual(parent);
    expect(pickInitialChange([workingCopy, parent], '44235e9')).toEqual(parent);
  });

  it('selects an explicit matching commit when non-empty', () => {
    const first = makeChange({
      changeId: 'aaaaaaaa1111',
      commitId: '11111111aaaa',
      isWorkingCopy: true,
    });
    const second = makeChange({
      changeId: 'bbbbbbbb2222',
      commitId: '22222222bbbb',
    });

    expect(pickInitialChange([first, second], '2222222')).toEqual(second);
  });

  it('filters unreviewed intermediate commits while keeping reviewed, selected, and latest commits', () => {
    const commits: JjCommit[] = [
      makeCommit({ commitId: '11111111aaaa', approved: true }),
      makeCommit({ commitId: '22222222bbbb' }),
      makeCommit({ commitId: '33333333cccc', commentCount: 2 }),
      makeCommit({ commitId: '44444444dddd' }),
      makeCommit({ commitId: '55555555eeee' }),
    ];

    const filtered = filterInterestingCommits(commits, {
      baseCommitId: '4444444',
      targetCommitId: '5555555',
    });

    expect(filtered.map((item) => item.commitNumber)).toEqual([5, 4, 3, 1]);
  });
});
