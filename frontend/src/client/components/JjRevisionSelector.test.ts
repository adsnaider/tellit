import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { JjChange, JjCommit } from '../../types/diff';

import JjRevisionSelector from './JjRevisionSelector.svelte';

const changes: JjChange[] = [
  {
    changeId: 'ltppwqry22222222',
    shortChangeId: 'ltppwqry',
    shortestChangeId: 'lt',
    commitId: '33333333cccc',
    shortCommitId: '3333333',
    isWorkingCopy: true,
    timestamp: '2026-11-16 15:00:00',
    description: 'Implement feature',
    commentCount: 0,
    unresolvedCommentCount: 0,
  },
];

const commits: JjCommit[] = [
  {
    commitId: '11111111aaaa',
    shortCommitId: '1111111',
    timestamp: '2026-11-16 13:00:00',
    description: 'Initial draft',
    isWorkingCopy: false,
    commentCount: 0,
    unresolvedCommentCount: 0,
    approved: true,
  },
  {
    commitId: '22222222bbbb',
    shortCommitId: '2222222',
    timestamp: '2026-11-16 14:00:00',
    description: 'Intermediate unreviewed',
    isWorkingCopy: false,
    commentCount: 0,
    unresolvedCommentCount: 0,
  },
  {
    commitId: '33333333cccc',
    shortCommitId: '3333333',
    timestamp: '2026-11-16 15:00:00',
    description: 'Implement feature',
    isWorkingCopy: true,
    commentCount: 0,
    unresolvedCommentCount: 0,
    cleanRebaseApproved: true,
    cleanRebaseSourceCommitNumber: 1,
  },
];

describe('JjRevisionSelector.svelte', () => {
  it('filters unreviewed evolog commits by default, expands on toggle, and selects From/To', async () => {
    const onSelectBase = vi.fn();
    const onSelectTarget = vi.fn();

    render(JjRevisionSelector, {
      changes,
      graphRows: [],
      selectedChangeId: 'ltppwqry22222222',
      commits,
      targetCommitId: '33333333cccc',
      baseCommitId: 'parent',
      onSelectChange: vi.fn(),
      onQueryChange: vi.fn(),
      onSelectBase,
      onSelectTarget,
      onToggleIgnoreRebase: vi.fn(),
      onToggleApprove: vi.fn(),
    });

    expect(screen.getByText('Rebase filter (interdiff only)')).toBeInTheDocument();

    // Open From dropdown: #2 is hidden until "Show all 3 revisions" is clicked
    await fireEvent.click(screen.getByRole('button', { name: 'Diff base revision (From)' }));
    expect(screen.getByText('1111111')).toBeInTheDocument();
    expect(screen.queryByText('2222222')).not.toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: /Show all 3 revisions/ }));
    expect(screen.getByText('2222222')).toBeInTheDocument();

    await fireEvent.click(screen.getByRole('button', { name: /#1:\s*1111111/ }));
    expect(onSelectBase).toHaveBeenCalledWith('11111111aaaa');
  });

  it('toggles rebase isolation, clears queryError on Find, and upgrades clean-rebase approval', async () => {
    const onToggleIgnoreRebase = vi.fn();
    const onToggleApprove = vi.fn();
    const onClearQueryError = vi.fn();

    render(JjRevisionSelector, {
      changes,
      graphRows: [],
      selectedChangeId: 'ltppwqry22222222',
      commits,
      targetCommitId: '33333333cccc',
      baseCommitId: '11111111aaaa',
      isRebaseDiff: true,
      rebaseIgnored: true,
      queryError: 'Revision not found',
      onClearQueryError,
      onSelectChange: vi.fn(),
      onQueryChange: vi.fn(),
      onSelectBase: vi.fn(),
      onSelectTarget: vi.fn(),
      onToggleIgnoreRebase,
      onToggleApprove,
    });

    const rebaseBtn = screen.getByRole('button', { name: 'Rebase hidden' });
    await fireEvent.click(rebaseBtn);
    expect(onToggleIgnoreRebase).toHaveBeenCalledTimes(1);

    const approveBtn = screen.getByRole('button', { name: 'Approved (Clean Rebase)' });
    await fireEvent.click(approveBtn);
    // Clean-rebase approval passes currentApproved = false so it records direct approval
    expect(onToggleApprove).toHaveBeenCalledWith('33333333cccc', false);

    await fireEvent.click(screen.getByRole('button', { name: 'Enter specific change ID' }));
    expect(onClearQueryError).toHaveBeenCalledTimes(1);
  });
});
