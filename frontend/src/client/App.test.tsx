import { render, screen, fireEvent, waitFor, act } from '@testing-library/react';
import { HotkeysProvider } from 'react-hotkeys-hook';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import '@testing-library/jest-dom';

import { mockFetch } from '../../vitest.setup';
import type { DiffCommentThread, DiffResponse } from '../types/diff';
import type { ClientWatchState } from '../types/watch';
import { DiffMode } from '../types/watch';

import App from './App';
import { useDiffComments } from './hooks/useDiffComments';
import { useViewedFiles } from './hooks/useViewedFiles';
import { useViewport } from './hooks/useViewport';

// Mock the useViewport hook
vi.mock('./hooks/useViewport', () => ({
  useViewport: vi.fn(() => ({ isMobile: false, isDesktop: true })),
}));

// Mock the useDiffComments hook
vi.mock('./hooks/useDiffComments', () => ({
  useDiffComments: vi.fn(() => ({
    hasLoadedComments: true,
    comments: [],
    threads: mockComments,
    replaceThreads: mockReplaceThreads,
    addComment: vi.fn(),
    addThread: vi.fn(),
    removeComment: vi.fn(),
    removeThread: vi.fn(),
    removeMessage: vi.fn(),
    replyToThread: vi.fn(),
    updateComment: vi.fn(),
    updateMessage: vi.fn(),
    clearAllComments: mockClearAllComments,
    applyCommentImports: mockApplyCommentImports,
    generatePrompt: vi.fn(),
    generateThreadPrompt: vi.fn(),
    generateAllCommentsPrompt: mockGenerateAllCommentsPrompt,
  })),
}));

