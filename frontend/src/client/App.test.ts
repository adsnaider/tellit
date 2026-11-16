import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { DiffResponse, JjChange } from '../types/diff';

import App from './App.svelte';
import { APPEARANCE_STORAGE_KEY } from './utils/appearanceTheme';

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

const sampleJjDiff: DiffResponse = {
  commit: '335eb579f263bb',
  baseCommitish: '335eb579f263bb^',
  targetCommitish: '335eb579f263bb',
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

describe('App.svelte shell', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
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
  });

  it('bootstraps jj status and changes, selects non-empty ancestor, and switches changes', async () => {
    const emptyDiff: DiffResponse = {
      commit: '44235e975acae6',
      targetCommitish: '44235e975acae6',
      files: [],
    };

    vi.mocked(global.fetch).mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
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
      if (url.startsWith('/api/diff?')) {
        return new Response(JSON.stringify(emptyDiff), { status: 200 });
      }
      if (url === '/api/jj/diff?target=335eb579f263bb') {
        return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
      }
      if (url === '/api/jj/diff?target=44235e975acae6') {
        return new Response(JSON.stringify(emptyDiff), { status: 200 });
      }
      return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
    });

    render(App);

    expect(screen.getByText('Loading diff...')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getAllByText('dprint.json').length).toBeGreaterThan(0);
    });

    expect(screen.getByText('Revisions (2)')).toBeInTheDocument();
    expect(screen.getByText('Files (2)')).toBeInTheDocument();
    expect(screen.getByText('+4')).toBeInTheDocument();

    // Switch to the empty working copy change in the sidebar
    const workingCopyButton = screen.getByRole('button', { name: /mkyoqzmz/ });
    await fireEvent.click(workingCopyButton);

    await waitFor(() => {
      expect(screen.getByText('No changes in this revision')).toBeInTheDocument();
    });
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

  it('looks up a revision via the sidebar query form and cancels superseded queries', async () => {
    const queriedChange: JjChange = {
      changeId: 'oskwwqxq33333333',
      shortChangeId: 'oskwwqxq',
      shortestChangeId: 'os',
      commitId: '63730a283667fc',
      shortCommitId: '63730a2',
      isWorkingCopy: false,
      isEmpty: false,
      hasDescription: true,
      timestamp: '2026-11-16 12:00:00',
      description: 'docs: add project README',
      commentCount: 0,
      unresolvedCommentCount: 0,
      approved: true,
    };

    const staleChange: JjChange = {
      changeId: 'zzzzzzzz44444444',
      shortChangeId: 'zzzzzzzz',
      commitId: '99999999999999',
      shortCommitId: '9999999',
      isWorkingCopy: false,
      isEmpty: false,
      hasDescription: true,
      timestamp: '2026-11-16 10:00:00',
      description: 'stale query result',
      commentCount: 0,
      unresolvedCommentCount: 0,
    };

    const readmeDiff: DiffResponse = {
      commit: '63730a283667fc',
      targetCommitish: '63730a283667fc',
      files: [
        {
          path: 'README.md',
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
                { type: 'add', content: '# tellit', newLineNumber: 1 },
                { type: 'add', content: 'Diff viewer', newLineNumber: 2 },
              ],
            },
          ],
        },
      ],
    };

    const emptyDiff: DiffResponse = {
      commit: '44235e975acae6',
      targetCommitish: '44235e975acae6',
      files: [],
    };

    let resolveSlowQuery: ((response: Response) => void) | undefined;

    vi.mocked(global.fetch).mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
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
      if (url === '/api/jj/changes?query=oskwwqxq') {
        return new Response(JSON.stringify({ changes: [queriedChange], graphRows: [] }), {
          status: 200,
        });
      }
      if (url.startsWith('/api/diff?')) {
        return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
      }
      if (url === '/api/jj/diff?target=335eb579f263bb') {
        return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
      }
      if (url === '/api/jj/diff?target=44235e975acae6') {
        return new Response(JSON.stringify(emptyDiff), { status: 200 });
      }
      if (url === '/api/jj/diff?target=63730a283667fc') {
        return new Response(JSON.stringify(readmeDiff), { status: 200 });
      }
      return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
    });

    render(App);

    await waitFor(() => {
      expect(screen.getByText('Revisions (2)')).toBeInTheDocument();
    });

    // Start a slow query, then supersede it by clicking a sidebar revision
    const queryInput = screen.getByLabelText('Jump to change ID or revset');
    await fireEvent.input(queryInput, { target: { value: 'slowrev' } });
    await fireEvent.submit(queryInput.closest('form')!);

    const workingCopyButton = screen.getByRole('button', { name: /mkyoqzmz/ });
    await fireEvent.click(workingCopyButton);

    await waitFor(() => {
      expect(screen.getByText('No changes in this revision')).toBeInTheDocument();
    });

    resolveSlowQuery?.(
      new Response(JSON.stringify({ changes: [staleChange], graphRows: [] }), { status: 200 }),
    );
    await Promise.resolve();
    expect(screen.queryByRole('button', { name: /zzzzzzzz/ })).not.toBeInTheDocument();
    expect(workingCopyButton).toHaveAttribute('aria-current', 'true');

    // Now submit a valid query that completes cleanly
    await fireEvent.input(queryInput, { target: { value: 'oskwwqxq' } });
    await fireEvent.submit(queryInput.closest('form')!);

    await waitFor(() => {
      expect(screen.getByText('+# tellit')).toBeInTheDocument();
    });
    expect(screen.getByText('Revisions (3)')).toBeInTheDocument();

    // Collapse the file card and ensure diff lines hide
    const fileHeaderToggle = screen.getByRole('button', { name: /▾\s*A\s*README\.md/ });
    await fireEvent.click(fileHeaderToggle);
    expect(screen.queryByText('+# tellit')).not.toBeInTheDocument();
  });

  it('keeps shell mounted and preserves selectedChangeId when switching changes fails', async () => {
    vi.mocked(global.fetch).mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
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
      if (url.startsWith('/api/diff?')) {
        return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
      }
      if (url === '/api/jj/diff?target=335eb579f263bb') {
        return new Response(JSON.stringify(sampleJjDiff), { status: 200 });
      }
      if (url === '/api/jj/diff?target=44235e975acae6') {
        return new Response(JSON.stringify({ error: 'Diff failed for revision' }), { status: 500 });
      }
      return new Response(JSON.stringify({ error: `Unexpected URL: ${url}` }), { status: 404 });
    });

    render(App);

    await waitFor(() => {
      expect(screen.getAllByText('dprint.json').length).toBeGreaterThan(0);
    });

    const initialChangeBtn = screen.getByRole('button', { name: /ltppwqry/ });
    const failingChangeBtn = screen.getByRole('button', { name: /mkyoqzmz/ });
    expect(initialChangeBtn).toHaveAttribute('aria-current', 'true');

    await fireEvent.click(failingChangeBtn);

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Diff failed for revision');
    });

    // Shell and existing diff remain mounted, and selectedChangeId stays on ltppwqry
    expect(screen.getByText('Revisions (2)')).toBeInTheDocument();
    expect(screen.getAllByText('dprint.json').length).toBeGreaterThan(0);
    expect(initialChangeBtn).toHaveAttribute('aria-current', 'true');
    expect(failingChangeBtn).not.toHaveAttribute('aria-current');
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

    expect(screen.queryByLabelText('Revisions')).not.toBeInTheDocument();
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

  it('displays initial API errors with a retry button', async () => {
    vi.mocked(global.fetch).mockImplementation(async (input: RequestInfo | URL) => {
      const url = typeof input === 'string' ? input : input.toString();
      if (url === '/api/jj/status') {
        return new Response(JSON.stringify({ isJj: false }), { status: 200 });
      }
      return new Response(JSON.stringify({ error: 'Repository unavailable' }), { status: 500 });
    });

    render(App);

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('Repository unavailable');
    });
  });
});
