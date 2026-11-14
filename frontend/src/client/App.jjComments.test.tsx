import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { HotkeysProvider } from 'react-hotkeys-hook';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import '@testing-library/jest-dom';

import type { DiffCommentThread, DiffResponse, JjChange, JjCommit, JjGraphRow } from '../types/diff';
import { DiffMode, type ClientWatchState } from '../types/watch';

import App from './App';

// Intentionally do NOT mock ./hooks/useDiffComments: these tests verify real
// useDiffComments + App integration in Jujutsu mode.
vi.mock('./hooks/useViewport', () => ({
  useViewport: vi.fn(() => ({ isMobile: false, isDesktop: true })),
}));

const mockViewedFiles = new Set<string>();
vi.mock('./hooks/useViewedFiles', () => ({
  useViewedFiles: vi.fn(() => ({
    viewedFiles: mockViewedFiles,
    changedSinceViewedFiles: new Set<string>(),
    hasLoadedInitialViewedFiles: true,
    toggleFileViewed: vi.fn(),
    isFileContentChanged: vi.fn(),
    getViewedFileRecord: vi.fn(),
    clearViewedFiles: vi.fn(),
  })),
}));

const mockWatchState: ClientWatchState = {
  isWatchEnabled: true,
  diffMode: DiffMode.DEFAULT,
  shouldReload: false,
  isReloading: false,
  lastChangeTime: null,
  lastChangeType: null,
  connectionStatus: 'connected',
};

vi.mock('./hooks/useFileWatch', () => ({
  useFileWatch: vi.fn(
    (onReload?: () => Promise<void>, _onCommentsChanged?: () => Promise<void>) => ({
      shouldReload: mockWatchState.shouldReload,
      isConnected: true,
      error: null,
      reload: vi.fn(async () => {
        if (onReload) {
          await onReload();
        }
      }),
      watchState: mockWatchState,
    }),
  ),
}));

Object.defineProperty(navigator, 'sendBeacon', {
  writable: true,
  value: vi.fn(),
});

Object.defineProperty(window, 'confirm', {
  writable: true,
  value: vi.fn(() => false),
});

class MockEventSource {
  onopen: (() => void) | null = null;
  onerror: ((err: Event) => void) | null = null;
  close = vi.fn();
  url: string;

  constructor(url: string) {
    this.url = url;
  }
}

Object.defineProperty(window, 'EventSource', {
  writable: true,
  value: MockEventSource,
});

function createDiffResponse(targetCommitId: string, filePath = 'test.ts'): DiffResponse {
  return {
    commit: targetCommitId,
    baseCommitish: `${targetCommitId}-`,
    targetCommitish: targetCommitId,
    requestedBaseCommitish: `${targetCommitId}-`,
    requestedTargetCommitish: targetCommitId,
    files: [
      {
        path: filePath,
        status: 'modified',
        additions: 1,
        deletions: 0,
        chunks: [
          {
            header: '@@ -10,1 +10,1 @@',
            oldStart: 10,
            oldLines: 1,
            newStart: 10,
            newLines: 1,
            lines: [
              {
                type: 'add',
                content: 'const updated = true;',
                newLineNumber: 10,
              },
            ],
          },
        ],
      },
    ],
    ignoreWhitespace: false,
    isEmpty: false,
  };
}

function createMockThread({
  id,
  filePath = 'test.ts',
  line = 10,
  body,
  resolved = false,
}: {
  id: string;
  filePath?: string;
  line?: number;
  body: string;
  resolved?: boolean;
}): DiffCommentThread {
  const timestamp = '2026-11-14T00:00:00.000Z';
  return {
    id,
    filePath,
    createdAt: timestamp,
    updatedAt: timestamp,
    resolved,
    position: { side: 'new', line },
    messages: [
      {
        id: `${id}:0`,
        body,
        author: 'User',
        createdAt: timestamp,
        updatedAt: timestamp,
      },
    ],
  };
}