// Mock the useViewedFiles hook
const mockClearViewedFiles = vi.fn();
const mockToggleFileViewed = vi.fn();
let mockViewedFiles = new Set<string>();
let mockHasLoadedInitialViewedFiles = true;
vi.mock('./hooks/useViewedFiles', () => ({
  useViewedFiles: vi.fn(() => ({
    viewedFiles: mockViewedFiles,
    changedSinceViewedFiles: new Set<string>(),
    hasLoadedInitialViewedFiles: mockHasLoadedInitialViewedFiles,
    toggleFileViewed: mockToggleFileViewed,
    isFileContentChanged: vi.fn(),
    getViewedFileRecord: vi.fn(),
    clearViewedFiles: mockClearViewedFiles,
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
  useFileWatch: vi.fn((onReload?: () => Promise<void>) => ({
    shouldReload: mockWatchState.shouldReload,
    isConnected: true,
    error: null,
    reload: vi.fn(async () => {
      if (onReload) {
        await onReload();
      }
      mockWatchState.shouldReload = false;
      mockWatchState.lastChangeType = null;
    }),
    watchState: mockWatchState,
  })),
}));

// Mock navigator.sendBeacon
Object.defineProperty(navigator, 'sendBeacon', {
  writable: true,
  value: vi.fn(),
});

// Mock window.confirm
const mockConfirm = vi.fn();
Object.defineProperty(window, 'confirm', {
  writable: true,
  value: mockConfirm,
});

// Mock EventSource
class MockEventSource {
  static instances: MockEventSource[] = [];
  onopen: (() => void) | null = null;
  onerror: ((err: any) => void) | null = null;
  close = vi.fn();

  constructor(url: string) {
    this.url = url;
    MockEventSource.instances.push(this);
  }

  url: string;

  static clearInstances() {
    MockEventSource.instances = [];
  }
}
Object.defineProperty(window, 'EventSource', {
  writable: true,
  value: MockEventSource,
});

let mockComments: DiffCommentThread[] = [];
const mockReplaceThreads = vi.fn();
const mockClearAllComments = vi.fn();
const mockApplyCommentImports = vi.fn(() => []);
const mockGenerateAllCommentsPrompt = vi.fn(() => 'formatted prompt');

function createMockThread({
  id,
  filePath,
  line,
  body,
  author = 'User',
}: {
  id: string;
  filePath: string;
  line: number;
  body: string;
  author?: string;
}): DiffCommentThread {
  const timestamp = '2024-01-01T00:00:00.000Z';
  return {
    id,
    filePath,
    createdAt: timestamp,
    updatedAt: timestamp,
    position: { side: 'new', line },
    messages: [
      {
        id,
        body,
        author,
        createdAt: timestamp,
        updatedAt: timestamp,
      },
    ],
  };
}

// Helper to render App with HotkeysProvider
const renderApp = () => {
  return render(
    <HotkeysProvider initiallyActiveScopes={['navigation']}>
      <App />
    </HotkeysProvider>,
  );
};

beforeEach(() => {
  window.localStorage.clear();
  vi.unstubAllEnvs();
  MockEventSource.clearInstances();
  mockViewedFiles = new Set<string>();
  mockHasLoadedInitialViewedFiles = true;
  mockReplaceThreads.mockReset();
  mockGenerateAllCommentsPrompt.mockClear();
});

const mockDiffResponse: DiffResponse = {
  commit: 'abc123',
  baseCommitish: 'HEAD^',
  targetCommitish: 'HEAD',
  requestedBaseCommitish: 'HEAD^',
  requestedTargetCommitish: 'HEAD',
  files: [
    {
      path: 'test.ts',
      status: 'modified',
      additions: 5,
      deletions: 2,
      chunks: [],
    },
  ],
  ignoreWhitespace: false,
  isEmpty: false,
};

describe('App Component - Clear Comments Functionality', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockComments = [];
    mockApplyCommentImports.mockReset();
    mockApplyCommentImports.mockReturnValue([]);
    mockConfirm.mockReturnValue(false);
    mockFetch(mockDiffResponse);
  });

  describe('Copy All Prompt Button', () => {
    it('should generate Copy All Prompt with requested and resolved diff context', async () => {
      mockComments = [
        createMockThread({ id: 'test-1', filePath: 'test.ts', line: 10, body: 'Test comment' }),
      ];
      mockFetch({
        ...mockDiffResponse,
        baseCommitish: 'abcdef1',
        targetCommitish: '1234567',
        requestedBaseCommitish: 'main',
        requestedTargetCommitish: 'feature/docs-update',
        requestedBaseMode: 'merge-base',
      });

      renderApp();

      fireEvent.click(await screen.findByText(/Copy All Prompt/));

      await waitFor(() => {
        expect(mockGenerateAllCommentsPrompt).toHaveBeenCalledWith({
          requestedBaseCommitish: 'main',
          requestedTargetCommitish: 'feature/docs-update',
          baseMode: 'merge-base',
          resolvedBaseCommitish: 'abcdef1',
          resolvedTargetCommitish: '1234567',
        });
      });
    });
  });

  describe('Cleanup All Prompt Button', () => {
    it('should not show delete button when no comments exist', async () => {
      mockComments = [];

      renderApp();

      await waitFor(() => {
        // Cleanup All Prompt should not be visible without comments (dropdown doesn't exist)
        expect(screen.queryByText('Copy All Prompt')).not.toBeInTheDocument();
        expect(screen.queryByText('Cleanup All Prompt')).not.toBeInTheDocument();
      });
    });

    it('should show delete button when comments exist', async () => {
      mockComments = [
        createMockThread({ id: 'test-1', filePath: 'test.ts', line: 10, body: 'Test comment' }),
      ];

      renderApp();

      await waitFor(() => {
        // Find and click the dropdown toggle button (chevron)
        const dropdownToggle = screen.getByTitle('More options');
        fireEvent.click(dropdownToggle);
      });

      await waitFor(() => {
        expect(screen.getByText('Cleanup All Prompt')).toBeInTheDocument();
      });
    });

    it('should call clearAllComments immediately when delete button is clicked', async () => {
      mockComments = [
        createMockThread({ id: '1', filePath: 'test.ts', line: 10, body: 'Comment 1' }),
        createMockThread({ id: '2', filePath: 'test.ts', line: 20, body: 'Comment 2' }),
      ];

      renderApp();

      await waitFor(() => {
        // First, open the dropdown
        const dropdownToggle = screen.getByTitle('More options');
        fireEvent.click(dropdownToggle);
      });

      await waitFor(() => {
        const deleteButton = screen.getByText('Cleanup All Prompt');
        fireEvent.click(deleteButton);
      });

      expect(mockClearAllComments).toHaveBeenCalled();
    });
  });

  describe('Clean flag on Startup', () => {
    it('should clear existing comments when clearComments flag is true in response', async () => {
      const responseWithClearFlag: DiffResponse = {
        ...mockDiffResponse,
        clearComments: true,
      };

      mockFetch(responseWithClearFlag);

      renderApp();

      await waitFor(() => {
        expect(mockClearAllComments).toHaveBeenCalledWith({
          resetAppliedCommentImportIds: true,
        });
      });
    });

    it('should clear viewed files when clearComments flag is true in response', async () => {
      const responseWithClearFlag: DiffResponse = {
        ...mockDiffResponse,
        clearComments: true,
      };

      mockFetch(responseWithClearFlag);

      renderApp();

      await waitFor(() => {
        expect(mockClearViewedFiles).toHaveBeenCalled();
      });
    });

    it('should not clear comments when clearComments flag is false', async () => {
      const responseWithoutClearFlag: DiffResponse = {
        ...mockDiffResponse,
        clearComments: false,
      };

      mockFetch(responseWithoutClearFlag);

      renderApp();

      await waitFor(() => {
        expect(mockClearAllComments).not.toHaveBeenCalled();
      });
    });

    it('should not clear comments when clearComments flag is undefined', async () => {
      const responseWithoutFlag: DiffResponse = {
        ...mockDiffResponse,
        // clearComments is undefined
      };

      mockFetch(responseWithoutFlag);

      renderApp();

      await waitFor(() => {
        expect(mockClearAllComments).not.toHaveBeenCalled();
      });
    });

    it('should log message when clearing comments via CLI flag', async () => {
      const consoleLogSpy = vi.spyOn(console, 'log').mockImplementation(() => {});

      const responseWithClearFlag: DiffResponse = {
        ...mockDiffResponse,
        clearComments: true,
      };

      mockFetch(responseWithClearFlag);

      renderApp();

      await waitFor(() => {
        expect(consoleLogSpy).toHaveBeenCalledWith(
          '✅ All existing comments and viewed files cleared as requested via --clean flag',
        );
      });

      consoleLogSpy.mockRestore();
    });

    it('hydrates comments from the server comment session on startup', async () => {
      const serverThreads = [
        createMockThread({
          id: 'imported-thread',
          filePath: 'test.ts',
          line: 10,
          body: 'Imported comment',
        }),
      ];

      vi.mocked(global.fetch).mockImplementation((input) => {
        const url = String(input);

        if (url.startsWith('/api/comments-json')) {
          return Promise.resolve({
            ok: true,
            json: async () => ({ threads: serverThreads }),
          } as Response);
        }

        if (url.startsWith('/api/comments')) {
          return Promise.resolve({
            ok: true,
            json: async () => ({ success: true }),
          } as Response);
        }

        if (url === '/api/revisions') {
          return Promise.resolve({
            ok: true,
            json: async () => null,
          } as Response);
        }

        return Promise.resolve({
          ok: true,
          json: async () => mockDiffResponse,
          blob: async () => ({ size: 1024 }),
        } as Response);
      });

      renderApp();

      await waitFor(() => {
        expect(mockReplaceThreads).toHaveBeenCalledWith(serverThreads);
      });

      expect(vi.mocked(global.fetch)).toHaveBeenCalledWith(
        '/api/comments-json?base=HEAD%5E&target=HEAD',
      );
    });

    it('preserves server-provided comments after clearing local comments on startup', async () => {
      mockComments = [
        createMockThread({
          id: 'stale-local-thread',
          filePath: 'test.ts',
          line: 5,
          body: 'Stale local comment',
        }),
      ];
      const serverThreads = [
        createMockThread({
          id: 'imported-thread',
          filePath: 'test.ts',
          line: 10,
          body: 'Imported comment',
        }),
      ];
      const responseWithClearFlag: DiffResponse = {
        ...mockDiffResponse,
        clearComments: true,
      };

      vi.mocked(global.fetch).mockImplementation((input) => {
        const url = String(input);

        if (url.startsWith('/api/comments-json')) {
          return Promise.resolve({
            ok: true,
            json: async () => ({ threads: serverThreads }),
          } as Response);
        }

        if (url.startsWith('/api/comments')) {
          return Promise.resolve({
            ok: true,
            json: async () => ({ success: true }),
          } as Response);
        }

        if (url === '/api/revisions') {
          return Promise.resolve({
            ok: true,
            json: async () => null,
          } as Response);
        }

        return Promise.resolve({
          ok: true,
          json: async () => responseWithClearFlag,
          blob: async () => ({ size: 1024 }),
        } as Response);
      });

      renderApp();

      await waitFor(() => {
        expect(mockClearAllComments).toHaveBeenCalledWith({
          resetAppliedCommentImportIds: true,
        });
      });

      await waitFor(() => {
        expect(mockReplaceThreads).toHaveBeenCalledWith(serverThreads);
      });

      expect(mockReplaceThreads).not.toHaveBeenCalledWith([...serverThreads, ...mockComments]);
    });
  });
});

