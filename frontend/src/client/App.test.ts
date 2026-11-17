import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { DiffCommentThread, DiffResponse, JjChange, JjCommit } from '../types/diff';
import { resetClientSettingsForTests } from './services/userSettings';
import { VIEWED_HASH_VERSION } from './services/StorageService';

import App from './App.svelte';
import { APPEARANCE_STORAGE_KEY } from './utils/appearanceTheme';

class MockEventSource {
  static instances: MockEventSource[] = [];
  url: string;
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  close = vi.fn();

  constructor(url: string) {
    this.url = url;
    MockEventSource.instances.push(this);
  }
}

const sampleChanges: JjChange[] = [
  {
    changeId: 'mkyoqzmz11111111',
    shortChangeId: 'mkyoqzmz',
    shortestChangeId: 'mk',
    commitId: '44235e975acae6',
    shortCommitId: '44235e9',
    isWorkingCopy: true,
    isEmpty: true,
    hasDescription: false,
    parentCommitIds: ['335eb579f263bb'],
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
    isEmpty: false,
    hasDescription: true,
    bookmarks: ['main'],
    timestamp: '2026-11-16 12:22:54',
    description: 'Add dprint configuration for markdown formatting',
    commentCount: 2,
    unresolvedCommentCount: 1,
  },
];

const sampleCommits: JjCommit[] = [
  {
    commitId: '22222222222222',
    shortCommitId: '2222222',
    timestamp: '2026-11-16 11:00:00',
    description: 'Initial dprint draft',
    isWorkingCopy: false,
    commentCount: 1,
    unresolvedCommentCount: 0,
  },
  {
    commitId: '335eb579f263bb',
    shortCommitId: '335eb57',
    timestamp: '2026-11-16 12:22:54',
    description: 'Add dprint configuration for markdown formatting',
    isWorkingCopy: false,
    commentCount: 2,
    unresolvedCommentCount: 1,
    approved: false,
  },
];

const sampleJjDiff: DiffResponse = {
  commit: '335eb579f263bb',
  baseCommitish: '335eb579f263bb^',
  targetCommitish: '335eb579f263bb',
  repositoryId: 'repo-1',
  files: [
    {
      path: 'Commit Message',
      status: 'added',
      additions: 1,
      deletions: 0,
      chunks: [
        {
          header: '@@ -0,0 +1,1 @@',
          oldStart: 0,
          oldLines: 0,
          newStart: 1,
          newLines: 1,
          lines: [
            {
              type: 'add',
              content: 'Add dprint configuration for markdown formatting',
              newLineNumber: 1,
            },
          ],
        },
      ],
    },
    {
      path: 'dprint.json',
      status: 'added',
      additions: 3,
      deletions: 0,
      chunks: [
        {
          header: '@@ -0,0 +1,3 @@',
          oldStart: 0,
          oldLines: 0,
          newStart: 1,
          newLines: 3,
          lines: [
            { type: 'add', content: '{', newLineNumber: 1 },
            { type: 'add', content: '  "includes": ["**/*.md"]', newLineNumber: 2 },
            { type: 'add', content: '}', newLineNumber: 3 },
          ],
        },
      ],
    },
  ],
};