function makeChange({
  changeId,
  shortChangeId,
  commitId,
  shortCommitId,
  description,
  isWorkingCopy = false,
  commentCount = 1,
  unresolvedCommentCount = 1,
}: {
  changeId: string;
  shortChangeId: string;
  commitId: string;
  shortCommitId: string;
  description: string;
  isWorkingCopy?: boolean;
  commentCount?: number;
  unresolvedCommentCount?: number;
}): JjChange {
  return {
    changeId,
    shortChangeId,
    shortestChangeId: shortChangeId.slice(0, 1),
    commitId,
    shortCommitId,
    isWorkingCopy,
    isEmpty: false,
    hasDescription: true,
    isImmutable: false,
    isConflict: false,
    bookmarks: [],
    parentCommitIds: [],
    timestamp: '2026-11-14 10:00:00',
    description,
    commentCount,
    unresolvedCommentCount,
  };
}

function renderApp() {
  return render(
    <HotkeysProvider initiallyActiveScopes={['navigation']}>
      <App />
    </HotkeysProvider>,
  );
}

function mockJjFetch({
  changes,
  graphRows,
  commitsByChange,
  commentsByCommit,
  initialCommitId,
}: {
  changes: JjChange[];
  graphRows: JjGraphRow[];
  commitsByChange: Record<string, JjCommit[]>;
  commentsByCommit: Record<string, DiffCommentThread[]>;
  initialCommitId: string;
}) {
  vi.mocked(global.fetch).mockImplementation((input) => {
    const { pathname, searchParams } = new URL(String(input), 'http://localhost');

    if (pathname === '/api/jj/status') {
      return Promise.resolve({
        ok: true,
        json: async () => ({ isJj: true }),
      } as Response);
    }
    if (pathname === '/api/jj/changes') {
      return Promise.resolve({
        ok: true,
        json: async () => ({ changes, graphRows }),
      } as Response);
    }
    const commitsMatch = pathname.match(/^\/api\/jj\/changes\/([^/]+)\/commits$/);
    if (commitsMatch) {
      const changeId = decodeURIComponent(commitsMatch[1] ?? '');
      return Promise.resolve({
        ok: true,
        json: async () => ({ commits: commitsByChange[changeId] ?? [] }),
      } as Response);
    }
    if (pathname === '/api/jj/diff') {
      const target = searchParams.get('target') ?? initialCommitId;
      return Promise.resolve({
        ok: true,
        json: async () => createDiffResponse(target),
      } as Response);
    }
    if (pathname === '/api/jj/comments') {
      const target = searchParams.get('target') ?? '';
      return Promise.resolve({
        ok: true,
        json: async () => ({ threads: commentsByCommit[target] ?? [] }),
      } as Response);
    }
    if (pathname === '/api/diff') {
      return Promise.resolve({
        ok: true,
        json: async () => createDiffResponse('initial-boot-commit'),
      } as Response);
    }
    if (pathname.includes('/api/revisions')) {
      return Promise.resolve({
        ok: true,
        json: async () => null,
      } as Response);
    }
    return Promise.resolve({
      ok: true,
      json: async () => ({}),
      blob: async () => ({ size: 1024 }),
    } as Response);
  });
}