describe('App Component - Heartbeat Connection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockComments = [];
    mockConfirm.mockReturnValue(false);
    mockFetch(mockDiffResponse);
  });

  it('uses the direct API url for heartbeat when configured in development', async () => {
    vi.stubEnv('VITE_DIFIT_API_URL', 'http://localhost:4969');

    renderApp();

    await waitFor(() => {
      expect(MockEventSource.instances[0]?.url).toBe('http://localhost:4969/api/heartbeat');
    });
  });
});

describe('App Component - Initial file collapsing', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockComments = [];
    mockConfirm.mockReturnValue(false);
    mockFetch(mockDiffResponse);
    mockViewedFiles = new Set<string>();
    mockHasLoadedInitialViewedFiles = false;
  });

  it('collapses initially viewed files after viewed state finishes loading', async () => {
    const view = renderApp();

    await waitFor(() => {
      expect(screen.getByTitle('Collapse file (Alt+Click to collapse all)')).toBeInTheDocument();
    });

    expect(screen.getByTitle('Collapse file (Alt+Click to collapse all)')).toBeInTheDocument();

    act(() => {
      mockViewedFiles = new Set(['test.ts']);
      mockHasLoadedInitialViewedFiles = true;
      view.rerender(
        <HotkeysProvider initiallyActiveScopes={['navigation']}>
          <App />
        </HotkeysProvider>,
      );
    });

    await waitFor(() => {
      expect(screen.getByTitle('Expand file (Alt+Click to expand all)')).toBeInTheDocument();
    });
  });
});

