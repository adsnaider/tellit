import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import '@testing-library/jest-dom';

import { type JjCommit } from '../../types/diff';

import {
  filterInterestingCommits,
  isInterestingCommit,
  JjCommitComparisonSelector,
} from './JjRevisionSelector';

function makeCommit(index: number, overrides: Partial<JjCommit> = {}): JjCommit {
  const hex = index.toString(16).repeat(40).slice(0, 40);
  return {
    commitId: overrides.commitId ?? hex,
    shortCommitId: overrides.shortCommitId ?? hex.slice(0, 8),
    timestamp: `2026-11-12 19:0${index}:00`,
    description: `Revision ${index}`,
    isWorkingCopy: false,
    commentCount: 0,
    unresolvedCommentCount: 0,
    ...overrides,
  };
}

describe('isInterestingCommit and filterInterestingCommits', () => {
  it('keeps latest, directly approved, commented, and active target/base commits while excluding cleanRebaseApproved-only commits', () => {
    const c1 = makeCommit(1, { description: 'Initial draft' });
    const c2 = makeCommit(2, { commentCount: 1, description: 'Commented revision' });
    const c3 = makeCommit(3, { description: 'Intermediate unreviewed' });
    const c4 = makeCommit(4, { approved: true, description: 'Directly approved' });
    const c5 = makeCommit(5, {
      cleanRebaseApproved: true,
      cleanRebaseSourceCommitNumber: 4,
      description: 'Clean rebase only',
    });
    const c6 = makeCommit(6, { description: 'Latest unreviewed' });

    expect(isInterestingCommit(c6, { isLatest: true })).toBe(true);
    expect(isInterestingCommit(c4, { isLatest: false })).toBe(true);
    expect(isInterestingCommit(c2, { isLatest: false })).toBe(true);
    expect(isInterestingCommit(c5, { isLatest: false })).toBe(false);
    expect(isInterestingCommit(c1, { isLatest: false, baseCommitId: 'parent' })).toBe(false);
    expect(
      isInterestingCommit(c3, {
        isLatest: false,
        targetCommitId: c3.commitId,
      }),
    ).toBe(true);
    expect(
      isInterestingCommit(c3, {
        isLatest: false,
        baseCommitId: c3.commitId.slice(0, 7),
      }),
    ).toBe(true);

    const filtered = filterInterestingCommits([c1, c2, c3, c4, c5, c6], {
      targetCommitId: c6.commitId,
      baseCommitId: 'parent',
    });

    expect(filtered.map((item) => item.commitNumber)).toEqual([6, 4, 2]);
    expect(filtered.map((item) => item.commit.commitId)).toEqual([
      c6.commitId,
      c4.commitId,
      c2.commitId,
    ]);
  });

  it('includes active 7-char prefix baseCommitId and targetCommitId even when otherwise unreviewed', () => {
    const c1 = makeCommit(1);
    const c2 = makeCommit(2);
    const c3 = makeCommit(3);

    const filtered = filterInterestingCommits([c1, c2, c3], {
      baseCommitId: c1.commitId.slice(0, 7),
      targetCommitId: c2.commitId.slice(0, 7),
    });

    expect(filtered.map((item) => item.commitNumber)).toEqual([3, 2, 1]);
  });
});