describe('App Component - Unmocked useDiffComments integration in Jujutsu mode', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.clear();
  });

  it('keeps initial load comments visible when diffData.commit updates and switches threads cleanly across changes', async () => {
    const changeA = makeChange({
      changeId: 'chgaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      shortChangeId: 'chgaaaaa',
      commitId: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      shortCommitId: 'aaaaaaaa',
      description: 'Change A feature',
      isWorkingCopy: true,
    });
    const changeB = makeChange({
      changeId: 'chgbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
      shortChangeId: 'chgbbbbb',
      commitId: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
      shortCommitId: 'bbbbbbbb',
      description: 'Change B cleanup',
      isWorkingCopy: false,
    });

    const graphRows: JjGraphRow[] = [
      {
        type: 'commit',
        key: changeA.commitId,
        graphCols: ['@'],
        downCols: [0],
        change: changeA,
      },
      {
        type: 'commit',
        key: changeB.commitId,
        graphCols: ['○'],
        downCols: [],
        change: changeB,
      },
    ];

    const commitA: JjCommit = {
      commitId: changeA.commitId,
      shortCommitId: changeA.shortCommitId,
      timestamp: changeA.timestamp,
      description: changeA.description,
      isWorkingCopy: true,
      commentCount: 1,
      unresolvedCommentCount: 1,
    };
    const commitB: JjCommit = {
      commitId: changeB.commitId,
      shortCommitId: changeB.shortCommitId,
      timestamp: changeB.timestamp,
      description: changeB.description,
      isWorkingCopy: false,
      commentCount: 1,
      unresolvedCommentCount: 1,
    };

    const threadA = createMockThread({
      id: 'thread-a',
      body: 'Initial comment on Change A',
    });
    const threadB = createMockThread({
      id: 'thread-b',
      body: 'Comment on Change B',
    });

    mockJjFetch({
      changes: [changeA, changeB],
      graphRows,
      commitsByChange: {
        [changeA.changeId]: [commitA],
        [changeB.changeId]: [commitB],
      },
      commentsByCommit: {
        [changeA.commitId]: [threadA],
        [changeB.commitId]: [threadB],
      },
      initialCommitId: changeA.commitId,
    });

    renderApp();

    // Verify Change A's inline diff comment and "1 open" header badge stay rendered
    // after diffData.commit updates to changeA.commitId.
    expect(await screen.findByText('Initial comment on Change A')).toBeInTheDocument();
    expect(screen.getByText('1 open')).toBeInTheDocument();

    // Switch to Change B in the Revisions tree.
    fireEvent.click(await screen.findByRole('button', { name: /Change B cleanup/ }));

    expect(await screen.findByText('Comment on Change B')).toBeInTheDocument();
    expect(screen.queryByText('Initial comment on Change A')).not.toBeInTheDocument();
    expect(screen.getByText('1 open')).toBeInTheDocument();

    const legacyCalls = vi
      .mocked(global.fetch)
      .mock.calls.map(([input]) => String(input))
      .filter((url) => url.startsWith('/api/comments') || url.includes('/api/comments-json'));
    expect(legacyCalls).toEqual([]);
  });

  it('preserves comments when switching the right target revision within a change', async () => {
    const commitV1: JjCommit = {
      commitId: '1111111111111111111111111111111111111111',
      shortCommitId: '11111111',
      timestamp: '2026-11-14 09:00:00',
      description: 'First revision',
      isWorkingCopy: false,
      commentCount: 1,
      unresolvedCommentCount: 1,
    };
    const commitV2: JjCommit = {
      commitId: '2222222222222222222222222222222222222222',
      shortCommitId: '22222222',
      timestamp: '2026-11-14 10:00:00',
      description: 'Second revision',
      isWorkingCopy: true,
      commentCount: 1,
      unresolvedCommentCount: 1,
    };

    const change = makeChange({
      changeId: 'chgevolooooooooooooooooooooooooo',
      shortChangeId: 'chgevolo',
      commitId: commitV2.commitId,
      shortCommitId: commitV2.shortCommitId,
      description: 'Evolog change under review',
      isWorkingCopy: true,
      commentCount: 2,
      unresolvedCommentCount: 2,
    });

    const graphRows: JjGraphRow[] = [
      {
        type: 'commit',
        key: change.commitId,
        graphCols: ['@'],
        downCols: [],
        change,
      },
    ];

    const latestThread = createMockThread({
      id: 'thread-v2',
      body: 'Latest revision comment',
    });
    const earlierThread = createMockThread({
      id: 'thread-v1',
      body: 'Earlier revision comment',
    });

    mockJjFetch({
      changes: [change],
      graphRows,
      commitsByChange: {
        [change.changeId]: [commitV1, commitV2],
      },
      commentsByCommit: {
        [commitV2.commitId]: [latestThread],
        [commitV1.commitId]: [earlierThread],
      },
      initialCommitId: commitV2.commitId,
    });

    renderApp();

    expect(await screen.findByText('Latest revision comment')).toBeInTheDocument();
    expect(screen.getByText('1 open')).toBeInTheDocument();

    // Open the right (To) revision selector dropdown and switch to #1 (commitV1).
    fireEvent.click(screen.getByTitle('Select diff head revision (To)'));
    fireEvent.click(await screen.findByRole('button', { name: /#1.*11111111/ }));

    await waitFor(() => {
      expect(screen.getByText('Earlier revision comment')).toBeInTheDocument();
      expect(screen.queryByText('Latest revision comment')).not.toBeInTheDocument();
      expect(screen.getByText('1 open')).toBeInTheDocument();
    });
  });
});