describe('App Component - Comment sync', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockConfirm.mockReturnValue(false);
    mockFetch(mockDiffResponse);
  });

  it('syncs an empty comment list after the last comment is resolved', async () => {
    mockComments = [
      createMockThread({ id: 'test-1', filePath: 'test.ts', line: 10, body: 'Test comment' }),
    ];

    const mockGlobalFetch = vi.mocked(global.fetch);
    const { rerender } = renderApp();

    await waitFor(() => {
      const commentCalls = mockGlobalFetch.mock.calls.filter(([url]) =>
        String(url).startsWith('/api/comments?'),
      );
      expect(commentCalls).toHaveLength(1);

      const [url, request] = commentCalls[0] as [string, RequestInit];
      expect(url).toBe('/api/comments?base=HEAD%5E&target=HEAD');
      expect(request.method).toBe('POST');
      expect(JSON.parse(String(request.body))).toEqual({
        threads: [
          expect.objectContaining({
            id: 'test-1',
            filePath: 'test.ts',
            position: { side: 'new', line: 10 },
            messages: [
              expect.objectContaining({
                id: 'test-1',
                body: 'Test comment',
                author: 'User',
              }),
            ],
          }),
        ],
      });
    });

    mockComments = [];
    rerender(
      <HotkeysProvider initiallyActiveScopes={['navigation']}>
        <App />
      </HotkeysProvider>,
    );

    await waitFor(() => {
      const commentCalls = mockGlobalFetch.mock.calls.filter(([url]) =>
        String(url).startsWith('/api/comments?'),
      );
      expect(commentCalls).toHaveLength(2);

      const [url, request] = commentCalls[1] as [string, RequestInit];
      expect(url).toBe('/api/comments?base=HEAD%5E&target=HEAD');
      expect(request.method).toBe('POST');
      expect(JSON.parse(String(request.body))).toEqual({ threads: [] });
    });
  });

  it('sends an empty comment list on unload when no comments remain', async () => {
    mockComments = [];
    const addEventListenerSpy = vi.spyOn(window, 'addEventListener');

    renderApp();

    await waitFor(() => {
      expect(addEventListenerSpy).toHaveBeenCalledWith('beforeunload', expect.any(Function));
    });

    const beforeUnloadHandler = addEventListenerSpy.mock.calls.find(
      ([eventName]) => eventName === 'beforeunload',
    )?.[1] as (() => void) | undefined;
    expect(beforeUnloadHandler).toBeDefined();
    beforeUnloadHandler?.();

    expect(navigator.sendBeacon).toHaveBeenCalledWith(
      '/api/comments?base=HEAD%5E&target=HEAD',
      JSON.stringify({ threads: [] }),
    );
    addEventListenerSpy.mockRestore();
  });

  it('shows author badges in the comments modal when the diff has multiple authors', async () => {
    mockComments = [
      createMockThread({ id: 'test-1', filePath: 'test.ts', line: 10, body: 'User comment' }),
      createMockThread({
        id: 'test-2',
        filePath: 'other.ts',
        line: 20,
        body: 'Reviewer comment',
        author: 'Reviewer',
      }),
    ];
    mockFetch({
      ...mockDiffResponse,
      files: [
        ...mockDiffResponse.files,
        {
          path: 'other.ts',
          status: 'modified',
          additions: 1,
          deletions: 1,
          chunks: [],
        },
      ],
    });

    renderApp();

    fireEvent.click(await screen.findByTitle('More options'));
    fireEvent.click(await screen.findByText('View All Comments'));

    expect(await screen.findByText('User')).toBeInTheDocument();
    expect(screen.getByText('Reviewer')).toBeInTheDocument();
  });
});

describe('App Component - Diff Mode Persistence', () => {
  it('initializes the selected view mode from localStorage', async () => {
    mockFetch(mockDiffResponse);
    window.localStorage.setItem('difit.diffViewMode', 'unified');

    renderApp();

    const unifiedButton = await screen.findByRole('button', { name: 'Unified' });

    await waitFor(() => {
      expect(unifiedButton).toHaveClass('bg-github-bg-primary');
    });
  });

  it('persists the selected view mode to localStorage', async () => {
    mockFetch(mockDiffResponse);

    renderApp();

    const unifiedButton = await screen.findByRole('button', { name: 'Unified' });
    fireEvent.click(unifiedButton);

    expect(window.localStorage.getItem('difit.diffViewMode')).toBe('unified');
  });

  it('keeps the selected view mode after triggering refresh', async () => {
    const mockGlobalFetch = vi.mocked(global.fetch);
    mockGlobalFetch.mockClear();
    mockComments = [];
    mockClearAllComments.mockReset();
    mockConfirm.mockReturnValue(false);
    mockWatchState.shouldReload = true;
    mockWatchState.lastChangeType = 'file';
    mockFetch(mockDiffResponse);

    renderApp();

    const unifiedButton = await screen.findByRole('button', { name: 'Unified' });
    fireEvent.click(unifiedButton);

    await waitFor(() => {
      expect(unifiedButton).toHaveClass('bg-github-bg-primary');
    });

    const refreshButton = await screen.findByRole('button', { name: 'Refresh' });
    fireEvent.click(refreshButton);

    await waitFor(() => {
      // 4 calls: initial /api/diff, /api/revisions, initial /api/comments sync, and refresh /api/diff
      expect(mockGlobalFetch).toHaveBeenCalledTimes(4);
    });

    await waitFor(() => {
      expect(unifiedButton).toHaveClass('bg-github-bg-primary');
    });
    mockWatchState.shouldReload = false;
    mockWatchState.lastChangeType = null;
  });
});

