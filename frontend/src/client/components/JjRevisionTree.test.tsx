import { fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import '@testing-library/jest-dom';

import { type JjChange, type JjGraphRow } from '../../types/diff';

import {
  commitIdsMatch,
  isEmptyUndescribedChange,
  pickInitialChange,
} from './JjRevisionSelector';
import {
  computeGraphLayout,
  JjRevisionTreePanel,
  REVISIONS_TREE_STORAGE_KEY,
} from './JjRevisionTree';

function makeChange(overrides: Partial<JjChange> = {}): JjChange {
  const changeId = overrides.changeId ?? 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
  const commitId = overrides.commitId ?? '1111111111111111111111111111111111111111';
  return {
    changeId,
    shortChangeId: overrides.shortChangeId ?? changeId.slice(0, 8),
    shortestChangeId: overrides.shortestChangeId ?? changeId.slice(0, 2),
    commitId,
    shortCommitId: overrides.shortCommitId ?? commitId.slice(0, 8),
    isWorkingCopy: false,
    isEmpty: false,
    hasDescription: true,
    isImmutable: false,
    isConflict: false,
    bookmarks: [],
    parentCommitIds: [],
    timestamp: '2026-11-12 19:00:00',
    description: 'Implement feature',
    commentCount: 0,
    unresolvedCommentCount: 0,
    ...overrides,
  };
}

describe('pickInitialChange and selection helpers', () => {
  it('matches exact commit IDs and 7-char Git prefixes against 40-char jj commit IDs', () => {
    const full = 'abcdef1234567890abcdef1234567890abcdef12';
    expect(commitIdsMatch(full, full)).toBe(true);
    expect(commitIdsMatch(full, 'abcdef1')).toBe(true);
    expect(commitIdsMatch('abcdef1', full)).toBe(true);
    expect(commitIdsMatch(full, 'abcde')).toBe(false);
    expect(commitIdsMatch(full, undefined)).toBe(false);
  });

  it('identifies empty undescribed changes only when commentCount is zero', () => {
    expect(
      isEmptyUndescribedChange(
        makeChange({ isEmpty: true, hasDescription: false, commentCount: 0 }),
      ),
    ).toBe(true);
    expect(
      isEmptyUndescribedChange(
        makeChange({ isEmpty: false, hasDescription: false, commentCount: 0 }),
      ),
    ).toBe(false);
    expect(
      isEmptyUndescribedChange(
        makeChange({ isEmpty: true, hasDescription: true, commentCount: 0 }),
      ),
    ).toBe(false);
    expect(
      isEmptyUndescribedChange(
        makeChange({ isEmpty: true, hasDescription: false, commentCount: 2 }),
      ),
    ).toBe(false);
  });

  it('skips empty undescribed @ along parentCommitIds even when an unrelated branch sits above @', () => {
    const unrelatedBranch = makeChange({
      changeId: 'uuuuuuuuuuuuuuuuuuuuuuuuuuuuuuuu',
      commitId: '9999999999999999999999999999999999999999',
      description: 'Unrelated branch tip',
      parentCommitIds: ['3333333333333333333333333333333333333333'],
    });
    const workingCopy = makeChange({
      changeId: 'wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww',
      commitId: '1111111111111111111111111111111111111111',
      isWorkingCopy: true,
      isEmpty: true,
      hasDescription: false,
      description: '(no description set)',
      parentCommitIds: ['2222222222222222222222222222222222222222'],
    });
    const parentChange = makeChange({
      changeId: 'pppppppppppppppppppppppppppppppp',
      commitId: '2222222222222222222222222222222222222222',
      description: 'Parent change to review',
      parentCommitIds: ['3333333333333333333333333333333333333333'],
    });
    const baseChange = makeChange({
      changeId: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
      commitId: '3333333333333333333333333333333333333333',
      isImmutable: true,
      description: 'main',
    });

    const changes = [unrelatedBranch, workingCopy, parentChange, baseChange];

    expect(pickInitialChange(changes)?.changeId).toBe(parentChange.changeId);
    expect(pickInitialChange(changes, '1111111')?.changeId).toBe(parentChange.changeId);
  });

  it('keeps @ when non-empty, described, or carrying comments', () => {
    const parent = makeChange({
      changeId: 'pppppppppppppppppppppppppppppppp',
      commitId: '2222222222222222222222222222222222222222',
      description: 'Parent',
    });

    const nonEmptyAt = makeChange({
      isWorkingCopy: true,
      isEmpty: false,
      hasDescription: false,
      parentCommitIds: [parent.commitId],
    });
    expect(pickInitialChange([nonEmptyAt, parent], '1111111')?.changeId).toBe(nonEmptyAt.changeId);

    const describedAt = makeChange({
      isWorkingCopy: true,
      isEmpty: true,
      hasDescription: true,
      description: 'Planned change',
      parentCommitIds: [parent.commitId],
    });
    expect(pickInitialChange([describedAt, parent])?.changeId).toBe(describedAt.changeId);

    const commentedAt = makeChange({
      isWorkingCopy: true,
      isEmpty: true,
      hasDescription: false,
      commentCount: 1,
      parentCommitIds: [parent.commitId],
    });
    expect(pickInitialChange([commentedAt, parent])?.changeId).toBe(commentedAt.changeId);
  });
});

describe('computeGraphLayout', () => {
  it('computes node kinds, box-drawing, ASCII, and elision connections', () => {
    const c1 = makeChange({ changeId: 'c1', commitId: '11111111' });
    const c2 = makeChange({ changeId: 'c2', commitId: '22222222' });
    const c3 = makeChange({ changeId: 'c3', commitId: '33333333', isImmutable: true });

    const rows: JjGraphRow[] = [
      { type: 'commit', key: '1', graphCols: ['@'], downCols: [0], change: c1 },
      { type: 'connector', key: 'conn-1', graphCols: ['├', '─', '╮'] },
      { type: 'commit', key: '2', graphCols: ['│', ' ', '×'], downCols: [0, 2], change: c2 },
      { type: 'connector', key: 'conn-2', graphCols: ['├', '─', '╯'] },
      {
        type: 'connector',
        key: 'elide',
        graphCols: ['~'],
        isElided: true,
        bridgesElided: true,
      },
      { type: 'commit', key: '3', graphCols: ['◆'], downCols: [], change: c3 },
    ];

    const layout = computeGraphLayout(rows);
    expect(layout[0]?.cols[0]).toMatchObject({ nodeKind: 'working', up: false, down: true });
    expect(layout[1]?.cols[0]).toMatchObject({ up: true, down: true, right: true });
    expect(layout[1]?.cols[1]).toMatchObject({ left: true, right: true });
    expect(layout[1]?.cols[2]).toMatchObject({ down: true, left: true });
    expect(layout[2]?.cols[2]).toMatchObject({ nodeKind: 'conflict', up: true, down: true });
    expect(layout[3]?.cols[2]).toMatchObject({ up: true, left: true });
    expect(layout[4]?.cols[0]).toMatchObject({
      isElided: true,
      up: true,
      down: true,
      dashedDown: true,
    });
    expect(layout[5]?.cols[0]).toMatchObject({ nodeKind: 'immutable', up: true, down: false });

    // Square and ASCII glyphs
    const asciiRows: JjGraphRow[] = [
      { type: 'connector', key: 'sq', graphCols: ['┌', '─', '┐', '└', '┘'] },
      { type: 'connector', key: 'asc', graphCols: ['|', '-', '/', '\\', '+'] },
      {
        type: 'commit',
        key: 'nodes',
        graphCols: ['o', '*', 'x', '◌'],
        downCols: [],
        change: c1,
      },
    ];
    const asciiLayout = computeGraphLayout(asciiRows);
    expect(asciiLayout[0]?.cols[0]).toMatchObject({ down: true, right: true });
    expect(asciiLayout[0]?.cols[2]).toMatchObject({ down: true, left: true });
    expect(asciiLayout[0]?.cols[3]).toMatchObject({ up: true, right: true });
    expect(asciiLayout[0]?.cols[4]).toMatchObject({ up: true, left: true });
    expect(asciiLayout[1]?.cols[0]).toMatchObject({ up: true, down: true });
    expect(asciiLayout[1]?.cols[1]).toMatchObject({ left: true, right: true });
    expect(asciiLayout[1]?.cols[2]).toMatchObject({ diagonal: '/' });
    expect(asciiLayout[1]?.cols[3]).toMatchObject({ diagonal: '\\' });
    expect(asciiLayout[1]?.cols[4]).toMatchObject({
      up: true,
      down: true,
      left: true,
      right: true,
    });
    expect(asciiLayout[2]?.cols.map((c) => c.nodeKind)).toEqual([
      'mutable',
      'immutable',
      'conflict',
      'hidden',
    ]);
  });
});

describe('JjRevisionTreePanel', () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  const buildSampleTree = () => {
    const emptyAt = makeChange({
      changeId: 'tplrlolnaaaaaaaaaaaaaaaaaaaaaaaa',
      shortChangeId: 'tplrloln',
      shortestChangeId: 't',
      commitId: '1111111111111111111111111111111111111111',
      isWorkingCopy: true,
      isEmpty: true,
      hasDescription: false,
      description: '(no description set)',
    });
    const active = makeChange({
      changeId: 'spkxnykkbbbbbbbbbbbbbbbbbbbbbbbb',
      shortChangeId: 'spkxnykk',
      shortestChangeId: 'sp',
      commitId: '2222222222222222222222222222222222222222',
      bookmarks: ['feat-tree'],
      description: 'Render revision graph',
      commentCount: 2,
      unresolvedCommentCount: 1,
    });
    const conflictBranch = makeChange({
      changeId: 'yrpvpvuocccccccccccccccccccccccc',
      shortChangeId: 'yrpvpvuo',
      shortestChangeId: 'y',
      commitId: '3333333333333333333333333333333333333333',
      isConflict: true,
      description: 'Conflicted branch',
    });
    const main = makeChange({
      changeId: 'vpvssspydddddddddddddddddddddddd',
      shortChangeId: 'vpvssspy',
      shortestChangeId: 'v',
      commitId: '4444444444444444444444444444444444444444',
      isImmutable: true,
      bookmarks: ['main'],
      description: 'Base commit',
      approved: true,
    });

    const graphRows: JjGraphRow[] = [
      { type: 'commit', key: emptyAt.commitId, graphCols: ['@'], downCols: [0], change: emptyAt },
      { type: 'commit', key: active.commitId, graphCols: ['○'], downCols: [0], change: active },
      {
        type: 'commit',
        key: conflictBranch.commitId,
        graphCols: ['│', ' ', '×'],
        downCols: [0, 2],
        change: conflictBranch,
      },
      { type: 'connector', key: 'merge-back', graphCols: ['├', '─', '╯'] },
      { type: 'commit', key: main.commitId, graphCols: ['◆'], downCols: [0], change: main },
      { type: 'connector', key: 'elide', graphCols: ['~'], isElided: true, bridgesElided: false },
    ];

    return {
      changes: [emptyAt, active, conflictBranch, main],
      graphRows,
      emptyAt,
      active,
      conflictBranch,
      main,
    };
  };

  it('renders graph nodes, connectors, bookmarks, comments, and allows clicking muted empty rows', () => {
    const { changes, graphRows, emptyAt, active } = buildSampleTree();
    const onSelectChange = vi.fn();

    const { container } = render(
      <JjRevisionTreePanel
        changes={changes}
        graphRows={graphRows}
        selectedChangeId={active.changeId}
        onSelectChange={onSelectChange}
        isCustomInput={false}
        setIsCustomInput={vi.fn()}
        customInput=""
        setCustomInput={vi.fn()}
        customError={null}
        handleCustomSubmit={vi.fn()}
      />,
    );

    expect(screen.getByText('Revisions')).toBeInTheDocument();
    expect(screen.getByText('feat-tree')).toBeInTheDocument();
    expect(screen.getByText('main')).toBeInTheDocument();
    expect(screen.getByText('(empty)')).toBeInTheDocument();
    expect(screen.getByTitle('1 unresolved comment')).toBeInTheDocument();
    expect(screen.getByTitle('Approved')).toBeInTheDocument();

    expect(container.querySelector('[data-node-kind="working"]')).toBeInTheDocument();
    expect(container.querySelector('[data-node-kind="mutable"]')).toBeInTheDocument();
    expect(container.querySelector('[data-node-kind="conflict"]')).toBeInTheDocument();
    expect(container.querySelector('[data-node-kind="immutable"]')).toBeInTheDocument();
    expect(container.querySelector('[data-elided="true"]')).toBeInTheDocument();

    const emptyRowBtn = screen.getByRole('button', { name: /tplrloln/ });
    expect(emptyRowBtn.className).toContain('opacity-55');
    fireEvent.click(emptyRowBtn);
    expect(onSelectChange).toHaveBeenCalledWith(emptyAt.changeId);
  });

  it('persists collapse/expand state in localStorage and shows selected summary when collapsed', () => {
    const { changes, graphRows, active } = buildSampleTree();
    const props = {
      changes,
      graphRows,
      selectedChangeId: active.changeId,
      onSelectChange: vi.fn(),
      isCustomInput: false,
      setIsCustomInput: vi.fn(),
      customInput: '',
      setCustomInput: vi.fn(),
      customError: null,
      handleCustomSubmit: vi.fn(),
    };

    const { unmount } = render(<JjRevisionTreePanel {...props} />);
    const toggleBtn = screen.getByRole('button', { name: /Revisions/i });
    expect(toggleBtn).toHaveAttribute('aria-expanded', 'true');

    fireEvent.click(toggleBtn);
    expect(toggleBtn).toHaveAttribute('aria-expanded', 'false');
    expect(window.localStorage.getItem(REVISIONS_TREE_STORAGE_KEY)).toBe('false');
    expect(screen.getByText('spkxnykk')).toBeInTheDocument();
    expect(screen.queryByRole('list', { name: 'Jujutsu revisions' })).not.toBeInTheDocument();

    unmount();

    render(<JjRevisionTreePanel {...props} />);
    expect(screen.getByRole('button', { name: /Revisions/i })).toHaveAttribute(
      'aria-expanded',
      'false',
    );
  });

  it('toggles and submits the inline custom revset search form', () => {
    const { changes, graphRows, active } = buildSampleTree();
    const setIsCustomInput = vi.fn();
    const setCustomInput = vi.fn();
    const handleCustomSubmit = vi.fn((e: React.FormEvent) => e.preventDefault());

    const { rerender } = render(
      <JjRevisionTreePanel
        changes={changes}
        graphRows={graphRows}
        selectedChangeId={active.changeId}
        onSelectChange={vi.fn()}
        isCustomInput={false}
        setIsCustomInput={setIsCustomInput}
        customInput=""
        setCustomInput={setCustomInput}
        customError={null}
        handleCustomSubmit={handleCustomSubmit}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Search revision' }));
    expect(setIsCustomInput).toHaveBeenCalledWith(true);

    rerender(
      <JjRevisionTreePanel
        changes={changes}
        graphRows={graphRows}
        selectedChangeId={active.changeId}
        onSelectChange={vi.fn()}
        isCustomInput={true}
        setIsCustomInput={setIsCustomInput}
        customInput="main"
        setCustomInput={setCustomInput}
        customError={null}
        handleCustomSubmit={handleCustomSubmit}
      />,
    );

    const input = screen.getByPlaceholderText('e.g. kkmpptxz or @-');
    fireEvent.change(input, { target: { value: '@-' } });
    expect(setCustomInput).toHaveBeenCalledWith('@-');

    fireEvent.click(screen.getByRole('button', { name: 'Go' }));
    expect(handleCustomSubmit).toHaveBeenCalledTimes(1);
  });

  it('shows green status dots for approved and clean-rebase-approved changes even with resolved comments', () => {
    const approvedResolved = makeChange({
      changeId: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      shortChangeId: 'aaaaaaaa',
      commitId: '1111111111111111111111111111111111111111',
      description: 'Approved after resolving comments',
      commentCount: 2,
      unresolvedCommentCount: 0,
      approved: true,
    });
    const cleanRebaseResolved = makeChange({
      changeId: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
      shortChangeId: 'bbbbbbbb',
      commitId: '2222222222222222222222222222222222222222',
      description: 'Clean rebased after approval',
      commentCount: 1,
      unresolvedCommentCount: 0,
      approved: false,
      cleanRebaseApproved: true,
    });
    const unapprovedResolved = makeChange({
      changeId: 'cccccccccccccccccccccccccccccccc',
      shortChangeId: 'cccccccc',
      commitId: '3333333333333333333333333333333333333333',
      description: 'Resolved without approval',
      commentCount: 2,
      unresolvedCommentCount: 0,
      approved: false,
      cleanRebaseApproved: false,
    });
    const approvedUnresolved = makeChange({
      changeId: 'dddddddddddddddddddddddddddddddd',
      shortChangeId: 'dddddddd',
      commitId: '4444444444444444444444444444444444444444',
      description: 'Approved with open comment',
      commentCount: 1,
      unresolvedCommentCount: 1,
      approved: true,
    });

    const changes = [
      approvedResolved,
      cleanRebaseResolved,
      unapprovedResolved,
      approvedUnresolved,
    ];
    const graphRows: JjGraphRow[] = changes.map((change, idx) => ({
      type: 'commit',
      key: change.commitId,
      graphCols: ['○'],
      downCols: idx < changes.length - 1 ? [0] : [],
      change,
    }));

    render(
      <JjRevisionTreePanel
        changes={changes}
        graphRows={graphRows}
        selectedChangeId={approvedResolved.changeId}
        onSelectChange={vi.fn()}
        isCustomInput={false}
        setIsCustomInput={vi.fn()}
        customInput=""
        setCustomInput={vi.fn()}
        customError={null}
        handleCustomSubmit={vi.fn()}
      />,
    );

    const approvedDot = screen.getByTitle('Approved (all comments resolved)');
    expect(approvedDot.className).toContain('bg-green-500');

    const cleanRebaseDot = screen.getByTitle('Approved (Clean Rebase)');
    expect(cleanRebaseDot.className).toContain('bg-emerald-400');

    const grayDot = screen.getByTitle('All comments resolved');
    expect(grayDot.className).toContain('bg-gray-400');

    const yellowDot = screen.getByTitle('1 unresolved comment');
    expect(yellowDot.className).toContain('bg-yellow-400');
  });
});