describe('JjCommitComparisonSelector', () => {
  const buildSixCommits = () => {
    const c1 = makeCommit(1, { description: 'Draft one' });
    const c2 = makeCommit(2, { commentCount: 2, description: 'Addressed feedback' });
    const c3 = makeCommit(3, { description: 'Intermediate tweak' });
    const c4 = makeCommit(4, { approved: true, description: 'Approved snapshot' });
    const c5 = makeCommit(5, {
      cleanRebaseApproved: true,
      cleanRebaseSourceCommitNumber: 4,
      description: 'Clean rebased snapshot',
    });
    const c6 = makeCommit(6, { description: 'Latest snapshot' });
    return { commits: [c1, c2, c3, c4, c5, c6], c3, c6 };
  };

  it('hides unreviewed intermediate revisions by default and expands/collapses inline', () => {
    const { commits, c3, c6 } = buildSixCommits();
    const onSelectBase = vi.fn();
    const onSelectTarget = vi.fn();

    const { rerender } = render(
      <JjCommitComparisonSelector
        commits={commits}
        targetCommitId={c6.commitId}
        baseCommitId="parent"
        onSelectBase={onSelectBase}
        onSelectTarget={onSelectTarget}
      />,
    );

    const fromTrigger = screen.getByTitle('Select diff base revision (From)');
    fireEvent.click(fromTrigger);

    expect(screen.getByText('Diff Base (From)')).toBeInTheDocument();
    expect(screen.getByText('(Entire Change)')).toBeInTheDocument();
    expect(screen.getByText('Latest snapshot')).toBeInTheDocument();
    expect(screen.getByText('Approved snapshot')).toBeInTheDocument();
    expect(screen.getByText('Addressed feedback')).toBeInTheDocument();
    expect(screen.queryByText('Clean rebased snapshot')).not.toBeInTheDocument();
    expect(screen.queryByText('Intermediate tweak')).not.toBeInTheDocument();
    expect(screen.queryByText('Draft one')).not.toBeInTheDocument();
    expect(screen.getByText('Show all 6 revisions')).toBeInTheDocument();
    expect(screen.getByText('3 hidden')).toBeInTheDocument();

    // Expand From inline without closing the dropdown
    fireEvent.click(screen.getByRole('button', { name: /Show all 6 revisions/i }));
    expect(screen.getByText('Diff Base (From)')).toBeInTheDocument();
    expect(screen.getByText('Clean rebased snapshot')).toBeInTheDocument();
    expect(screen.getByText('Intermediate tweak')).toBeInTheDocument();
    expect(screen.getByText('Draft one')).toBeInTheDocument();
    expect(screen.getByText('Hide 3 unreviewed revisions')).toBeInTheDocument();

    // Collapse again
    fireEvent.click(screen.getByRole('button', { name: /Hide 3 unreviewed revisions/i }));
    expect(screen.queryByText('Clean rebased snapshot')).not.toBeInTheDocument();
    expect(screen.queryByText('Intermediate tweak')).not.toBeInTheDocument();
    expect(screen.queryByText('Draft one')).not.toBeInTheDocument();

    // Expand and select #3 in From
    fireEvent.click(screen.getByRole('button', { name: /Show all 6 revisions/i }));
    fireEvent.click(screen.getByText('Intermediate tweak'));
    expect(onSelectBase).toHaveBeenCalledWith(c3.commitId);
    expect(screen.queryByText('Diff Base (From)')).not.toBeInTheDocument();

    // Re-render with #3 selected as baseCommitId: #3 remains visible and hiddenCount drops to 2
    rerender(
      <JjCommitComparisonSelector
        commits={commits}
        targetCommitId={c6.commitId}
        baseCommitId={c3.commitId}
        onSelectBase={onSelectBase}
        onSelectTarget={onSelectTarget}
      />,
    );

    fireEvent.click(screen.getByTitle('Select diff base revision (From)'));
    expect(screen.getByText('Intermediate tweak')).toBeInTheDocument();
    expect(screen.getByText('2 hidden')).toBeInTheDocument();
    expect(screen.queryByText('Draft one')).not.toBeInTheDocument();
    fireEvent.click(screen.getByTitle('Select diff base revision (From)'));

    // Reset baseCommitId to parent and inspect To dropdown
    rerender(
      <JjCommitComparisonSelector
        commits={commits}
        targetCommitId={c6.commitId}
        baseCommitId="parent"
        onSelectBase={onSelectBase}
        onSelectTarget={onSelectTarget}
      />,
    );

    const toTrigger = screen.getByTitle('Select diff head revision (To)');
    fireEvent.click(toTrigger);

    expect(screen.getByText('Diff Head / Commits (3 of 6)')).toBeInTheDocument();
    expect(screen.getByText('Latest snapshot')).toBeInTheDocument();
    expect(screen.getByText('Approved snapshot')).toBeInTheDocument();
    expect(screen.getByText('Addressed feedback')).toBeInTheDocument();
    expect(screen.queryByText('Clean rebased snapshot')).not.toBeInTheDocument();
    expect(screen.queryByText('Intermediate tweak')).not.toBeInTheDocument();
    expect(screen.queryByText('Draft one')).not.toBeInTheDocument();
    expect(screen.getByText('Show all 6 revisions')).toBeInTheDocument();

    // Expand To, verify header updates to (6), then select #3
    fireEvent.click(screen.getByRole('button', { name: /Show all 6 revisions/i }));
    expect(screen.getByText('Diff Head / Commits (6)')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Intermediate tweak'));
    expect(onSelectTarget).toHaveBeenCalledWith(c3.commitId);
    expect(screen.queryByText('Diff Head / Commits (6)')).not.toBeInTheDocument();
  });

  it('omits the visibility toggle footer when all commits are interesting', () => {
    const c1 = makeCommit(1, { approved: true, description: 'First approved' });
    const c2 = makeCommit(2, { commentCount: 1, description: 'Second commented' });
    const c3 = makeCommit(3, { description: 'Latest commit' });

    render(
      <JjCommitComparisonSelector
        commits={[c1, c2, c3]}
        targetCommitId={c3.commitId}
        baseCommitId="parent"
        onSelectBase={vi.fn()}
        onSelectTarget={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByTitle('Select diff base revision (From)'));
    expect(screen.queryByText(/Show all .* revisions/i)).not.toBeInTheDocument();
    fireEvent.click(screen.getByTitle('Select diff base revision (From)'));

    fireEvent.click(screen.getByTitle('Select diff head revision (To)'));
    expect(screen.getByText('Diff Head / Commits (3)')).toBeInTheDocument();
    expect(screen.queryByText(/Show all .* revisions/i)).not.toBeInTheDocument();
  });
});