describe('App Component - Merge-base selection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockComments = [];
    mockConfirm.mockReturnValue(false);
  });

  it('clears the resolved base revision after switching to a merge-base quick diff', async () => {
    const initialDiffResponse: DiffResponse = {
      ...mockDiffResponse,
      baseCommitish: '88aabb0',
      targetCommitish: '.',
      requestedBaseCommitish: 'HEAD',
      requestedTargetCommitish: '.',
    };
    const mergeBaseDiffResponse: DiffResponse = {
      ...mockDiffResponse,
      baseCommitish: '1122334',
      targetCommitish: '.',
      requestedBaseCommitish: 'origin/main',
      requestedTargetCommitish: '.',
      requestedBaseMode: 'merge-base',
    };
    const revisionsResponse = {
      specialOptions: [{ value: '.', label: 'All Uncommitted Changes' }],
      branches: [],
      commits: [
        {
          hash: '88aabb0fffff1111222233334444555566667777',
          shortHash: '88aabb0',
          message: 'stale direct base',
        },
        {
          hash: '1122334fffff1111222233334444555566667777',
          shortHash: '1122334',
          message: 'merge base',
        },
      ],
      originDefaultBranch: 'origin/main',
    };

    vi.mocked(global.fetch).mockImplementation((input) => {
      const url = String(input);

      if (url.includes('/api/revisions')) {
        return Promise.resolve({
          ok: true,
          json: async () => revisionsResponse,
        } as Response);
      }

      if (url.includes('/api/diff')) {
        const response =
          url.includes('base=origin%2Fmain') && url.includes('baseMode=merge-base')
            ? mergeBaseDiffResponse
            : initialDiffResponse;

        return Promise.resolve({
          ok: true,
          json: async () => response,
          blob: async () => ({ size: 1024 }),
        } as Response);
      }

      return Promise.resolve({
        ok: true,
        json: async () => ({}),
      } as Response);
    });

    renderApp();

    fireEvent.click(await screen.findByRole('button', { name: /Revision menu:/ }));
    fireEvent.click(
      await screen.findByRole('button', { name: 'origin/main...Uncommitted (merge-base)' }),
    );

    await waitFor(() => {
      expect(
        screen.getByRole('button', {
          name: 'Revision menu: origin/main...Uncommitted Changes (merge-base)',
        }),
      ).toBeInTheDocument();
    });

    expect(
      screen.queryByRole('button', {
        name: 'Revision menu: 88aabb0...Uncommitted Changes (merge-base)',
      }),
    ).not.toBeInTheDocument();
  });

  it('uses resolved revisions for persisted diff state identity', async () => {
    const response: DiffResponse = {
      ...mockDiffResponse,
      baseCommitish: '1234567',
      targetCommitish: '98664e1',
      requestedBaseCommitish: '98664e1^',
      requestedTargetCommitish: '98664e1',
    };

    mockFetch(response);

    renderApp();

    await waitFor(() => {
      expect(vi.mocked(useDiffComments)).toHaveBeenCalledWith(
        '1234567',
        '98664e1',
        'abc123',
        undefined,
        undefined,
        undefined,
      );
    });

    expect(vi.mocked(useViewedFiles)).toHaveBeenCalledWith(
      '1234567',
      '98664e1',
      'abc123',
      undefined,
      response.files,
      undefined,
      [],
      undefined,
    );
  });

  it('ignores stale resolvedBase from /api/revisions on initial merge-base load', async () => {
    const mergeBaseDiffResponse: DiffResponse = {
      ...mockDiffResponse,
      baseCommitish: '1122334',
      targetCommitish: '.',
      requestedBaseCommitish: 'origin/main',
      requestedTargetCommitish: '.',
      requestedBaseMode: 'merge-base',
    };
    const revisionsResponse = {
      specialOptions: [{ value: '.', label: 'All Uncommitted Changes' }],
      branches: [],
      commits: [
        {
          hash: '88aabb0fffff1111222233334444555566667777',
          shortHash: '88aabb0',
          message: 'stale direct base',
        },
      ],
      originDefaultBranch: 'origin/main',
      resolvedBase: '88aabb0',
      resolvedTarget: '1122334',
    };

    let resolveRevisions: (() => void) | null = null;

    vi.mocked(global.fetch).mockImplementation((input) => {
      const url = String(input);

      if (url.includes('/api/revisions')) {
        return new Promise<Response>((resolve) => {
          resolveRevisions = () =>
            resolve({
              ok: true,
              json: async () => revisionsResponse,
            } as Response);
        });
      }

      if (url.includes('/api/diff')) {
        return Promise.resolve({
          ok: true,
          json: async () => mergeBaseDiffResponse,
          blob: async () => ({ size: 1024 }),
        } as Response);
      }

      return Promise.resolve({
        ok: true,
        json: async () => ({}),
      } as Response);
    });

    renderApp();

    await waitFor(() => {
      expect(screen.getByText('Reviewing:')).toBeInTheDocument();
    });

    await act(async () => {
      resolveRevisions?.();
    });

    await waitFor(() => {
      expect(
        screen.getByRole('button', {
          name: 'Revision menu: origin/main...Uncommitted Changes (merge-base)',
        }),
      ).toBeInTheDocument();
    });

    expect(
      screen.queryByRole('button', {
        name: 'Revision menu: 88aabb0...Uncommitted Changes (merge-base)',
      }),
    ).not.toBeInTheDocument();
  });
});

