import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { DiffResponse, JjChange, JjCommit } from '../types/diff';
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
});