describe('App.svelte integration', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetClientSettingsForTests();
    localStorage.clear();
    MockEventSource.instances = [];
    document.documentElement.removeAttribute('data-theme');
    document.documentElement.removeAttribute('style');
    Object.defineProperty(window, 'matchMedia', {
      configurable: true,
      writable: true,
      value: vi.fn().mockImplementation(() => ({
        matches: true,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    });
    Object.defineProperty(window, 'EventSource', {
      configurable: true,
      writable: true,
      value: MockEventSource,
    });
  });

  it('bootstraps jj graph, From -> To interdiff + rebase toggle, approval, generated-file collapse, and SSE reload', async () => {
    let approved = false;
    const interDiffRebaseHidden: DiffResponse = {
      ...sampleJjDiff,
      baseCommitish: '22222222222222',
      isRebaseDiff: true,
      rebaseIgnored: true,
    };
    const interDiffRebaseShown: DiffResponse = {
      ...sampleJjDiff,
      baseCommitish: '22222222222222',
      isRebaseDiff: true,
      rebaseIgnored: false,
    };

    vi.mocked(global.fetch).mockImplementation(
      async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = typeof input === 'string' ? input : input.toString();
        if (url === '/api/user-settings') {
          return new Response(JSON.stringify({ client: {} }), { status: 200 });
        }
        if (url === '/api/jj/status') {
          return new Response(JSON.stringify({ isJj: true, repoRoot: '/workspace' }), {
            status: 200,
          });
        }
        if (url === '/api/jj/changes') {
          const updatedChanges = sampleChanges.map((c) =>
            c.changeId === 'ltppwqry22222222'
              ? { ...c, approved, unresolvedCommentCount: approved ? 0 : 1 }
              : c,
          );
          return new Response(JSON.stringify({ changes: updatedChanges, graphRows: [] }), {
            status: 200,
          });
        }
        if (url === '/api/jj/changes/ltppwqry22222222/commits') {
          const updatedCommits = sampleCommits.map((c) =>
            c.commitId === '335eb579f263bb'
              ? { ...c, approved, unresolvedCommentCount: approved ? 0 : 1 }
              : c,
          );
          return new Response(JSON.stringify({ commits: updatedCommits }), { status: 200 });
        }
        if (url.startsWith('/api/diff?')) {
          return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
        }
        if (url === '/api/jj/diff?target=335eb579f263bb&ignoreRebase=true') {
          return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
        }
        if (url === '/api/jj/diff?target=335eb579f263bb&base=22222222222222&ignoreRebase=true') {
          return new Response(JSON.stringify(interDiffRebaseHidden), { status: 200 });
        }
        if (url === '/api/jj/diff?target=335eb579f263bb&base=22222222222222&ignoreRebase=false') {
          return new Response(JSON.stringify(interDiffRebaseShown), { status: 200 });
        }
        if (url.startsWith('/api/generated-status/dprint.json')) {
          return new Response(
            JSON.stringify({
              path: 'dprint.json',
              ref: '335eb579f263bb',
              isGenerated: true,
              source: 'content',
            }),
            { status: 200 },
          );
        }
        if (url === '/api/jj/approve' && init?.method === 'POST') {
          const body = JSON.parse(String(init.body)) as { disapprove: boolean };
          approved = !body.disapprove;
          return new Response(JSON.stringify({ success: true, approved }), { status: 200 });
        }
        return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
      },
    );

    const { unmount } = render(App);

    await waitFor(() => {
      expect(screen.getAllByText('dprint.json').length).toBeGreaterThan(0);
    });

    // Generated status check marks dprint.json as generated, collapses it, and marks it viewed
    await waitFor(() => {
      expect(screen.getByText('Generated')).toBeInTheDocument();
      expect(screen.getByText('1/2 viewed')).toBeInTheDocument();
    });
    expect(screen.queryByText('+  "includes": ["**/*.md"]')).not.toBeInTheDocument();

    // Select an earlier evolog commit in From -> switches to rebase-isolated interdiff
    await fireEvent.click(screen.getByRole('button', { name: 'Diff base revision (From)' }));
    await fireEvent.click(screen.getByRole('button', { name: /#1:\s*2222222/ }));

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Rebase hidden' })).toBeInTheDocument();
    });

    // Toggle rebase isolation off -> shows "Show rebase"
    await fireEvent.click(screen.getByRole('button', { name: 'Rebase hidden' }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Show rebase' })).toBeInTheDocument();
    });

    // Approve the active commit -> button switches to "Approved"
    await fireEvent.click(screen.getByRole('button', { name: 'Approve' }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Approved' })).toBeInTheDocument();
    });

    // SSE /api/watch reload signal updates the Reload button, and clicking Reload clears it
    const watchStream = MockEventSource.instances.find((s) => s.url === '/api/watch')!;
    expect(watchStream).toBeDefined();
    watchStream.onmessage?.(
      new MessageEvent('message', {
        data: JSON.stringify({
          type: 'reload',
          diffMode: 'default',
          changeType: 'commit',
          timestamp: '2026-11-16T21:00:00Z',
        }),
      }),
    );

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Reload (updated)' })).toBeInTheDocument();
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Reload (updated)' }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Reload' })).toBeInTheDocument();
    });

    // Unmount closes both /api/heartbeat and /api/watch EventSources
    unmount();
    for (const source of MockEventSource.instances) {
      expect(source.close).toHaveBeenCalled();
    }
  });

  it('keeps piped stdin diffs intact even inside a jj workspace', async () => {
    const stdinDiff: DiffResponse = {
      commit: 'stdin',
      baseCommitish: 'stdin',
      targetCommitish: 'stdin',
      files: [
        {
          path: 'piped.txt',
          status: 'added',
          additions: 1,
          deletions: 0,
          chunks: [
            {
              header: '@@ -0,0 +1,1 @@',
              oldStart: 0,
              oldLines: 0,
              newStart: 1,
              newLines: 1,
              lines: [{ type: 'add', content: 'from stdin', newLineNumber: 1 }],
            },
          ],
        },
      ],
    };

    const calledUrls: string[] = [];
    vi.mocked(global.fetch).mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
      calledUrls.push(url);
      if (url === '/api/user-settings') {
        return new Response(JSON.stringify({ client: {} }), { status: 200 });
      }
      if (url === '/api/jj/status') {
        return new Response(JSON.stringify({ isJj: true, repoRoot: '/workspace' }), {
          status: 200,
        });
      }
      if (url.startsWith('/api/diff?')) {
        return new Response(JSON.stringify(stdinDiff), { status: 200 });
      }
      return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
    });

    render(App);

    await waitFor(() => {
      expect(screen.getAllByText('piped.txt').length).toBeGreaterThan(0);
    });

    expect(screen.queryByLabelText('Revisions')).not.toBeInTheDocument();
    expect(calledUrls.some((url) => url.startsWith('/api/jj/changes'))).toBe(false);
    expect(calledUrls.some((url) => url.startsWith('/api/jj/diff'))).toBe(false);
  });

  it('cancels superseded revset queries and keeps shell mounted on secondary errors', async () => {
    const emptyDiff: DiffResponse = {
      commit: '44235e975acae6',
      targetCommitish: '44235e975acae6',
      files: [],
    };
    const staleChange: JjChange = {
      changeId: 'zzzzzzzz44444444',
      shortChangeId: 'zzzzzzzz',
      commitId: '99999999999999',
      shortCommitId: '9999999',
      isWorkingCopy: false,
      timestamp: '2026-11-16 10:00:00',
      description: 'stale',
      commentCount: 0,
      unresolvedCommentCount: 0,
    };

    let resolveSlowQuery: ((res: Response) => void) | undefined;
    let failWorkingCopyDiff = false;

    vi.mocked(global.fetch).mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
      if (url === '/api/user-settings') {
        return new Response(JSON.stringify({ client: {} }), { status: 200 });
      }
      if (url === '/api/jj/status') {
        return new Response(JSON.stringify({ isJj: true, repoRoot: '/workspace' }), {
          status: 200,
        });
      }
      if (url === '/api/jj/changes') {
        return new Response(JSON.stringify({ changes: sampleChanges, graphRows: [] }), {
          status: 200,
        });
      }
      if (url === '/api/jj/changes?query=slowrev') {
        return new Promise<Response>((resolve) => {
          resolveSlowQuery = resolve;
        });
      }
      if (url === '/api/jj/changes/ltppwqry22222222/commits') {
        return new Response(JSON.stringify({ commits: sampleCommits }), { status: 200 });
      }
      if (url === '/api/jj/changes/mkyoqzmz11111111/commits') {
        return new Response(
          JSON.stringify({
            commits: [
              {
                commitId: '44235e975acae6',
                shortCommitId: '44235e9',
                timestamp: '2026-11-16 15:47:08',
                description: '(no description set)',
                isWorkingCopy: true,
                commentCount: 0,
                unresolvedCommentCount: 0,
              },
            ],
          }),
          { status: 200 },
        );
      }
      if (url.startsWith('/api/diff?')) {
        return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
      }
      if (url.startsWith('/api/jj/diff?target=335eb579f263bb')) {
        return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
      }
      if (url.startsWith('/api/jj/diff?target=44235e975acae6')) {
        if (failWorkingCopyDiff) {
          return new Response(JSON.stringify({ error: 'Diff failed for revision' }), {
            status: 500,
          });
        }
        return new Response(JSON.stringify(emptyDiff), { status: 200 });
      }
      return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
    });

    render(App);

    await waitFor(() => {
      expect(screen.getAllByText('dprint.json').length).toBeGreaterThan(0);
    });

    // Open sidebar search, submit slow query, then supersede by clicking mkyoqzmz
    await fireEvent.click(screen.getByRole('button', { name: 'Search revision' }));
    const queryInput = screen.getByLabelText('Jump to change ID or revset');
    await fireEvent.input(queryInput, { target: { value: 'slowrev' } });
    await fireEvent.submit(queryInput.closest('form')!);

    const workingCopyBtn = screen.getByRole('button', { name: /mkyoqzmz/ });
    await fireEvent.click(workingCopyBtn);

    await waitFor(() => {
      expect(screen.getByText('No changes in this revision')).toBeInTheDocument();
    });

    resolveSlowQuery?.(
      new Response(JSON.stringify({ changes: [staleChange], graphRows: [] }), { status: 200 }),
    );
    await Promise.resolve();
    expect(screen.queryByRole('button', { name: /zzzzzzzz/ })).not.toBeInTheDocument();
    expect(workingCopyBtn).toHaveAttribute('aria-current', 'true');

    // Switch back to ltppwqry, then make mkyoqzmz fail: shell stays mounted and selectedChangeId stays on ltppwqry
    const ltBtn = screen.getByRole('button', { name: /ltppwqry/ });
    await fireEvent.click(ltBtn);
    await waitFor(() => {
      expect(screen.getAllByText('dprint.json').length).toBeGreaterThan(0);
    });

    failWorkingCopyDiff = true;
    await fireEvent.click(workingCopyBtn);
    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Diff failed for revision');
    });
    expect(ltBtn).toHaveAttribute('aria-current', 'true');
  });

  it('preserves diff selection without re-bootstrapping when toggling Ignore Whitespace, and updates theme tokens', async () => {
    const gitDiff: DiffResponse = {
      commit: 'origin/main...HEAD',
      baseCommitish: 'abc1234',
      targetCommitish: 'def5678',
      requestedBaseCommitish: 'origin/main',
      requestedTargetCommitish: '.',
      requestedBaseMode: 'merge-base',
      files: [
        {
          path: 'src/main.rs',
          status: 'modified',
          additions: 1,
          deletions: 1,
          chunks: [
            {
              header: '@@ -1,1 +1,1 @@',
              oldStart: 1,
              oldLines: 1,
              newStart: 1,
              newLines: 1,
              lines: [
                { type: 'delete', content: 'old()', oldLineNumber: 1 },
                { type: 'add', content: 'new()', newLineNumber: 1 },
              ],
            },
          ],
        },
      ],
    };

    const calledUrls: string[] = [];
    vi.mocked(global.fetch).mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
      calledUrls.push(url);
      if (url === '/api/user-settings') {
        return new Response(JSON.stringify({ client: {} }), { status: 200 });
      }
      if (url === '/api/jj/status') {
        return new Response(JSON.stringify({ isJj: false }), { status: 200 });
      }
      if (url.startsWith('/api/diff?')) {
        return new Response(JSON.stringify(gitDiff), { status: 200 });
      }
      return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
    });

    render(App);

    await waitFor(() => {
      expect(screen.getByText('origin/main...HEAD')).toBeInTheDocument();
    });

    expect(calledUrls.filter((url) => url === '/api/jj/status')).toHaveLength(1);

    const ignoreWsCheckbox = screen.getByRole('checkbox', { name: 'Ignore Whitespace' });
    await fireEvent.click(ignoreWsCheckbox);

    await waitFor(() => {
      expect(calledUrls).toContain(
        '/api/diff?ignoreWhitespace=false&base=origin%2Fmain&target=.&baseMode=merge-base',
      );
    });
    expect(calledUrls.filter((url) => url === '/api/jj/status')).toHaveLength(1);

    const themeSelect = screen.getByLabelText('Theme preference');
    await fireEvent.change(themeSelect, { target: { value: 'light' } });

    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    expect(JSON.parse(localStorage.getItem(APPEARANCE_STORAGE_KEY) ?? '{}')).toMatchObject({
      theme: 'light',
    });
  });

  it('clears loading flags when handleReload or handleSelectJjChange is superseded', async () => {
    const emptyDiff: DiffResponse = {
      commit: '44235e975acae6',
      targetCommitish: '44235e975acae6',
      files: [],
    };
    let changesCallCount = 0;
    let resolveSlowReloadChanges: ((res: Response) => void) | undefined;
    let resolveSlowCommits: ((res: Response) => void) | undefined;

    vi.mocked(global.fetch).mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
      if (url === '/api/user-settings') {
        return new Response(JSON.stringify({ client: {} }), { status: 200 });
      }
      if (url === '/api/jj/status') {
        return new Response(JSON.stringify({ isJj: true, repoRoot: '/workspace' }), {
          status: 200,
        });
      }
      if (url === '/api/jj/changes') {
        changesCallCount += 1;
        if (changesCallCount === 1) {
          return new Response(JSON.stringify({ changes: sampleChanges, graphRows: [] }), {
            status: 200,
          });
        }
        return new Promise<Response>((resolve) => {
          resolveSlowReloadChanges = resolve;
        });
      }
      if (url === '/api/jj/changes/ltppwqry22222222/commits') {
        return new Response(JSON.stringify({ commits: sampleCommits }), { status: 200 });
      }
      if (url === '/api/jj/changes/mkyoqzmz11111111/commits') {
        return new Promise<Response>((resolve) => {
          resolveSlowCommits = resolve;
        });
      }
      if (url.startsWith('/api/diff?')) {
        return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
      }
      if (url.startsWith('/api/jj/diff?target=335eb579f263bb')) {
        return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
      }
      if (url.startsWith('/api/jj/diff?target=44235e975acae6')) {
        return new Response(JSON.stringify(emptyDiff), { status: 200 });
      }
      return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
    });

    render(App);

    await waitFor(() => {
      expect(screen.getAllByText('dprint.json').length).toBeGreaterThan(0);
    });

    // Trigger Reload, leaving /api/jj/changes pending, then supersede with change selection
    await fireEvent.click(screen.getByRole('button', { name: 'Reload' }));
    expect(screen.getByRole('button', { name: 'Reloading...' })).toBeDisabled();

    await fireEvent.click(screen.getByRole('button', { name: /mkyoqzmz/ }));
    expect(screen.getByRole('button', { name: 'Reload' })).toBeEnabled();

    // Supersede pending mkyoqzmz commits request with ltppwqry before it resolves
    await fireEvent.click(screen.getByRole('button', { name: /ltppwqry/ }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Diff head revision (To)' })).toBeInTheDocument();
    });

    resolveSlowReloadChanges?.(
      new Response(JSON.stringify({ changes: sampleChanges, graphRows: [] }), { status: 200 }),
    );
    resolveSlowCommits?.(new Response(JSON.stringify({ commits: [] }), { status: 200 }));
    await Promise.resolve();

    expect(screen.getByRole('button', { name: 'Reload' })).toBeEnabled();
    expect(screen.queryByText('Loading commits...')).not.toBeInTheDocument();
  });

  it('clears updated badge when generated-status promotes a file and resets viewed state on diff switch', async () => {
    const secondDiff: DiffResponse = {
      commit: '44235e975acae6',
      baseCommitish: '44235e975acae6^',
      targetCommitish: '44235e975acae6',
      repositoryId: 'repo-1',
      files: [
        {
          path: 'dprint.json',
          status: 'modified',
          additions: 1,
          deletions: 0,
          chunks: [],
        },
      ],
    };

    localStorage.setItem(
      'difit-viewed-index-v1/repo-1',
      JSON.stringify({
        version: 1,
        lastModifiedAt: '2026-11-16T00:00:00.000Z',
        entries: [
          {
            filePath: 'dprint.json',
            diffContentHash: 'stale-hash-from-earlier-patchset',
            hashVersion: VIEWED_HASH_VERSION,
            viewedAt: '2026-11-16T00:00:00.000Z',
          },
        ],
      }),
    );

    vi.mocked(global.fetch).mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
      if (url === '/api/user-settings') {
        return new Response(JSON.stringify({ client: {} }), { status: 200 });
      }
      if (url === '/api/jj/status') {
        return new Response(JSON.stringify({ isJj: true, repoRoot: '/workspace' }), {
          status: 200,
        });
      }
      if (url === '/api/jj/changes') {
        return new Response(JSON.stringify({ changes: sampleChanges, graphRows: [] }), {
          status: 200,
        });
      }
      if (url === '/api/jj/changes/ltppwqry22222222/commits') {
        return new Response(JSON.stringify({ commits: sampleCommits }), { status: 200 });
      }
      if (url === '/api/jj/changes/mkyoqzmz11111111/commits') {
        return new Response(
          JSON.stringify({
            commits: [
              {
                commitId: '44235e975acae6',
                shortCommitId: '44235e9',
                timestamp: '2026-11-16 15:47:08',
                description: 'second',
                isWorkingCopy: true,
                commentCount: 0,
                unresolvedCommentCount: 0,
              },
            ],
          }),
          { status: 200 },
        );
      }
      if (url.startsWith('/api/diff?')) {
        return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
      }
      if (url.startsWith('/api/jj/diff?target=335eb579f263bb')) {
        return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
      }
      if (url.startsWith('/api/jj/diff?target=44235e975acae6')) {
        return new Response(JSON.stringify(secondDiff), { status: 200 });
      }
      if (url.startsWith('/api/generated-status/dprint.json')) {
        return new Response(
          JSON.stringify({
            path: 'dprint.json',
            ref: '335eb579f263bb',
            isGenerated: true,
            source: 'content',
          }),
          { status: 200 },
        );
      }
      return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
    });

    render(App);

    // Generated promotion marks dprint.json viewed and clears the stale "updated" badge
    await waitFor(() => {
      expect(screen.getByText('1/2 viewed')).toBeInTheDocument();
    });
    expect(screen.queryByText('updated')).not.toBeInTheDocument();

    // Switch to mkyoqzmz while crypto/storage hydration fails: viewed state must reset synchronously
    const getItemSpy = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('Storage unavailable');
    });
    try {
      await fireEvent.click(screen.getByRole('button', { name: /mkyoqzmz/ }));
      await waitFor(() => {
        expect(screen.getByText('0/1 viewed')).toBeInTheDocument();
      });
      expect(screen.queryByText('updated')).not.toBeInTheDocument();
    } finally {
      getItemSpy.mockRestore();
    }
  });

  it('retains queried out-of-graph changes across commentsChanged SSE and approvals', async () => {
    let oldChangeApproved = false;
    let oldChangeUnresolved = 0;

    const makeOldChange = (): JjChange => ({
      changeId: 'oooooooo99999999',
      shortChangeId: 'oooooooo',
      shortestChangeId: 'oo',
      commitId: '99999999eeeeee',
      shortCommitId: '9999999',
      isWorkingCopy: false,
      timestamp: '2026-11-15 09:00:00',
      description: 'Archived change',
      commentCount: oldChangeUnresolved,
      unresolvedCommentCount: oldChangeUnresolved,
      approved: oldChangeApproved,
    });

    const oldDiff: DiffResponse = {
      commit: '99999999eeeeee',
      targetCommitish: '99999999eeeeee',
      files: [],
    };

    vi.mocked(global.fetch).mockImplementation(
      async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = typeof input === 'string' ? input : input.toString();
        if (url === '/api/user-settings') {
          return new Response(JSON.stringify({ client: {} }), { status: 200 });
        }
        if (url === '/api/jj/status') {
          return new Response(JSON.stringify({ isJj: true, repoRoot: '/workspace' }), {
            status: 200,
          });
        }
        if (url === '/api/jj/changes') {
          return new Response(JSON.stringify({ changes: sampleChanges, graphRows: [] }), {
            status: 200,
          });
        }
        if (
          url === '/api/jj/changes?query=oldrev' ||
          url === '/api/jj/changes?query=oooooooo99999999'
        ) {
          return new Response(JSON.stringify({ changes: [makeOldChange()], graphRows: [] }), {
            status: 200,
          });
        }
        if (url === '/api/jj/changes/ltppwqry22222222/commits') {
          return new Response(JSON.stringify({ commits: sampleCommits }), { status: 200 });
        }
        if (url === '/api/jj/changes/oooooooo99999999/commits') {
          return new Response(
            JSON.stringify({
              commits: [
                {
                  commitId: '99999999eeeeee',
                  shortCommitId: '9999999',
                  timestamp: '2026-11-15 09:00:00',
                  description: 'Archived change',
                  isWorkingCopy: false,
                  commentCount: oldChangeUnresolved,
                  unresolvedCommentCount: oldChangeUnresolved,
                  approved: oldChangeApproved,
                },
              ],
            }),
            { status: 200 },
          );
        }
        if (url.startsWith('/api/diff?') || url.startsWith('/api/jj/diff?target=335eb579f263bb')) {
          return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
        }
        if (url.startsWith('/api/jj/diff?target=99999999eeeeee')) {
          return new Response(JSON.stringify(oldDiff), { status: 200 });
        }
        if (url === '/api/jj/approve' && init?.method === 'POST') {
          const body = JSON.parse(String(init.body)) as { disapprove: boolean };
          oldChangeApproved = !body.disapprove;
          return new Response(JSON.stringify({ success: true, approved: oldChangeApproved }), {
            status: 200,
          });
        }
        return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
      },
    );

    render(App);

    await waitFor(() => {
      expect(screen.getAllByText('dprint.json').length).toBeGreaterThan(0);
    });

    await fireEvent.click(screen.getByRole('button', { name: 'Search revision' }));
    const queryInput = screen.getByLabelText('Jump to change ID or revset');
    await fireEvent.input(queryInput, { target: { value: 'oldrev' } });
    await fireEvent.submit(queryInput.closest('form')!);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: /oooooooo/ })).toHaveAttribute(
        'aria-current',
        'true',
      );
    });

    // SSE commentsChanged updates metadata while keeping the out-of-graph queried change
    oldChangeUnresolved = 1;
    const watchStream = MockEventSource.instances.find((s) => s.url === '/api/watch')!;
    watchStream.onmessage?.(
      new MessageEvent('message', {
        data: JSON.stringify({
          type: 'commentsChanged',
          version: 1,
          timestamp: '2026-11-16T21:05:00Z',
        }),
      }),
    );

    await waitFor(() => {
      expect(screen.getByTitle('1 unresolved comment')).toBeInTheDocument();
    });
    expect(screen.getByRole('button', { name: /oooooooo/ })).toHaveAttribute(
      'aria-current',
      'true',
    );

    // Approving the out-of-graph change keeps it in the tree and updates button state
    oldChangeUnresolved = 0;
    await fireEvent.click(screen.getByRole('button', { name: 'Approve' }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Approved' })).toBeInTheDocument();
    });
    expect(screen.getByRole('button', { name: /oooooooo/ })).toHaveAttribute(
      'aria-current',
      'true',
    );
  });

  it('renders inline .tellit/ threads on Commit Message and code lines, supports drag-comment, reply, resolve, drawer jump, expand, and open-in-editor', async () => {
    const calledUrls: string[] = [];
    let threads: DiffCommentThread[] = [
      {
        id: 't-commit-msg',
        filePath: 'Commit Message',
        createdAt: '2026-11-16T10:00:00.000Z',
        updatedAt: '2026-11-16T10:00:00.000Z',
        resolved: false,
        position: { side: 'new', line: 1 },
        messages: [
          {
            id: 't-commit-msg:0',
            body: 'Clarify commit summary',
            createdAt: '2026-11-16T10:00:00.000Z',
            updatedAt: '2026-11-16T10:00:00.000Z',
          },
        ],
      },
    ];

    const codeDiff: DiffResponse = {
      commit: '335eb579f263bb',
      baseCommitish: '335eb579f263bb^',
      targetCommitish: '335eb579f263bb',
      repositoryId: 'repo-1',
      openInEditorAvailable: true,
      files: [
        sampleJjDiff.files[0]!,
        {
          path: 'src/feature.ts',
          status: 'modified',
          additions: 2,
          deletions: 0,
          chunks: [
            {
              header: '@@ -3,1 +3,3 @@',
              oldStart: 3,
              oldLines: 1,
              newStart: 3,
              newLines: 3,
              lines: [
                { type: 'normal', content: 'const a = 1;', oldLineNumber: 3, newLineNumber: 3 },
                { type: 'add', content: 'const b = 2;', newLineNumber: 4 },
                { type: 'add', content: 'const c = 3;', newLineNumber: 5 },
              ],
            },
          ],
        },
      ],
    };

    vi.mocked(global.fetch).mockImplementation(
      async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = typeof input === 'string' ? input : input.toString();
        calledUrls.push(url);
        if (url === '/api/user-settings') {
          if (init?.method === 'PUT') {
            return new Response(JSON.stringify({ client: {} }), { status: 200 });
          }
          return new Response(JSON.stringify({ client: {} }), { status: 200 });
        }
        if (url === '/api/jj/status') {
          return new Response(JSON.stringify({ isJj: true, repoRoot: '/workspace' }), {
            status: 200,
          });
        }
        if (url === '/api/jj/changes') {
          return new Response(JSON.stringify({ changes: sampleChanges, graphRows: [] }), {
            status: 200,
          });
        }
        if (url === '/api/jj/changes/ltppwqry22222222/commits') {
          return new Response(JSON.stringify({ commits: sampleCommits }), { status: 200 });
        }
        if (url.startsWith('/api/diff?') || url.startsWith('/api/jj/diff?target=335eb579f263bb')) {
          return new Response(JSON.stringify(codeDiff), { status: 200 });
        }
        if (url.startsWith('/api/generated-status/')) {
          return new Response(
            JSON.stringify({
              path: 'src/feature.ts',
              ref: '335eb579f263bb',
              isGenerated: false,
              source: 'path',
            }),
            { status: 200 },
          );
        }
        if (url.startsWith('/api/line-count/src%2Ffeature.ts')) {
          return new Response(JSON.stringify({ oldLineCount: 4, newLineCount: 6 }), {
            status: 200,
          });
        }
        if (url.startsWith('/api/blob/src%2Ffeature.ts')) {
          return new Response('line 1\nline 2\nconst a = 1;\nline 4\n', { status: 200 });
        }
        if (url === '/api/jj/comments?commit=335eb579f263bb') {
          return new Response(JSON.stringify({ threads }), { status: 200 });
        }
        if (url === '/api/jj/comments' && init?.method === 'POST') {
          const body = JSON.parse(String(init.body)) as {
            filePath: string;
            line?: { start: number; end: number } | number;
            content: string;
          };
          threads = [
            ...threads,
            {
              id: 't-new-range',
              filePath: body.filePath,
              createdAt: '2026-11-16T10:05:00.000Z',
              updatedAt: '2026-11-16T10:05:00.000Z',
              resolved: false,
              position: { side: 'new', line: body.line ?? 1 },
              messages: [
                {
                  id: 't-new-range:0',
                  body: body.content,
                  createdAt: '2026-11-16T10:05:00.000Z',
                  updatedAt: '2026-11-16T10:05:00.000Z',
                },
              ],
            },
          ];
          return new Response(JSON.stringify({ success: true, threadId: 't-new-range' }), {
            status: 200,
          });
        }
        if (url === '/api/jj/comments/t-new-range/reply' && init?.method === 'POST') {
          const body = JSON.parse(String(init.body)) as { content: string };
          threads = threads.map((t) =>
            t.id === 't-new-range'
              ? {
                  ...t,
                  messages: [
                    ...t.messages,
                    {
                      id: 't-new-range:1',
                      body: body.content,
                      createdAt: '2026-11-16T10:06:00.000Z',
                      updatedAt: '2026-11-16T10:06:00.000Z',
                    },
                  ],
                }
              : t,
          );
          return new Response(JSON.stringify({ success: true }), { status: 200 });
        }
        if (url === '/api/jj/comments/t-new-range/resolve' && init?.method === 'POST') {
          threads = threads.map((t) => (t.id === 't-new-range' ? { ...t, resolved: true } : t));
          return new Response(JSON.stringify({ success: true, resolved: true }), { status: 200 });
        }
        if (url === '/api/open-in-editor' && init?.method === 'POST') {
          return new Response(JSON.stringify({ success: true }), { status: 200 });
        }
        return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
      },
    );

    const { container } = render(App);

    await waitFor(() => {
      expect(screen.getByText('Clarify commit summary')).toBeInTheDocument();
      expect(screen.getByText('1 open')).toBeInTheDocument();
    });

    // Switch to Unified mode
    await fireEvent.click(screen.getByRole('button', { name: 'Unified' }));
    expect(container.querySelector('[data-diff-mode="unified"]')).toBeInTheDocument();
    expect(localStorage.getItem('difit.diffViewMode')).toBe('unified');

    // Expand hidden lines above chunk 0 in src/feature.ts
    await fireEvent.click(screen.getByRole('button', { name: 'Expand all 2 hidden lines' }));
    await waitFor(() => {
      const expandedLine1Cell = container.querySelector(
        'article[data-file-path="src/feature.ts"] td[data-side="new"][data-line-number="1"]',
      );
      expect(expandedLine1Cell).toBeInTheDocument();
      expect(expandedLine1Cell?.closest('tr')?.textContent).toContain('line 1');
    });

    // Open in editor from line 4
    await fireEvent.click(
      screen.getByRole('button', { name: 'Open src/feature.ts line 4 in editor' }),
    );
    expect(calledUrls).toContain('/api/open-in-editor');

    // Drag across lines 4..5 in src/feature.ts and submit a range comment
    const line4Cell = container.querySelector('td[data-side="new"][data-line-number="4"]')!;
    const line5Cell = container.querySelector('td[data-side="new"][data-line-number="5"]')!;
    await fireEvent.mouseDown(line4Cell, { button: 0 });
    await fireEvent.mouseEnter(line5Cell);
    await fireEvent.mouseUp(window);

    const commentInput = screen.getByLabelText('Comment on Lines 4–5');
    await fireEvent.input(commentInput, { target: { value: 'Combine b and c' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Comment' }));

    await waitFor(() => {
      expect(screen.getByText('Combine b and c')).toBeInTheDocument();
      expect(screen.getByText('2 open')).toBeInTheDocument();
    });

    // Open Comments drawer, jump to the new thread, ensure unique inline DOM ids, reply, and resolve
    await fireEvent.click(screen.getByRole('button', { name: 'Toggle comments drawer' }));
    expect(screen.getByLabelText('Review comments drawer')).toBeInTheDocument();
    expect(document.querySelectorAll('[id="comment-thread-t-commit-msg"]')).toHaveLength(1);
    expect(document.querySelectorAll('[id="comment-thread-t-new-range"]')).toHaveLength(1);

    const jumpButtons = screen.getAllByRole('button', { name: 'Jump' });
    await fireEvent.click(jumpButtons[1]!);

    // Reply inside the inline thread card
    const replyTriggers = screen.getAllByRole('button', { name: 'Reply...' });
    await fireEvent.click(replyTriggers[replyTriggers.length - 1]!);
    await fireEvent.input(screen.getByLabelText('Reply to thread'), {
      target: { value: 'Combined cleanly' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Reply' }));

    await waitFor(() => {
      expect(screen.getAllByText('Combined cleanly').length).toBeGreaterThan(0);
    });

    // Resolve the thread
    const resolveButtons = screen.getAllByRole('button', { name: 'Resolve' });
    await fireEvent.click(resolveButtons[resolveButtons.length - 1]!);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Toggle comments drawer' }).textContent).toContain(
        '1 open',
      );
    });

    // Ensure no legacy /api/comments endpoints were called in Jujutsu mode
    const legacyCalls = calledUrls.filter(
      (url) => url === '/api/comments' || url.startsWith('/api/comments-json'),
    );
    expect(legacyCalls).toEqual([]);
  });

  it('supports whole-file comments, commentsChanged SSE refresh, and interdiff old-side comments', async () => {
    const postedComments: Array<{
      commitId: string;
      filePath: string;
      line?: number | { start: number; end: number };
      content: string;
    }> = [];
    let threads: DiffCommentThread[] = [];
    let nextThreadSeq = 1;

    const interDiff: DiffResponse = {
      commit: '335eb579f263bb',
      baseCommitish: '22222222222222',
      targetCommitish: '335eb579f263bb',
      repositoryId: 'repo-1',
      files: [
        {
          path: 'src/feature.ts',
          status: 'modified',
          additions: 1,
          deletions: 1,
          chunks: [
            {
              header: '@@ -1,2 +1,2 @@',
              oldStart: 1,
              oldLines: 2,
              newStart: 1,
              newLines: 2,
              lines: [
                { type: 'delete', content: 'const legacy = true;', oldLineNumber: 1 },
                { type: 'add', content: 'const legacy = false;', newLineNumber: 1 },
                { type: 'normal', content: 'export {};', oldLineNumber: 2, newLineNumber: 2 },
              ],
            },
          ],
        },
      ],
    };

    vi.mocked(global.fetch).mockImplementation(
      async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = typeof input === 'string' ? input : input.toString();
        if (url === '/api/user-settings') {
          return new Response(JSON.stringify({ client: {} }), { status: 200 });
        }
        if (url === '/api/jj/status') {
          return new Response(JSON.stringify({ isJj: true, repoRoot: '/workspace' }), {
            status: 200,
          });
        }
        if (url === '/api/jj/changes') {
          return new Response(JSON.stringify({ changes: sampleChanges, graphRows: [] }), {
            status: 200,
          });
        }
        if (url === '/api/jj/changes/ltppwqry22222222/commits') {
          return new Response(JSON.stringify({ commits: sampleCommits }), { status: 200 });
        }
        if (url.startsWith('/api/diff?') || url.startsWith('/api/jj/diff?')) {
          return new Response(JSON.stringify(interDiff), { status: 200 });
        }
        if (url.startsWith('/api/generated-status/')) {
          return new Response(
            JSON.stringify({
              path: 'src/feature.ts',
              ref: '335eb579f263bb',
              isGenerated: false,
              source: 'path',
            }),
            { status: 200 },
          );
        }
        if (url.startsWith('/api/line-count/')) {
          return new Response(JSON.stringify({ oldLineCount: 2, newLineCount: 2 }), {
            status: 200,
          });
        }
        if (url.startsWith('/api/jj/comments?')) {
          return new Response(JSON.stringify({ threads }), { status: 200 });
        }
        if (url === '/api/jj/comments' && init?.method === 'POST') {
          const body = JSON.parse(String(init.body)) as {
            commitId: string;
            filePath: string;
            line?: number | { start: number; end: number };
            content: string;
          };
          postedComments.push(body);
          const id = `t-${nextThreadSeq++}`;
          threads = [
            ...threads,
            {
              id,
              filePath: body.filePath,
              createdAt: '2026-11-16T10:10:00.000Z',
              updatedAt: '2026-11-16T10:10:00.000Z',
              resolved: false,
              position: {
                side: body.commitId === '22222222222222' ? 'old' : 'new',
                line: body.line ?? 1,
              },
              messages: [
                {
                  id: `${id}:0`,
                  body: body.content,
                  createdAt: '2026-11-16T10:10:00.000Z',
                  updatedAt: '2026-11-16T10:10:00.000Z',
                },
              ],
            },
          ];
          return new Response(JSON.stringify({ success: true, threadId: id }), { status: 200 });
        }
        return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
      },
    );

    const { container } = render(App);

    await waitFor(() => {
      expect(screen.getByText('src/feature.ts')).toBeInTheDocument();
    });

    // Switch base revision (From) to prior commit 22222222222222 for interdiff
    await fireEvent.click(screen.getByRole('button', { name: 'Diff base revision (From)' }));
    await fireEvent.click(screen.getByRole('button', { name: /2222222/ }));

    // Submit a whole-file comment from the file header (no line field in payload)
    await fireEvent.click(screen.getByRole('button', { name: 'Comment on src/feature.ts' }));
    await fireEvent.input(screen.getByRole('textbox', { name: 'Comment on src/feature.ts' }), {
      target: { value: 'Whole file looks solid' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Comment' }));

    await waitFor(() => {
      expect(screen.getByText('Whole file looks solid')).toBeInTheDocument();
    });
    expect(postedComments[0]).toEqual({
      commitId: '335eb579f263bb',
      filePath: 'src/feature.ts',
      content: 'Whole file looks solid',
    });

    // Post an old-side comment in interdiff mode; targets base commit 22222222222222
    const oldLine1Cell = container.querySelector('td[data-side="old"][data-line-number="1"]')!;
    await fireEvent.mouseDown(oldLine1Cell, { button: 0 });
    await fireEvent.mouseUp(window);
    await fireEvent.input(screen.getByLabelText('Comment on Line 1 (old)'), {
      target: { value: 'Why was legacy true before?' },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Comment' }));

    await waitFor(() => {
      expect(screen.getByText('Why was legacy true before?')).toBeInTheDocument();
    });
    expect(postedComments[1]).toEqual({
      commitId: '22222222222222',
      filePath: 'src/feature.ts',
      line: 1,
      content: 'Why was legacy true before?',
    });

    // Simulate external commentsChanged SSE push and verify background refresh
    threads = [
      ...threads,
      {
        id: 't-sse-added',
        filePath: 'src/feature.ts',
        createdAt: '2026-11-16T10:12:00.000Z',
        updatedAt: '2026-11-16T10:12:00.000Z',
        resolved: false,
        position: { side: 'new', line: 1 },
        messages: [
          {
            id: 't-sse-added:0',
            body: 'Injected via SSE watcher',
            createdAt: '2026-11-16T10:12:00.000Z',
            updatedAt: '2026-11-16T10:12:00.000Z',
          },
        ],
      },
    ];
    const watchStream = MockEventSource.instances.find((s) => s.url === '/api/watch')!;
    watchStream.onmessage?.({
      data: JSON.stringify({ type: 'commentsChanged' }),
    } as MessageEvent);
    await waitFor(() => {
      expect(screen.getByText('Injected via SSE watcher')).toBeInTheDocument();
    });
  });

  it('clears stale threads on jj change switch and copies resolved threads when Copy all is clicked', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });

    const resolvedThreads: DiffCommentThread[] = [
      {
        id: 't-resolved-1',
        filePath: 'dprint.json',
        createdAt: '2026-11-16T09:00:00.000Z',
        updatedAt: '2026-11-16T09:05:00.000Z',
        resolved: true,
        position: { side: 'new', line: 1 },
        messages: [
          {
            id: 't-resolved-1:0',
            body: 'Resolved note from ltppwqry',
            createdAt: '2026-11-16T09:00:00.000Z',
            updatedAt: '2026-11-16T09:05:00.000Z',
          },
        ],
      },
    ];

    const workingCopyCommits: JjCommit[] = [
      {
        commitId: '44235e975acae6',
        shortCommitId: '44235e9',
        timestamp: '2026-11-16 15:47:08',
        description: '(no description set)',
        isWorkingCopy: true,
        commentCount: 0,
        unresolvedCommentCount: 0,
      },
    ];
    const workingCopyDiff: DiffResponse = {
      commit: '44235e975acae6',
      baseCommitish: '335eb579f263bb',
      targetCommitish: '44235e975acae6',
      repositoryId: 'repo-1',
      files: [],
    };

    vi.mocked(global.fetch).mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
      if (url === '/api/user-settings') {
        return new Response(JSON.stringify({ client: {} }), { status: 200 });
      }
      if (url === '/api/jj/status') {
        return new Response(JSON.stringify({ isJj: true, repoRoot: '/workspace' }), {
          status: 200,
        });
      }
      if (url === '/api/jj/changes') {
        return new Response(JSON.stringify({ changes: sampleChanges, graphRows: [] }), {
          status: 200,
        });
      }
      if (url === '/api/jj/changes/ltppwqry22222222/commits') {
        return new Response(JSON.stringify({ commits: sampleCommits }), { status: 200 });
      }
      if (url === '/api/jj/changes/mkyoqzmz11111111/commits') {
        return new Response(JSON.stringify({ commits: workingCopyCommits }), { status: 200 });
      }
      if (url.startsWith('/api/diff?') || url.startsWith('/api/jj/diff?target=335eb579f263bb')) {
        return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
      }
      if (url.startsWith('/api/jj/diff?target=44235e975acae6')) {
        return new Response(JSON.stringify(workingCopyDiff), { status: 200 });
      }
      if (url.startsWith('/api/generated-status/')) {
        return new Response(
          JSON.stringify({
            path: 'dprint.json',
            ref: '335eb579f263bb',
            isGenerated: false,
            source: 'path',
          }),
          { status: 200 },
        );
      }
      if (url === '/api/jj/comments?commit=335eb579f263bb') {
        return new Response(JSON.stringify({ threads: resolvedThreads }), { status: 200 });
      }
      if (url === '/api/jj/comments?commit=44235e975acae6') {
        return new Response(JSON.stringify({ error: 'Comment service unavailable' }), {
          status: 500,
        });
      }
      return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
    });

    render(App);

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Toggle comments drawer' })).toBeInTheDocument();
    });

    // Open drawer and verify "Copy all" copies resolved thread when unresolvedCount === 0
    await fireEvent.click(screen.getByRole('button', { name: 'Toggle comments drawer' }));
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Copy all' })).toBeInTheDocument();
    });
    await fireEvent.click(screen.getByRole('button', { name: 'Copy all' }));
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(
        expect.stringContaining('Resolved note from ltppwqry'),
      );
    });

    // Switch to another change whose /api/jj/comments fails; stale threads must clear immediately
    await fireEvent.click(screen.getByRole('button', { name: /mkyoqzmz/ }));
    await waitFor(() => {
      expect(screen.getByText('No changes in this revision')).toBeInTheDocument();
      expect(screen.getByText('Comments (0)')).toBeInTheDocument();
    });
  });

  it('routes markdown and image viewers, executes keyboard shortcuts, and persists settings via /api/user-settings', async () => {
    const savedSettingsPayloads: Array<Record<string, unknown>> = [];
    const multiViewerDiff: DiffResponse = {
      commit: '335eb579f263bb',
      baseCommitish: '335eb579f263bb^',
      targetCommitish: '335eb579f263bb',
      repositoryId: 'repo-1',
      files: [
        {
          path: 'README.md',
          oldPath: 'README.md',
          status: 'modified',
          additions: 1,
          deletions: 0,
          chunks: [
            {
              header: '@@ -1,3 +1,4 @@',
              oldStart: 1,
              oldLines: 3,
              newStart: 1,
              newLines: 4,
              lines: [
                { type: 'normal', content: '---', oldLineNumber: 1, newLineNumber: 1 },
                { type: 'normal', content: 'title: Tellit', oldLineNumber: 2, newLineNumber: 2 },
                { type: 'normal', content: '---', oldLineNumber: 3, newLineNumber: 3 },
                { type: 'add', content: '# Updated Documentation', newLineNumber: 4 },
              ],
            },
          ],
        },
        {
          path: 'assets/logo.png',
          oldPath: 'assets/logo.png',
          status: 'modified',
          additions: 1,
          deletions: 1,
          chunks: [],
        },
        {
          path: 'src/util.ts',
          status: 'modified',
          additions: 1,
          deletions: 0,
          chunks: [
            {
              header: '@@ -1,1 +1,2 @@',
              oldStart: 1,
              oldLines: 1,
              newStart: 1,
              newLines: 2,
              lines: [
                { type: 'normal', content: 'const base = 1;', oldLineNumber: 1, newLineNumber: 1 },
                { type: 'add', content: 'export const next = 2;', newLineNumber: 2 },
              ],
            },
          ],
        },
      ],
    };

    const sampleThreads: DiffCommentThread[] = [
      {
        id: 't-unresolved',
        filePath: 'src/util.ts',
        createdAt: '2026-11-16T10:00:00.000Z',
        updatedAt: '2026-11-16T10:00:00.000Z',
        resolved: false,
        position: { side: 'new', line: 2 },
        messages: [
          {
            id: 't-unresolved:0',
            body: 'Check return value',
            createdAt: '2026-11-16T10:00:00.000Z',
            updatedAt: '2026-11-16T10:00:00.000Z',
          },
        ],
      },
    ];

    vi.mocked(global.fetch).mockImplementation(
      async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = typeof input === 'string' ? input : input.toString();
        if (url === '/api/user-settings') {
          if (init?.method === 'PUT') {
            savedSettingsPayloads.push(JSON.parse(String(init.body)) as Record<string, unknown>);
            return new Response(JSON.stringify({ ok: true }), { status: 200 });
          }
          return new Response(JSON.stringify({ client: {} }), { status: 200 });
        }
        if (url === '/api/jj/status') {
          return new Response(JSON.stringify({ isJj: true, repoRoot: '/workspace' }), {
            status: 200,
          });
        }
        if (url === '/api/jj/changes') {
          return new Response(JSON.stringify({ changes: sampleChanges, graphRows: [] }), {
            status: 200,
          });
        }
        if (url === '/api/jj/changes/ltppwqry22222222/commits') {
          return new Response(JSON.stringify({ commits: sampleCommits }), { status: 200 });
        }
        if (url.startsWith('/api/diff?') || url.startsWith('/api/jj/diff?')) {
          return new Response(JSON.stringify(multiViewerDiff), { status: 200 });
        }
        if (url.startsWith('/api/generated-status/')) {
          return new Response(
            JSON.stringify({
              path: 'src/util.ts',
              ref: '335eb579f263bb',
              isGenerated: false,
              source: 'path',
            }),
            { status: 200 },
          );
        }
        if (url.startsWith('/api/line-count/')) {
          return new Response(JSON.stringify({ oldLineCount: 3, newLineCount: 4 }), {
            status: 200,
          });
        }
        if (url.startsWith('/api/blob/README.md')) {
          return new Response('---\ntitle: Tellit\n---\n# Updated Documentation\n', {
            status: 200,
          });
        }
        if (url.startsWith('/api/blob/')) {
          return new Response(new Blob([new Uint8Array(64)]), { status: 200 });
        }
        if (url.startsWith('/api/jj/comments')) {
          return new Response(JSON.stringify({ threads: sampleThreads }), { status: 200 });
        }
        return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
      },
    );

    const { container } = render(App);

    await waitFor(() => {
      expect(screen.getByRole('tab', { name: 'Diff Preview' })).toBeInTheDocument();
      expect(screen.getByText('Modified Image')).toBeInTheDocument();
    });

    // Switch Markdown viewer to Diff Preview and verify rendered heading
    await fireEvent.click(screen.getByRole('tab', { name: 'Diff Preview' }));
    expect(screen.getByRole('heading', { name: 'Updated Documentation' })).toBeInTheDocument();

    // Switch back to Code tab so line cursor navigation targets README.md lines
    await fireEvent.click(screen.getByRole('tab', { name: 'Code' }));

    // Keyboard u/s switches between Unified and Split modes
    await fireEvent.keyDown(window, { key: 'u' });
    expect(container.querySelector('[data-diff-mode="unified"]')).toBeInTheDocument();
    await fireEvent.keyDown(window, { key: 's' });
    expect(container.querySelector('[data-diff-mode="split"]')).toBeInTheDocument();

    // Keyboard j moves cursor onto first line, n moves to next chunk, ] jumps to next file, N jumps to unresolved comment
    await fireEvent.keyDown(window, { key: 'j' });
    expect(container.querySelector('.keyboard-cursor')).not.toBeNull();
    await fireEvent.keyDown(window, { key: ']' });
    expect(
      container.querySelector('[data-file-path="assets/logo.png"] .keyboard-focused-file-header'),
    ).not.toBeNull();
    await fireEvent.keyDown(window, { key: 'N' });
    expect(
      container.querySelector('[data-file-path="src/util.ts"] .keyboard-focused-file-header'),
    ).not.toBeNull();

    // Keyboard c opens inline comment form on the focused cursor line, and typing does not fire shortcuts
    await fireEvent.keyDown(window, { key: 'c' });
    const commentBox = screen.getByLabelText('Comment on Line 2');
    expect(commentBox).toBeInTheDocument();
    await fireEvent.keyDown(commentBox, { key: 'u' });
    expect(container.querySelector('[data-diff-mode="split"]')).toBeInTheDocument();

    // Keyboard v toggles viewed on the focused file
    await fireEvent.keyDown(window, { key: 'v' });
    await waitFor(() => {
      expect(screen.getByText('1/3 viewed')).toBeInTheDocument();
    });

    // Keyboard ? opens HelpModal and Escape closes it
    await fireEvent.keyDown(window, { key: '?' });
    expect(screen.getByRole('dialog', { name: 'Keyboard Shortcuts' })).toBeInTheDocument();
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.queryByRole('dialog', { name: 'Keyboard Shortcuts' })).not.toBeInTheDocument();

    // Settings modal updates font size and autoViewedPatterns and persists via PUT /api/user-settings
    await fireEvent.click(screen.getByRole('button', { name: 'Open settings' }));
    expect(screen.getByRole('dialog', { name: 'Settings' })).toBeInTheDocument();
    const fontSizeSlider = screen.getByLabelText('Font Size');
    await fireEvent.input(fontSizeSlider, { target: { value: '16' } });
    expect(document.documentElement.style.getPropertyValue('--app-font-size')).toBe('16px');
    await fireEvent.click(screen.getByRole('button', { name: 'Close' }));

    await waitFor(
      () => {
        expect(savedSettingsPayloads.length).toBeGreaterThan(0);
      },
      { timeout: 1500 },
    );
  });

  it('handles added-file split cursors, persistent Shift+N progression, non-replaying c comments, ? toggle, and partial settings migration', async () => {
    localStorage.setItem(APPEARANCE_STORAGE_KEY, JSON.stringify({ theme: 'light' }));
    localStorage.setItem(
      'difit.editor',
      JSON.stringify({ id: 'textmate', command: 'mate', argsTemplate: '-l %line %file' }),
    );

    const addedDiff: DiffResponse = {
      commit: '335eb579f263bb',
      baseCommitish: '335eb579f263bb^',
      targetCommitish: '335eb579f263bb',
      repositoryId: 'repo-1',
      openInEditorAvailable: true,
      files: [
        {
          path: 'src/added.ts',
          status: 'added',
          additions: 2,
          deletions: 0,
          chunks: [
            {
              header: '@@ -0,0 +1,2 @@',
              oldStart: 0,
              oldLines: 0,
              newStart: 1,
              newLines: 2,
              lines: [
                { type: 'add', content: 'export const a = 1;', newLineNumber: 1 },
                { type: 'add', content: 'export const b = 2;', newLineNumber: 2 },
              ],
            },
          ],
        },
        {
          path: 'src/other.ts',
          status: 'modified',
          additions: 1,
          deletions: 0,
          chunks: [
            {
              header: '@@ -1,1 +1,2 @@',
              oldStart: 1,
              oldLines: 1,
              newStart: 1,
              newLines: 2,
              lines: [
                { type: 'normal', content: 'const k = 1;', oldLineNumber: 1, newLineNumber: 1 },
                { type: 'add', content: 'const m = 2;', newLineNumber: 2 },
              ],
            },
          ],
        },
      ],
    };

    const twoThreads: DiffCommentThread[] = [
      {
        id: 't-open-1',
        filePath: 'src/added.ts',
        createdAt: '2026-11-16T09:00:00.000Z',
        updatedAt: '2026-11-16T09:00:00.000Z',
        resolved: false,
        position: { side: 'new', line: 2 },
        messages: [
          {
            id: 't-open-1:0',
            body: 'First unresolved thread',
            createdAt: '2026-11-16T09:00:00.000Z',
            updatedAt: '2026-11-16T09:00:00.000Z',
          },
        ],
      },
      {
        id: 't-open-2',
        filePath: 'src/other.ts',
        createdAt: '2026-11-16T09:01:00.000Z',
        updatedAt: '2026-11-16T09:01:00.000Z',
        resolved: false,
        position: { side: 'new', line: 2 },
        messages: [
          {
            id: 't-open-2:0',
            body: 'Second unresolved thread',
            createdAt: '2026-11-16T09:01:00.000Z',
            updatedAt: '2026-11-16T09:01:00.000Z',
          },
        ],
      },
    ];

    vi.mocked(global.fetch).mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
      if (url === '/api/user-settings') {
        return new Response(JSON.stringify({ client: {} }), { status: 200 });
      }
      if (url === '/api/jj/status') {
        return new Response(JSON.stringify({ isJj: true, repoRoot: '/workspace' }), {
          status: 200,
        });
      }
      if (url === '/api/jj/changes') {
        return new Response(JSON.stringify({ changes: sampleChanges, graphRows: [] }), {
          status: 200,
        });
      }
      if (url === '/api/jj/changes/ltppwqry22222222/commits') {
        return new Response(JSON.stringify({ commits: sampleCommits }), { status: 200 });
      }
      if (url.startsWith('/api/diff?') || url.startsWith('/api/jj/diff?')) {
        return new Response(JSON.stringify(addedDiff), { status: 200 });
      }
      if (url.startsWith('/api/generated-status/')) {
        return new Response(
          JSON.stringify({
            path: 'src/added.ts',
            ref: '335eb579f263bb',
            isGenerated: false,
            source: 'path',
          }),
          { status: 200 },
        );
      }
      if (url.startsWith('/api/line-count/')) {
        return new Response(JSON.stringify({ oldLineCount: 1, newLineCount: 2 }), { status: 200 });
      }
      if (url.startsWith('/api/jj/comments')) {
        return new Response(JSON.stringify({ threads: twoThreads }), { status: 200 });
      }
      return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
    });

    const { container } = render(App);

    await waitFor(() => {
      expect(
        screen.getByRole('button', { name: 'Open src/added.ts line 1 in editor' }),
      ).toBeInTheDocument();
    });

    // Partial reviewit-appearance-settings preserved light theme AND migrated difit.editor preset
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    await fireEvent.click(screen.getByRole('button', { name: 'Open settings' }));
    await fireEvent.click(screen.getByRole('button', { name: 'System' }));
    expect((screen.getByLabelText('Command') as HTMLInputElement).value).toBe('mate');
    await fireEvent.click(screen.getByRole('button', { name: 'Close' }));

    // Split mode ] onto added file highlights right cell via fixSide
    await fireEvent.keyDown(window, { key: ']' });
    expect(container.querySelector('#file-0-chunk-0-line-0-right.keyboard-cursor')).not.toBeNull();

    // Press c to open CommentForm, cancel it, collapse & re-expand file -> form must NOT replay
    await fireEvent.keyDown(window, { key: 'c' });
    expect(screen.getByLabelText('Comment on Line 1')).toBeInTheDocument();
    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByLabelText('Comment on Line 1')).not.toBeInTheDocument();

    const addedToggle = container.querySelector(
      '[data-file-path="src/added.ts"] button[aria-expanded]',
    );
    expect(addedToggle).not.toBeNull();
    if (addedToggle) {
      await fireEvent.click(addedToggle);
      await fireEvent.click(addedToggle);
    }
    expect(screen.queryByLabelText('Comment on Line 1')).not.toBeInTheDocument();

    // Shift+N steps from thread 1 to thread 2 cleanly even after highlight timer expires
    vi.useFakeTimers();
    try {
      await fireEvent.keyDown(window, { key: 'N' });
      expect(container.querySelector('#comment-thread-t-open-1')).toHaveClass('ring-2');
      await vi.advanceTimersByTimeAsync(2100);
      expect(container.querySelector('#comment-thread-t-open-1')).not.toHaveClass('ring-2');
      await fireEvent.keyDown(window, { key: 'N' });
      expect(container.querySelector('#comment-thread-t-open-2')).toHaveClass('ring-2');
    } finally {
      vi.useRealTimers();
    }

    // Help modal toggles both open and closed via ?
    await fireEvent.keyDown(window, { key: '?' });
    expect(screen.getByRole('dialog', { name: 'Keyboard Shortcuts' })).toBeInTheDocument();
    await fireEvent.keyDown(window, { key: '?' });
    expect(screen.queryByRole('dialog', { name: 'Keyboard Shortcuts' })).not.toBeInTheDocument();
  });
});