describe('App Component - Revision-aware refetching', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockComments = [];
    mockConfirm.mockReturnValue(false);
  });

  it('keeps the selected revisions when refetching without explicit revision params', async () => {
    const diffResponse: DiffResponse = {
      ...mockDiffResponse,
      requestedBaseCommitish: 'HEAD^',
      requestedTargetCommitish: 'HEAD',
    };

    vi.mocked(global.fetch).mockImplementation((input: string | URL | Request) => {
      const url =
        typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url;

      if (url.includes('/api/revisions')) {
        return Promise.resolve({
          ok: true,
          json: async () => ({
            specialOptions: [],
            branches: [],
            commits: [
              {
                hash: 'abc1234',
                shortHash: 'abc1234',
                message: 'Test commit',
              },
            ],
          }),
        } as Response);
      }

      if (url.startsWith('/api/comments?')) {
        return Promise.resolve({
          ok: true,
          json: async () => ({ success: true }),
        } as Response);
      }

      return Promise.resolve({
        ok: true,
        json: async () => diffResponse,
        blob: async () => ({ size: 1024 }),
      } as Response);
    });

    renderApp();

    fireEvent.click(await screen.findByRole('button', { name: /Revision menu:/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Previous commit' }));

    await waitFor(() => {
      const diffCalls = vi
        .mocked(global.fetch)
        .mock.calls.filter(([url]) => typeof url === 'string' && url.startsWith('/api/diff'));
      expect(diffCalls).toHaveLength(2);
      expect(String(diffCalls[1]?.[0])).toContain('base=HEAD%5E%5E');
      expect(String(diffCalls[1]?.[0])).toContain('target=HEAD%5E');
    });

    fireEvent.click(screen.getByRole('checkbox', { name: 'Ignore Whitespace' }));

    await waitFor(() => {
      const diffCalls = vi
        .mocked(global.fetch)
        .mock.calls.filter(([url]) => typeof url === 'string' && url.startsWith('/api/diff'));
      expect(diffCalls).toHaveLength(3);
      expect(String(diffCalls[2]?.[0])).toContain('base=HEAD%5E%5E');
      expect(String(diffCalls[2]?.[0])).toContain('target=HEAD%5E');
    });
  });
});

describe('App Component - Sidebar persistence', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockComments = [];
    mockConfirm.mockReturnValue(false);
    vi.mocked(useViewport).mockReturnValue({ isMobile: false, isDesktop: true });
    mockFetch(mockDiffResponse);
  });

  it('restores file tree open state from localStorage', async () => {
    window.localStorage.setItem('difit.sidebarOpen', 'false');

    renderApp();

    const toggleButton = await screen.findByRole('button', { name: /toggle file tree panel/i });
    expect(toggleButton).toHaveAttribute('aria-expanded', 'false');
  });

  it('persists file tree open state when toggled', async () => {
    renderApp();

    const toggleButton = await screen.findByRole('button', { name: /toggle file tree panel/i });

    fireEvent.click(toggleButton);
    await waitFor(() => {
      expect(window.localStorage.getItem('difit.sidebarOpen')).toBe('false');
    });

    fireEvent.click(toggleButton);
    await waitFor(() => {
      expect(window.localStorage.getItem('difit.sidebarOpen')).toBe('true');
    });
  });
});

describe('App Component - Mobile sidebar auto-close', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockComments = [];
    mockConfirm.mockReturnValue(false);
    vi.mocked(useViewport).mockReturnValue({ isMobile: true, isDesktop: false });
  });

  afterEach(() => {
    vi.mocked(useViewport).mockReturnValue({ isMobile: false, isDesktop: true });
  });

  it('closes the sidebar when a file is selected on mobile', async () => {
    mockFetch(mockDiffResponse);
    renderApp();

    // Sidebar toggle button
    const toggleButton = await screen.findByRole('button', { name: /toggle file tree panel/i });
    expect(toggleButton).toHaveAttribute('aria-expanded', 'true');

    // Wait for file list to render, then click the file row
    const fileRow = await screen.findByTitle('test.ts');
    fireEvent.click(fileRow.closest('[data-file-row]')!);

    // Sidebar should now be closed on mobile
    await waitFor(() => {
      expect(toggleButton).toHaveAttribute('aria-expanded', 'false');
    });
  });
});

