import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { JjChange, JjGraphRow } from '../../types/diff';

import JjRevisionTree from './JjRevisionTree.svelte';

const changes: JjChange[] = [
  {
    changeId: 'mkyoqzmz11111111',
    shortChangeId: 'mkyoqzmz',
    shortestChangeId: 'mk',
    commitId: '44235e975acae6',
    shortCommitId: '44235e9',
    isWorkingCopy: true,
    isEmpty: true,
    hasDescription: false,
    timestamp: '2026-11-16 15:47:08',
    description: '(no description set)',
    commentCount: 0,
    unresolvedCommentCount: 0,
  },
  {
    changeId: 'ltppwqry22222222',
    shortChangeId: 'ltppwqry',
    shortestChangeId: 'lt',
    commitId: '335eb579f263bb',
    shortCommitId: '335eb57',
    isWorkingCopy: false,
    isImmutable: false,
    isConflict: true,
    bookmarks: ['exp/svelte'],
    timestamp: '2026-11-16 12:22:54',
    description: 'Add revision tree',
    commentCount: 2,
    unresolvedCommentCount: 1,
    approved: true,
  },
  {
    changeId: 'oskwwqxq33333333',
    shortChangeId: 'oskwwqxq',
    shortestChangeId: 'os',
    commitId: '63730a283667fc',
    shortCommitId: '63730a2',
    isWorkingCopy: false,
    isImmutable: true,
    bookmarks: ['main'],
    timestamp: '2026-11-16 10:00:00',
    description: 'docs: add project README',
    commentCount: 0,
    unresolvedCommentCount: 0,
    cleanRebaseApproved: true,
  },
];

const graphRows: JjGraphRow[] = [
  {
    type: 'commit',
    key: 'r0',
    graphCols: ['@'],
    downCols: [0],
    change: changes[0]!,
  },
  {
    type: 'connector',
    key: 'r1',
    graphCols: ['│'],
  },
  {
    type: 'commit',
    key: 'r2',
    graphCols: ['×'],
    downCols: [0],
    change: changes[1]!,
  },
  {
    type: 'commit',
    key: 'r3',
    graphCols: ['◆'],
    downCols: [],
    change: changes[2]!,
  },
];

describe('JjRevisionTree.svelte', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('renders graph glyphs, shortest prefix, bookmarks, conflict/empty badges, and status dots', async () => {
    const onSelectChange = vi.fn();
    const { container } = render(JjRevisionTree, {
      changes,
      graphRows,
      selectedChangeId: 'ltppwqry22222222',
      onSelectChange,
    });

    expect(container.querySelector('[data-node-kind="working"]')).not.toBeNull();
    expect(container.querySelector('[data-node-kind="conflict"]')).not.toBeNull();
    expect(container.querySelector('[data-node-kind="immutable"]')).not.toBeNull();
    expect(screen.getByText('exp/svelte')).toBeInTheDocument();
    expect(screen.getByText('conflict')).toBeInTheDocument();
    expect(screen.getByText('(empty)')).toBeInTheDocument();

    expect(container.querySelector('[data-status-dot="unresolved"]')).not.toBeNull();
    expect(container.querySelector('[data-status-dot="clean-rebase"]')).not.toBeNull();

    await fireEvent.click(screen.getByRole('button', { name: /oskwwqxq/ }));
    expect(onSelectChange).toHaveBeenCalledWith('oskwwqxq33333333');
  });

  it('persists panel collapse state and submits revision queries via Find', async () => {
    const onQuerySubmit = vi.fn().mockResolvedValue(true);
    render(JjRevisionTree, {
      changes,
      graphRows,
      selectedChangeId: 'ltppwqry22222222',
      onSelectChange: vi.fn(),
      onQuerySubmit,
    });

    const toggleButton = screen.getByRole('button', { name: /Revisions/ });
    await fireEvent.click(toggleButton);
    expect(localStorage.getItem('tellit.revisionsTreeOpen')).toBe('false');
    expect(screen.queryByRole('list', { name: 'Jujutsu revisions' })).not.toBeInTheDocument();

    // Clicking Find reopens the panel and opens the search input
    await fireEvent.click(screen.getByRole('button', { name: 'Search revision' }));
    const input = screen.getByLabelText('Jump to change ID or revset');
    await fireEvent.input(input, { target: { value: '@-' } });
    await fireEvent.submit(input.closest('form')!);

    expect(onQuerySubmit).toHaveBeenCalledWith('@-');
  });
});
