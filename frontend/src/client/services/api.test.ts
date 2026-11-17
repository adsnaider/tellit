import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
  addJjComment,
  approveJjRevision,
  fetchDiff,
  fetchFileBlobText,
  fetchGeneratedStatus,
  fetchJjChanges,
  fetchJjComments,
  fetchJjCommitsForChange,
  fetchJjDiff,
  fetchJjStatus,
  fetchLineCount,
  normalizeJjBaseCommitId,
  openInEditor,
  readApiErrorMessage,
  replyJjComment,
  resolveJjComment,
} from './api';

describe('API client helpers', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('normalizeJjBaseCommitId', () => {
    it('strips parent and relative suffixes', () => {
      expect(normalizeJjBaseCommitId(undefined)).toBeUndefined();
      expect(normalizeJjBaseCommitId('parent')).toBeUndefined();
      expect(normalizeJjBaseCommitId('abc1234^')).toBeUndefined();
      expect(normalizeJjBaseCommitId('abc1234-')).toBeUndefined();
      expect(normalizeJjBaseCommitId('abc1234')).toBe('abc1234');
    });
  });

  describe('readApiErrorMessage', () => {
    it('returns the JSON error string when present', async () => {
      const response = new Response(JSON.stringify({ error: 'Commit not found' }), {
        status: 404,
        statusText: 'Not Found',
      });

      await expect(readApiErrorMessage(response, 'Request failed')).resolves.toBe(
        'Commit not found',
      );
    });

    it('falls back to status code when JSON parsing fails', async () => {
      const response = new Response('plain text error', {
        status: 500,
        statusText: 'Internal Server Error',
      });

      await expect(readApiErrorMessage(response, 'Request failed')).resolves.toBe(
        'Request failed (500 Internal Server Error)',
      );
    });
  });

  describe('fetchJjStatus', () => {
    it('returns isJj and repoRoot from /api/jj/status', async () => {
      vi.mocked(global.fetch).mockResolvedValueOnce(
        new Response(JSON.stringify({ isJj: true, repoRoot: '/repo' }), { status: 200 }),
      );

      await expect(fetchJjStatus()).resolves.toEqual({
        isJj: true,
        repoRoot: '/repo',
      });
      expect(global.fetch).toHaveBeenCalledWith('/api/jj/status', undefined);
    });

    it('returns isJj: false when endpoint fails', async () => {
      vi.mocked(global.fetch).mockRejectedValueOnce(new Error('network error'));

      await expect(fetchJjStatus()).resolves.toEqual({ isJj: false });
    });

    it('rethrows AbortError so cancelled effects exit cleanly', async () => {
      const abortError = Object.assign(new Error('Aborted'), { name: 'AbortError' });
      vi.mocked(global.fetch).mockRejectedValueOnce(abortError);

      await expect(fetchJjStatus()).rejects.toMatchObject({ name: 'AbortError' });
    });
  });

  describe('fetchJjChanges and fetchJjCommitsForChange', () => {
    it('fetches changes and synthesizes fallback graphRows when omitted', async () => {
      const change = {
        changeId: 'kkmpptxz12345678',
        shortChangeId: 'kkmpptxz',
        commitId: '44235e975acae6',
        shortCommitId: '44235e9',
        isWorkingCopy: true,
        timestamp: '2026-11-16 15:00:00',
        description: 'Implement Svelte shell',
        commentCount: 0,
        unresolvedCommentCount: 0,
      };

      vi.mocked(global.fetch).mockResolvedValueOnce(
        new Response(JSON.stringify({ changes: [change] }), { status: 200 }),
      );

      const result = await fetchJjChanges('  @-  ');
      expect(global.fetch).toHaveBeenCalledWith('/api/jj/changes?query=%40-', undefined);
      expect(result.changes).toEqual([change]);
      expect(result.graphRows).toEqual([
        {
          type: 'commit',
          key: change.commitId,
          graphCols: ['@'],
          downCols: [],
          change,
        },
      ]);
    });

    it('fetches commits for a change ID', async () => {
      const commits = [
        {
          commitId: '335eb579f263bb',
          shortCommitId: '335eb57',
          timestamp: '2026-11-16 12:22:54',
          description: 'Add dprint config',
          isWorkingCopy: false,
          commentCount: 0,
          unresolvedCommentCount: 0,
          approved: true,
        },
      ];
      vi.mocked(global.fetch).mockResolvedValueOnce(
        new Response(JSON.stringify({ commits }), { status: 200 }),
      );

      await expect(fetchJjCommitsForChange('ltppwqry')).resolves.toEqual(commits);
      expect(global.fetch).toHaveBeenCalledWith('/api/jj/changes/ltppwqry/commits', undefined);
    });

    it('throws server error message on failure', async () => {
      vi.mocked(global.fetch).mockResolvedValueOnce(
        new Response(JSON.stringify({ error: 'Invalid revset' }), { status: 500 }),
      );

      await expect(fetchJjChanges('bad()')).rejects.toThrow('Invalid revset');
    });
  });

  describe('approveJjRevision and fetchGeneratedStatus', () => {
    it('posts approval and revoke requests to /api/jj/approve', async () => {
      vi.mocked(global.fetch)
        .mockResolvedValueOnce(
          new Response(JSON.stringify({ success: true, approved: true }), { status: 200 }),
        )
        .mockResolvedValueOnce(
          new Response(JSON.stringify({ error: 'Cannot approve' }), { status: 500 }),
        );

      await expect(approveJjRevision('335eb579f263bb', false)).resolves.toBe(true);
      expect(global.fetch).toHaveBeenCalledWith('/api/jj/approve', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ commitId: '335eb579f263bb', disapprove: false }),
      });

      await expect(approveJjRevision('335eb579f263bb', true)).rejects.toThrow('Cannot approve');
    });

    it('fetches and validates generated file status', async () => {
      vi.mocked(global.fetch).mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            path: 'src/gen/api.pb.ts',
            ref: 'HEAD',
            isGenerated: true,
            source: 'content',
          }),
          { status: 200 },
        ),
      );

      await expect(fetchGeneratedStatus('src/gen/api.pb.ts', 'HEAD')).resolves.toEqual({
        path: 'src/gen/api.pb.ts',
        ref: 'HEAD',
        isGenerated: true,
        source: 'content',
      });
      expect(global.fetch).toHaveBeenCalledWith(
        '/api/generated-status/src%2Fgen%2Fapi.pb.ts?ref=HEAD',
        undefined,
      );
    });
  });

  describe('fetchDiff and fetchJjDiff', () => {
    it('encodes query parameters for /api/diff', async () => {
      vi.mocked(global.fetch).mockResolvedValueOnce(
        new Response(JSON.stringify({ commit: 'HEAD', files: [] }), { status: 200 }),
      );

      await fetchDiff({
        base: 'main',
        target: '.',
        baseMode: 'merge-base',
        ignoreWhitespace: false,
      });

      expect(global.fetch).toHaveBeenCalledWith(
        '/api/diff?ignoreWhitespace=false&base=main&target=.&baseMode=merge-base',
        undefined,
      );
    });

    it('omits parent base and encodes explicit inter-diff base for /api/jj/diff', async () => {
      vi.mocked(global.fetch)
        .mockResolvedValueOnce(
          new Response(JSON.stringify({ commit: 'target1', files: [] }), { status: 200 }),
        )
        .mockResolvedValueOnce(
          new Response(JSON.stringify({ commit: 'target1', files: [] }), { status: 200 }),
        );

      await fetchJjDiff({ target: 'target1', base: 'parent' });
      expect(global.fetch).toHaveBeenNthCalledWith(1, '/api/jj/diff?target=target1', undefined);

      await fetchJjDiff({ target: 'target1', base: 'base0', ignoreRebase: false });
      expect(global.fetch).toHaveBeenNthCalledWith(
        2,
        '/api/jj/diff?target=target1&base=base0&ignoreRebase=false',
        undefined,
      );
    });

    it('throws Invalid diff response when payload lacks commit or files array', async () => {
      vi.mocked(global.fetch)
        .mockResolvedValueOnce(
          new Response(JSON.stringify({ commit: 'HEAD', files: 'bad' }), { status: 200 }),
        )
        .mockResolvedValueOnce(new Response(JSON.stringify({}), { status: 200 }));

      await expect(fetchDiff()).rejects.toThrow('Invalid diff response');
      await expect(fetchJjDiff({ target: 'target1' })).rejects.toThrow('Invalid diff response');
    });
  });

  describe('blob, line-count, open-in-editor, and jj comments helpers', () => {
    it('fetches blob text, line counts, and triggers open-in-editor', async () => {
      vi.mocked(global.fetch)
        .mockResolvedValueOnce(new Response('line 1\nline 2\n', { status: 200 }))
        .mockResolvedValueOnce(
          new Response(JSON.stringify({ oldLineCount: 20, newLineCount: 22 }), { status: 200 }),
        )
        .mockResolvedValueOnce(new Response(JSON.stringify({ success: true }), { status: 200 }));

      await expect(fetchFileBlobText('src/a b.ts', 'abc123')).resolves.toBe('line 1\nline 2\n');
      expect(global.fetch).toHaveBeenNthCalledWith(
        1,
        '/api/blob/src%2Fa%20b.ts?ref=abc123',
        undefined,
      );

      await expect(fetchLineCount('src/new.ts', 'base1', 'head1', 'src/old.ts')).resolves.toEqual({
        oldLineCount: 20,
        newLineCount: 22,
      });
      expect(global.fetch).toHaveBeenNthCalledWith(
        2,
        '/api/line-count/src%2Fnew.ts?oldRef=base1&newRef=head1&oldPath=src%2Fold.ts',
        undefined,
      );

      await openInEditor({ filePath: 'src/new.ts', line: 12 });
      expect(global.fetch).toHaveBeenNthCalledWith(3, '/api/open-in-editor', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ filePath: 'src/new.ts', line: 12 }),
      });
    });

    it('fetches, creates, replies to, and resolves jj comments', async () => {
      const thread = {
        id: 't1',
        filePath: 'src/app.ts',
        createdAt: '2026-11-16T00:00:00Z',
        updatedAt: '2026-11-16T00:00:00Z',
        position: { side: 'new', line: 5 },
        messages: [],
      };
      vi.mocked(global.fetch)
        .mockResolvedValueOnce(new Response(JSON.stringify({ threads: [thread] }), { status: 200 }))
        .mockResolvedValueOnce(new Response(JSON.stringify({ threadId: 't2' }), { status: 200 }))
        .mockResolvedValueOnce(new Response(JSON.stringify({ success: true }), { status: 200 }))
        .mockResolvedValueOnce(
          new Response(JSON.stringify({ success: true, resolved: true }), { status: 200 }),
        );

      await expect(fetchJjComments('commit2', 'parent')).resolves.toEqual([thread]);
      expect(global.fetch).toHaveBeenNthCalledWith(1, '/api/jj/comments?commit=commit2', undefined);

      await expect(
        addJjComment({
          commitId: 'commit2',
          filePath: 'src/app.ts',
          line: { start: 10, end: 12 },
          content: 'Check range',
        }),
      ).resolves.toBe('t2');

      await replyJjComment('t2', 'Done');
      expect(global.fetch).toHaveBeenNthCalledWith(3, '/api/jj/comments/t2/reply', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ content: 'Done' }),
      });

      await expect(resolveJjComment('t2', false)).resolves.toBe(true);
      expect(global.fetch).toHaveBeenNthCalledWith(4, '/api/jj/comments/t2/resolve', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ unresolve: false }),
      });
    });
  });
});