describe('App Component - Jujutsu comment error handling', () => {
  const jjDiffResponse: DiffResponse = {
    ...mockDiffResponse,
    commit: 'commit123',
    baseCommitish: 'commit000',
    targetCommitish: 'commit123',
    requestedBaseCommitish: 'commit000',
    requestedTargetCommitish: 'commit123',
    files: [
      {
        path: 'test.ts',
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
                content: 'const first = 1;',
                newLineNumber: 10,
              },
            ],
          },
        ],
      },
    ],
  };

  beforeEach(() => {
    vi.clearAllMocks();
    mockComments = [];
    mockConfirm.mockReturnValue(false);
    window.localStorage.setItem('difit.diffViewMode', 'unified');
  });

  it('shows a dismissible error banner and preserves the draft when POST /api/jj/comments fails', async () => {
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});

    vi.mocked(global.fetch).mockImplementation((input, init) => {
      const url = String(input);
      if (url === '/api/jj/status') {
        return Promise.resolve({
          ok: true,
          json: async () => ({ isJj: true }),
        } as Response);
      }
      if (url === '/api/jj/changes') {
        return Promise.resolve({
          ok: true,
          json: async () => ({ changes: [] }),
        } as Response);
      }
      if (url === '/api/jj/comments' && init?.method === 'POST') {
        return Promise.resolve({
          ok: false,
          status: 500,
          statusText: 'Internal Server Error',
          json: async () => ({ error: 'jj error: The working copy is stale' }),
        } as Response);
      }
      if (url.startsWith('/api/jj/comments')) {
        return Promise.resolve({
          ok: true,
          json: async () => ({ threads: mockComments }),
        } as Response);
      }
      if (url.includes('/api/revisions')) {
        return Promise.resolve({
          ok: true,
          json: async () => null,
        } as Response);
      }
      return Promise.resolve({
        ok: true,
        json: async () => jjDiffResponse,
        blob: async () => ({ size: 1024 }),
      } as Response);
    });

    const { container } = renderApp();

    await waitFor(() => {
      expect(vi.mocked(global.fetch)).toHaveBeenCalledWith(
        expect.stringContaining('/api/jj/comments?target=commit123'),
      );
    });

    const row = await waitFor(() => {
      const found = container.querySelector('[data-diff-line-row="true"]');
      expect(found).not.toBeNull();
      return found!;
    });

    fireEvent.click(row);
    fireEvent.click(row, { shiftKey: true });

    const textarea = await screen.findByPlaceholderText('Leave a comment...');
    fireEvent.change(textarea, { target: { value: 'Draft comment on stale workspace' } });
    fireEvent.click(screen.getByRole('button', { name: 'Submit' }));

    const alerts = await waitFor(() => {
      const foundAlerts = screen.getAllByRole('alert');
      expect(foundAlerts).toHaveLength(2);
      return foundAlerts;
    });
    for (const alert of alerts) {
      expect(alert).toHaveTextContent('jj error: The working copy is stale');
    }
    expect(screen.getByPlaceholderText('Leave a comment...')).toHaveValue(
      'Draft comment on stale workspace',
    );

    // Dismissing the top-level banner leaves the inline form draft intact.
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss comment error' }));
    expect(screen.getAllByRole('alert')).toHaveLength(1);
    expect(screen.getByPlaceholderText('Leave a comment...')).toHaveValue(
      'Draft comment on stale workspace',
    );

    consoleErrorSpy.mockRestore();
  });

  it('preserves the reply draft and surfaces errors when POST /api/jj/comments/:id/reply fails', async () => {
    mockComments = [
      createMockThread({
        id: 'thread-99',
        filePath: 'test.ts',
        line: 10,
        body: 'Initial thread comment',
      }),
    ];

    let replyAttempts = 0;
    vi.mocked(global.fetch).mockImplementation((input, init) => {
      const url = String(input);
      if (url === '/api/jj/status') {
        return Promise.resolve({
          ok: true,
          json: async () => ({ isJj: true }),
        } as Response);
      }
      if (url === '/api/jj/changes') {
        return Promise.resolve({
          ok: true,
          json: async () => ({ changes: [] }),
        } as Response);
      }
      if (url === '/api/jj/comments/thread-99/reply' && init?.method === 'POST') {
        replyAttempts += 1;
        if (replyAttempts === 1) {
          return Promise.resolve({
            ok: false,
            status: 500,
            statusText: 'Internal Server Error',
            json: async () => ({ error: 'jj error: Thread not found in revision' }),
          } as Response);
        }
        return Promise.resolve({
          ok: true,
          json: async () => ({ id: 'reply-1', success: true }),
        } as Response);
      }
      if (url.startsWith('/api/jj/comments')) {
        return Promise.resolve({
          ok: true,
          json: async () => ({ threads: mockComments }),
        } as Response);
      }
      if (url.includes('/api/revisions')) {
        return Promise.resolve({
          ok: true,
          json: async () => null,
        } as Response);
      }
      return Promise.resolve({
        ok: true,
        json: async () => jjDiffResponse,
        blob: async () => ({ size: 1024 }),
      } as Response);
    });

    renderApp();

    await waitFor(() => {
      expect(vi.mocked(global.fetch)).toHaveBeenCalledWith(
        expect.stringContaining('/api/jj/comments?target=commit123'),
      );
    });

    fireEvent.click(await screen.findByRole('button', { name: 'Write a reply...' }));
    const replyTextarea = await screen.findByPlaceholderText('Write a reply...');
    fireEvent.change(replyTextarea, { target: { value: 'Follow-up note' } });
    fireEvent.click(screen.getByRole('button', { name: 'Reply' }));

    const alerts = await waitFor(() => {
      const foundAlerts = screen.getAllByRole('alert');
      expect(foundAlerts).toHaveLength(2);
      return foundAlerts;
    });
    for (const alert of alerts) {
      expect(alert).toHaveTextContent('jj error: Thread not found in revision');
    }
    expect(screen.getByPlaceholderText('Write a reply...')).toHaveValue('Follow-up note');

    // Retrying clears the banner and closes the reply form on success.
    fireEvent.click(screen.getByRole('button', { name: 'Reply' }));
    await waitFor(() => {
      expect(replyAttempts).toBe(2);
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
      expect(screen.queryByPlaceholderText('Write a reply...')).not.toBeInTheDocument();
    });
  });

  it('shows the error banner cleanly when POST /api/jj/comments/:id/resolve fails and clears it on success', async () => {
    mockComments = [
      createMockThread({
        id: 'thread-42',
        filePath: 'test.ts',
        line: 10,
        body: 'Check error handling',
      }),
    ];

    let resolveAttempts = 0;
    vi.mocked(global.fetch).mockImplementation((input, init) => {
      const url = String(input);
      if (url === '/api/jj/status') {
        return Promise.resolve({
          ok: true,
          json: async () => ({ isJj: true }),
        } as Response);
      }
      if (url === '/api/jj/changes') {
        return Promise.resolve({
          ok: true,
          json: async () => ({ changes: [] }),
        } as Response);
      }
      if (url === '/api/jj/comments/thread-42/resolve' && init?.method === 'POST') {
        resolveAttempts += 1;
        if (resolveAttempts === 1) {
          return Promise.resolve({
            ok: false,
            status: 500,
            statusText: 'Internal Server Error',
            json: async () => ({ error: 'jj error: Failed to write store' }),
          } as Response);
        }
        return Promise.resolve({
          ok: true,
          json: async () => ({ success: true, resolved: true }),
        } as Response);
      }
      if (url.startsWith('/api/jj/comments')) {
        return Promise.resolve({
          ok: true,
          json: async () => ({ threads: mockComments }),
        } as Response);
      }
      if (url.includes('/api/revisions')) {
        return Promise.resolve({
          ok: true,
          json: async () => null,
        } as Response);
      }
      return Promise.resolve({
        ok: true,
        json: async () => jjDiffResponse,
        blob: async () => ({ size: 1024 }),
      } as Response);
    });

    renderApp();

    await waitFor(() => {
      expect(vi.mocked(global.fetch)).toHaveBeenCalledWith(
        expect.stringContaining('/api/jj/comments?target=commit123'),
      );
    });

    fireEvent.click(await screen.findByRole('button', { name: 'Resolve thread' }));
    const confirmButton = screen.queryByRole('button', { name: 'Resolve' });
    if (confirmButton) {
      fireEvent.click(confirmButton);
    }

    const banner = await screen.findByRole('alert');
    expect(banner).toHaveTextContent('jj error: Failed to write store');

    // Retrying clears the error banner once the resolve request succeeds.
    fireEvent.click(screen.getByRole('button', { name: 'Resolve thread' }));
    const retryConfirm = screen.queryByRole('button', { name: 'Resolve' });
    if (retryConfirm) {
      fireEvent.click(retryConfirm);
    }

    await waitFor(() => {
      expect(resolveAttempts).toBe(2);
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    });
  });

  it('displays the Revisions tree above FileList, skips empty undescribed @, and hides the duplicate header Change selector when sidebar is open', async () => {
    vi.mocked(useViewport).mockReturnValue({ isMobile: false, isDesktop: true });

    const emptyAtChange = {
      changeId: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      shortChangeId: 'aaaaaaaa',
      shortestChangeId: 'a',
      commitId: '1111111111111111111111111111111111111111',
      shortCommitId: '11111111',
      isWorkingCopy: true,
      isEmpty: true,
      hasDescription: false,
      isImmutable: false,
      isConflict: false,
      bookmarks: [],
      parentCommitIds: ['2222222222222222222222222222222222222222'],
      timestamp: '2026-11-12 19:00:00',
      description: '(no description set)',
      commentCount: 0,
      unresolvedCommentCount: 0,
    };

    const parentChange = {
      changeId: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
      shortChangeId: 'bbbbbbbb',
      shortestChangeId: 'b',
      commitId: '2222222222222222222222222222222222222222',
      shortCommitId: '22222222',
      isWorkingCopy: false,
      isEmpty: false,
      hasDescription: true,
      isImmutable: false,
      isConflict: false,
      bookmarks: ['feat-sidebar'],
      parentCommitIds: [],
      timestamp: '2026-11-12 18:55:00',
      description: 'Active change under review',
      commentCount: 0,
      unresolvedCommentCount: 0,
    };

    const graphRows = [
      {
        type: 'commit' as const,
        key: emptyAtChange.commitId,
        graphCols: ['@'],
        downCols: [0],
        change: emptyAtChange,
      },
      {
        type: 'commit' as const,
        key: parentChange.commitId,
        graphCols: ['○'],
        downCols: [],
        change: parentChange,
      },
    ];

    const jjDiffResponse: DiffResponse = {
      ...mockDiffResponse,
      commit: '2222222222222222222222222222222222222222',
      targetCommitish: '2222222222222222222222222222222222222222',
      baseCommitish: '2222222222222222222222222222222222222222-',
    };

    vi.mocked(global.fetch).mockImplementation((input) => {
      const url = String(input);
      if (url === '/api/jj/status') {
        return Promise.resolve({
          ok: true,
          json: async () => ({ isJj: true }),
        } as Response);
      }
      if (url === '/api/jj/changes') {
        return Promise.resolve({
          ok: true,
          json: async () => ({ changes: [emptyAtChange, parentChange], graphRows }),
        } as Response);
      }
      if (url === `/api/jj/changes/${parentChange.changeId}/commits`) {
        return Promise.resolve({
          ok: true,
          json: async () => ({
            commits: [
              {
                commitId: parentChange.commitId,
                shortCommitId: parentChange.shortCommitId,
                timestamp: parentChange.timestamp,
                description: parentChange.description,
                isWorkingCopy: false,
                commentCount: 0,
                unresolvedCommentCount: 0,
              },
            ],
          }),
        } as Response);
      }
      if (url.startsWith('/api/jj/diff') || url.startsWith('/api/diff')) {
        return Promise.resolve({
          ok: true,
          json: async () => jjDiffResponse,
        } as Response);
      }
      if (url.startsWith('/api/jj/comments')) {
        return Promise.resolve({
          ok: true,
          json: async () => ({ threads: [] }),
        } as Response);
      }
      if (url.includes('/api/revisions')) {
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

    const { container } = renderApp();

    const revisionsSection = await screen.findByRole('region', { name: 'Revisions' });
    const sidebar = container.querySelector('#file-tree-panel');
    expect(sidebar).toContainElement(revisionsSection);

    // Revisions tree renders above FileList and auto-selects the non-empty parent change.
    const parentRow = await screen.findByRole('button', { name: /Active change under review/ });
    await waitFor(() => {
      expect(parentRow).toHaveAttribute('aria-current', 'true');
    });

    // FileList renders inside the sidebar below the Revisions tree.
    const fileItems = await screen.findAllByText('test.ts');
    expect(sidebar).toContainElement(fileItems[0] ?? null);
    expect(
      revisionsSection.compareDocumentPosition(fileItems[0] as Node) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    // Duplicate header Change: dropdown stays hidden while the desktop sidebar is open.
    expect(screen.queryByText('Change:')).not.toBeInTheDocument();

    // Collapsing the sidebar reveals the header Change: fallback selector.
    fireEvent.click(screen.getByTitle('Collapse file tree'));
    expect(await screen.findByText('Change:')).toBeInTheDocument();
  });
});
