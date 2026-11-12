import { mkdtempSync, rmSync, writeFileSync } from 'fs';
import { tmpdir } from 'os';
import { join } from 'path';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  JJ_LOG_EDGE_MARKER,
  JJ_LOG_NODE_MARKER,
  JujutsuService,
  parseJjLogGraph,
  type CommenterThreadInfo,
} from './jujutsu';
import { createCommitMessageDiffFile } from './server';

describe('JujutsuService comment aggregation', () => {
  let jjService: JujutsuService;

  beforeEach(() => {
    jjService = new JujutsuService();
  });

  it('assigns side: old to base commit comments and side: new to target commit comments when diffing against an earlier commit', async () => {
    const threadOnBase: CommenterThreadInfo = {
      thread_id: 't-base-1',
      change_id: 'change-123',
      commit_id: 'commit-base-aaa',
      git_commit_hash: 'commit-base-aaa',
      filepath: 'src/main.rs',
      line_range: { start: 10, end: 15 },
      comments: ['Comment on earlier revision'],
      resolved: false,
    };

    const threadOnTarget: CommenterThreadInfo = {
      thread_id: 't-target-1',
      change_id: 'change-123',
      commit_id: 'commit-target-bbb',
      git_commit_hash: 'commit-target-bbb',
      filepath: 'src/main.rs',
      line_range: { start: 20, end: 25 },
      comments: ['Comment on later revision'],
      resolved: true,
    };

    vi.spyOn(jjService, 'getChange').mockImplementation(async (query: string) => {
      if (query === 'commit-base-aaa') {
        return {
          changeId: 'change-123',
          shortChangeId: 'change-1',
          commitId: 'commit-base-aaa',
          shortCommitId: 'commit-b',
          isWorkingCopy: false,
          timestamp: '2026-10-20 12:00:00',
          description: 'Base commit',
          commentCount: 0,
          unresolvedCommentCount: 0,
        };
      }
      return {
        changeId: 'change-123',
        shortChangeId: 'change-1',
        commitId: 'commit-target-bbb',
        shortCommitId: 'commit-t',
        isWorkingCopy: true,
        timestamp: '2026-10-20 13:00:00',
        description: 'Target commit',
        commentCount: 0,
        unresolvedCommentCount: 0,
      };
    });

    vi.spyOn(jjService, 'getChangeThreads').mockResolvedValue([threadOnBase, threadOnTarget]);

    const result = await jjService.getCommentsForDiff({
      targetCommit: 'commit-target-bbb',
      baseCommit: 'commit-base-aaa',
    });

    expect(result).toHaveLength(2);
    const baseThread = result.find((t) => t.thread_id === 't-base-1');
    const targetThread = result.find((t) => t.thread_id === 't-target-1');

    expect(baseThread).toBeDefined();
    expect(baseThread?.side).toBe('old');

    expect(targetThread).toBeDefined();
    expect(targetThread?.side).toBe('new');
  });

  it('carries forward unresolved comments from earlier commits on the same change when viewing a later commit against parent', async () => {
    const unresolvedOnEarlierCommit: CommenterThreadInfo = {
      thread_id: 't-earlier-unresolved',
      change_id: 'change-123',
      commit_id: 'commit-earlier-aaa',
      git_commit_hash: 'commit-earlier-aaa',
      filepath: 'src/lib.rs',
      line_range: { start: 5, end: 5 },
      comments: ['Still open feedback'],
      resolved: false,
    };

    const resolvedOnEarlierCommit: CommenterThreadInfo = {
      thread_id: 't-earlier-resolved',
      change_id: 'change-123',
      commit_id: 'commit-earlier-aaa',
      git_commit_hash: 'commit-earlier-aaa',
      filepath: 'src/lib.rs',
      line_range: { start: 1, end: 2 },
      comments: ['Resolved feedback'],
      resolved: true,
    };

    const threadOnCurrentCommit: CommenterThreadInfo = {
      thread_id: 't-current-new',
      change_id: 'change-123',
      commit_id: 'commit-current-bbb',
      git_commit_hash: 'commit-current-bbb',
      filepath: 'src/main.rs',
      line_range: { start: 30, end: 30 },
      comments: ['New comment on current revision'],
      resolved: false,
    };

    vi.spyOn(jjService, 'getChange').mockResolvedValue({
      changeId: 'change-123',
      shortChangeId: 'change-1',
      commitId: 'commit-current-bbb',
      shortCommitId: 'commit-c',
      isWorkingCopy: true,
      timestamp: '2026-10-20 14:00:00',
      description: 'Current commit',
      commentCount: 0,
      unresolvedCommentCount: 0,
    });

    vi.spyOn(jjService, 'getChangeThreads').mockResolvedValue([
      unresolvedOnEarlierCommit,
      resolvedOnEarlierCommit,
      threadOnCurrentCommit,
    ]);

    const result = await jjService.getCommentsForDiff({
      targetCommit: 'commit-current-bbb',
    });

    expect(result).toHaveLength(3);
    const carriedOver = result.find((t) => t.thread_id === 't-earlier-unresolved');
    const current = result.find((t) => t.thread_id === 't-current-new');
    const oldResolved = result.find((t) => t.thread_id === 't-earlier-resolved');

    expect(carriedOver).toBeDefined();
    expect(carriedOver?.side).toBe('new');
    expect(carriedOver?.resolved).toBe(false);

    expect(current).toBeDefined();
    expect(current?.side).toBe('new');

    expect(oldResolved).toBeDefined();
    expect(oldResolved?.side).toBe('new');
    expect(oldResolved?.resolved).toBe(true);
  });

  it('retrieves commit description using getCommitDescription', async () => {
    vi.spyOn(jjService, 'runJj').mockResolvedValue('feat: test description\n\nDetailed message');
    const desc = await jjService.getCommitDescription('@');
    expect(desc).toBe('feat: test description\n\nDetailed message');
  });

  describe('createCommitMessageDiffFile', () => {
    it('creates an added virtual file when base description is empty', () => {
      const file = createCommitMessageDiffFile(
        '',
        'feat: add user login\n\nFirst line\nSecond line',
      );
      expect(file.path).toBe('Commit Message');
      expect(file.status).toBe('added');
      expect(file.additions).toBe(4);
      expect(file.deletions).toBe(0);
      expect(file.chunks[0].lines).toHaveLength(4);
      expect(file.chunks[0].lines[0]).toEqual({
        type: 'add',
        content: 'feat: add user login',
        newLineNumber: 1,
      });
    });

    it('creates a normal context virtual file when base and target descriptions are identical', () => {
      const desc = 'feat: same message';
      const file = createCommitMessageDiffFile(desc, desc);
      expect(file.path).toBe('Commit Message');
      expect(file.status).toBe('modified');
      expect(file.additions).toBe(0);
      expect(file.deletions).toBe(0);
      expect(file.chunks[0].lines[0]).toEqual({
        type: 'normal',
        content: 'feat: same message',
        oldLineNumber: 1,
        newLineNumber: 1,
      });
    });

    it('creates a diff virtual file when commit messages differ across commits in a change', () => {
      const base = 'feat: initial commit message\n\nSome body';
      const target = 'feat: updated commit message\n\nSome body';
      const file = createCommitMessageDiffFile(base, target);
      expect(file.path).toBe('Commit Message');
      expect(file.status).toBe('modified');
      expect(file.additions).toBeGreaterThan(0);
      expect(file.deletions).toBeGreaterThan(0);
      const delLine = file.chunks[0].lines.find((l) => l.type === 'delete');
      const addLine = file.chunks[0].lines.find((l) => l.type === 'add');
      expect(delLine?.content).toBe('feat: initial commit message');
      expect(addLine?.content).toBe('feat: updated commit message');
    });
  });

  describe('JujutsuService rebase diff isolation', () => {
    it('returns standard diff for commit vs parent', async () => {
      vi.spyOn(jjService, 'runJj').mockResolvedValue('diff --git a/test.txt b/test.txt\n...');
      const res = await jjService.getDiff('commit-target');
      expect(res.rawDiff).toContain('diff --git');
      expect(res.isRebaseDiff).toBe(false);
      expect(res.rebaseIgnored).toBe(false);
    });

    it('isolates upstream rebase changes using 3-way git merge-tree when parents differ within same change', async () => {
      const c1 = '1111111111111111111111111111111111111111';
      const c2 = '2222222222222222222222222222222222222222';
      const p1 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
      const p2 = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
      const changeId = 'cccccccccccccccccccccccccccccccc';

      const logOutput = `${c1}\t${changeId}\t${p1}\n${c2}\t${changeId}\t${p2}\n`;

      vi.spyOn(jjService, 'runJj').mockImplementation(async (args) => {
        if (args.includes('log')) {
          return logOutput;
        }
        return 'fallback diff';
      });

      const virtualTreeOid = '3333333333333333333333333333333333333333';
      const gitSpy = vi.spyOn(jjService, 'runGit').mockImplementation(async (args) => {
        if (args[0] === 'merge-tree') {
          expect(args).toEqual(['merge-tree', '--write-tree', '--merge-base', p1, p2, c1]);
          return { stdout: `${virtualTreeOid}\n`, stderr: '', exitCode: 0 };
        }
        if (args[0] === 'diff') {
          expect(args).toEqual(['diff', '--no-color', '--no-ext-diff', virtualTreeOid, c2]);
          return {
            stdout: 'diff --git a/only-author.txt b/only-author.txt\n',
            stderr: '',
            exitCode: 0,
          };
        }
        return { stdout: '', stderr: '', exitCode: 1 };
      });

      const res = await jjService.getDiff(c2, c1, true);
      expect(gitSpy).toHaveBeenCalledTimes(2);
      expect(res.isRebaseDiff).toBe(true);
      expect(res.rebaseIgnored).toBe(true);
      expect(res.rebaseConflict).toBe(false);
      expect(res.rawDiff).toContain('only-author.txt');
    });

    it('falls back gracefully to direct diff when merge-tree encounters a rebase conflict', async () => {
      const c1 = '1111111111111111111111111111111111111111';
      const c2 = '2222222222222222222222222222222222222222';
      const p1 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
      const p2 = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
      const changeId = 'cccccccccccccccccccccccccccccccc';

      const logOutput = `${c1}\t${changeId}\t${p1}\n${c2}\t${changeId}\t${p2}\n`;

      vi.spyOn(jjService, 'runJj').mockImplementation(async (args) => {
        if (args.includes('log')) {
          return logOutput;
        }
        return 'direct diff with rebase conflicts';
      });

      vi.spyOn(jjService, 'runGit').mockResolvedValue({
        stdout: 'conflict info',
        stderr: 'conflict',
        exitCode: 1,
      });

      const res = await jjService.getDiff(c2, c1, true);
      expect(res.isRebaseDiff).toBe(true);
      expect(res.rebaseIgnored).toBe(false);
      expect(res.rebaseConflict).toBe(true);
      expect(res.rawDiff).toBe('direct diff with rebase conflicts');
    });

    it('honors ignoreRebase=false to show the full direct diff including rebase changes', async () => {
      const c1 = '1111111111111111111111111111111111111111';
      const c2 = '2222222222222222222222222222222222222222';
      const p1 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
      const p2 = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
      const changeId = 'cccccccccccccccccccccccccccccccc';

      const logOutput = `${c1}\t${changeId}\t${p1}\n${c2}\t${changeId}\t${p2}\n`;

      vi.spyOn(jjService, 'runJj').mockImplementation(async (args) => {
        if (args.includes('log')) {
          return logOutput;
        }
        return 'full direct diff with upstream';
      });

      const gitSpy = vi.spyOn(jjService, 'runGit');

      const res = await jjService.getDiff(c2, c1, false);
      expect(res.isRebaseDiff).toBe(true);
      expect(res.rebaseIgnored).toBe(false);
      expect(res.rebaseConflict).toBe(false);
      expect(res.rawDiff).toBe('full direct diff with upstream');
      expect(gitSpy).not.toHaveBeenCalled();
    });
  });

  describe('Clean Rebase Approval Propagation', () => {
    it('marks later commit as cleanRebaseApproved when 3-way virtual tree matches target commit', async () => {
      const c1 = '1111111111111111111111111111111111111111';
      const c2 = '2222222222222222222222222222222222222222';
      const p1 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
      const p2 = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
      const changeId = 'cccccccccccccccccccccccccccccccc';

      // c1 was approved, c2 is unapproved
      const evologOutput = [
        `${c1}\t2026-10-23 10:00:00\t0\tInitial commit\t${p1}`,
        `${c2}\t2026-10-23 11:00:00\t1\tRebased commit\t${p2}`,
      ].join('\n');

      vi.spyOn(jjService, 'runJj').mockResolvedValue(evologOutput);
      vi.spyOn(jjService, 'getChangeThreads').mockResolvedValue([]);
      vi.spyOn(jjService, 'getCommitDetails').mockImplementation(async (commitId) => {
        if (commitId === c1) {
          return { threads: [], approval: true };
        }
        return { threads: [], approval: false };
      });

      const virtualTreeOid = '9999999999999999999999999999999999999999';
      vi.spyOn(jjService, 'runGit').mockImplementation(async (args) => {
        if (args[0] === 'merge-tree') {
          return { stdout: `${virtualTreeOid}\n`, stderr: '', exitCode: 0 };
        }
        if (args[0] === 'diff') {
          // Zero diff between virtual tree and c2 -> clean rebase!
          return { stdout: '', stderr: '', exitCode: 0 };
        }
        return { stdout: '', stderr: '', exitCode: 1 };
      });

      const commits = await jjService.getCommitsForChange(changeId);
      expect(commits).toHaveLength(2);
      expect(commits[0].approved).toBe(true);
      expect(commits[0].cleanRebaseApproved).toBeUndefined();

      expect(commits[1].approved).toBe(false);
      expect(commits[1].cleanRebaseApproved).toBe(true);
      expect(commits[1].cleanRebaseSourceCommitId).toBe(c1);
      expect(commits[1].cleanRebaseSourceCommitNumber).toBe(1);
    });

    it('does not mark later commit as cleanRebaseApproved if author made additional code changes', async () => {
      const c1 = '1111111111111111111111111111111111111111';
      const c2 = '2222222222222222222222222222222222222222';
      const p1 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
      const p2 = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
      const changeId = 'cccccccccccccccccccccccccccccccc';

      const evologOutput = [
        `${c1}\t2026-10-23 10:00:00\t0\tInitial commit\t${p1}`,
        `${c2}\t2026-10-23 11:00:00\t1\tRebased and edited\t${p2}`,
      ].join('\n');

      vi.spyOn(jjService, 'runJj').mockResolvedValue(evologOutput);
      vi.spyOn(jjService, 'getChangeThreads').mockResolvedValue([]);
      vi.spyOn(jjService, 'getCommitDetails').mockImplementation(async (commitId) => {
        if (commitId === c1) {
          return { threads: [], approval: true };
        }
        return { threads: [], approval: false };
      });

      const virtualTreeOid = '9999999999999999999999999999999999999999';
      vi.spyOn(jjService, 'runGit').mockImplementation(async (args) => {
        if (args[0] === 'merge-tree') {
          return { stdout: `${virtualTreeOid}\n`, stderr: '', exitCode: 0 };
        }
        if (args[0] === 'diff') {
          // Author made additional edits!
          return { stdout: 'diff --git a/new.txt b/new.txt\n+new line', stderr: '', exitCode: 0 };
        }
        return { stdout: '', stderr: '', exitCode: 1 };
      });

      const commits = await jjService.getCommitsForChange(changeId);
      expect(commits).toHaveLength(2);
      expect(commits[0].approved).toBe(true);
      expect(commits[1].approved).toBe(false);
      expect(commits[1].cleanRebaseApproved).toBeUndefined();
    });

    it('does not mark later commit as cleanRebaseApproved if merge-tree encountered a conflict', async () => {
      const c1 = '1111111111111111111111111111111111111111';
      const c2 = '2222222222222222222222222222222222222222';
      const p1 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
      const p2 = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';
      const changeId = 'cccccccccccccccccccccccccccccccc';

      const evologOutput = [
        `${c1}\t2026-10-23 10:00:00\t0\tInitial commit\t${p1}`,
        `${c2}\t2026-10-23 11:00:00\t1\tRebased with conflict\t${p2}`,
      ].join('\n');

      vi.spyOn(jjService, 'runJj').mockResolvedValue(evologOutput);
      vi.spyOn(jjService, 'getChangeThreads').mockResolvedValue([]);
      vi.spyOn(jjService, 'getCommitDetails').mockImplementation(async (commitId) => {
        if (commitId === c1) {
          return { threads: [], approval: true };
        }
        return { threads: [], approval: false };
      });

      vi.spyOn(jjService, 'runGit').mockResolvedValue({
        stdout: 'conflict',
        stderr: 'conflict',
        exitCode: 1,
      });

      const commits = await jjService.getCommitsForChange(changeId);
      expect(commits).toHaveLength(2);
      expect(commits[0].approved).toBe(true);
      expect(commits[1].approved).toBe(false);
      expect(commits[1].cleanRebaseApproved).toBeUndefined();
    });
  });

  describe('runTellit error formatting', () => {
    let tempDir: string;
    let originalTellitBin: string | undefined;

    beforeEach(() => {
      tempDir = mkdtempSync(join(tmpdir(), 'tellit-test-'));
      originalTellitBin = process.env.TELLIT_BIN;
    });

    afterEach(() => {
      if (originalTellitBin === undefined) {
        delete process.env.TELLIT_BIN;
      } else {
        process.env.TELLIT_BIN = originalTellitBin;
      }
      rmSync(tempDir, { recursive: true, force: true });
    });

    it('extracts trimmed stderr without leaking the command line or draft content', async () => {
      const mockBin = join(tempDir, 'mock-tellit');
      writeFileSync(
        mockBin,
        [
          '#!/bin/sh',
          'case "$2" in',
          '  add)',
          '    printf "  Error: jj error: The working copy is stale  \\n" >&2',
          '    exit 1',
          '    ;;',
          '  reply)',
          '    printf "  Store directory does not exist  \\n" >&2',
          '    exit 1',
          '    ;;',
          '  empty-err)',
          '    exit 1',
          '    ;;',
          '  ok)',
          '    printf "  thread-uuid-123  \\n"',
          '    exit 0',
          '    ;;',
          'esac',
          '',
        ].join('\n'),
        { mode: 0o755 },
      );
      process.env.TELLIT_BIN = mockBin;

      const service = new JujutsuService(tempDir);

      await expect(service.runTellit(['ok'])).resolves.toBe('thread-uuid-123');

      let addErr: Error | undefined;
      try {
        await service.addComment({
          commitId: 'abc123',
          filePath: 'src/lib.rs',
          lineRange: 10,
          content: 'Sensitive draft comment body',
        });
      } catch (err) {
        addErr = err as Error;
      }
      expect(addErr).toBeInstanceOf(Error);
      expect(addErr?.message).toBe('jj error: The working copy is stale');
      expect(addErr?.message).not.toContain('Command failed');
      expect(addErr?.message).not.toContain('Sensitive draft comment body');

      await expect(service.replyComment('t-1', 'Reply body')).rejects.toThrow(
        'Store directory does not exist',
      );

      await expect(service.runTellit(['empty-err'])).rejects.toThrow(/Command failed/);
    });
  });

  describe('parseJjLogGraph', () => {
    const makeNodeLine = (
      prefix: string,
      opts: {
        changeId: string;
        shortestChangeId?: string;
        commitId: string;
        isWorkingCopy?: boolean;
        isEmpty?: boolean;
        isImmutable?: boolean;
        isConflict?: boolean;
        isRoot?: boolean;
        bookmarks?: string[];
        parents?: string[];
        timestamp?: string;
        description?: string;
      },
    ) => {
      const fields = [
        JJ_LOG_NODE_MARKER,
        opts.changeId,
        opts.shortestChangeId ?? opts.changeId.slice(0, 1),
        opts.commitId,
        opts.isWorkingCopy ? '1' : '0',
        opts.isEmpty ? '1' : '0',
        opts.isImmutable ? '1' : '0',
        opts.isConflict ? '1' : '0',
        opts.isRoot ? '1' : '0',
        (opts.bookmarks ?? []).join(','),
        (opts.parents ?? []).join(','),
        opts.timestamp ?? '2026-11-12 13:59:09',
      ];
      if (opts.description !== undefined && opts.description.length > 0) {
        fields.push(opts.description);
      }
      return `${prefix}${fields.join('\t')}`;
    };

    it('parses a linear stack with empty undescribed @, immutable main, root skipping, and trailing elision', () => {
      const raw = [
        makeNodeLine('@  ', {
          changeId: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          shortestChangeId: 'a',
          commitId: '1111111111111111111111111111111111111111',
          isWorkingCopy: true,
          isEmpty: true,
          parents: ['2222222222222222222222222222222222222222'],
        }),
        `│  ${JJ_LOG_EDGE_MARKER}`,
        makeNodeLine('◆  ', {
          changeId: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
          shortestChangeId: 'b',
          commitId: '2222222222222222222222222222222222222222',
          isImmutable: true,
          bookmarks: ['main'],
          parents: ['0000000000000000000000000000000000000000'],
          description: 'Initial commit',
        }),
        `│  ${JJ_LOG_EDGE_MARKER}`,
        '│',
        '~',
      ].join('\n');

      const { changes, graphRows } = parseJjLogGraph(raw);

      expect(changes).toHaveLength(2);
      expect(changes[0]).toMatchObject({
        changeId: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
        shortestChangeId: 'a',
        isWorkingCopy: true,
        isEmpty: true,
        hasDescription: false,
        description: '(no description set)',
        bookmarks: [],
        parentCommitIds: ['2222222222222222222222222222222222222222'],
      });
      expect(changes[1]).toMatchObject({
        changeId: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
        isImmutable: true,
        hasDescription: true,
        description: 'Initial commit',
        bookmarks: ['main'],
      });

      expect(graphRows).toHaveLength(3);
      expect(graphRows[0]).toMatchObject({
        type: 'commit',
        key: '1111111111111111111111111111111111111111',
        graphCols: ['@'],
        downCols: [0],
      });
      expect(graphRows[1]).toMatchObject({
        type: 'commit',
        key: '2222222222222222222222222222222222222222',
        graphCols: ['◆'],
        downCols: [0],
      });
      expect(graphRows[2]).toMatchObject({
        type: 'connector',
        graphCols: ['~'],
        isElided: true,
        bridgesElided: false,
      });
    });

    it('drops root commits and clears dangling downward stems on the preceding row', () => {
      const raw = [
        makeNodeLine('@  ', {
          changeId: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          commitId: '1111111111111111111111111111111111111111',
          isWorkingCopy: true,
          description: 'Top commit',
        }),
        `│  ${JJ_LOG_EDGE_MARKER}`,
        makeNodeLine('◆  ', {
          changeId: '00000000000000000000000000000000',
          commitId: '0000000000000000000000000000000000000000',
          isRoot: true,
          isImmutable: true,
        }),
        `   ${JJ_LOG_EDGE_MARKER}`,
      ].join('\n');

      const { changes, graphRows } = parseJjLogGraph(raw);
      expect(changes).toHaveLength(1);
      expect(graphRows).toHaveLength(1);
      expect(graphRows[0]?.type).toBe('commit');
      if (graphRows[0]?.type === 'commit') {
        expect(graphRows[0].downCols).toEqual([]);
      }
    });

    it('parses branched histories, 2-parent merge edges, bridging elisions, and leading spaces on row 0', () => {
      const raw = [
        makeNodeLine('  @  ', {
          changeId: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          commitId: '1111111111111111111111111111111111111111',
          isWorkingCopy: true,
          description: 'Indented head commit',
        }),
        `  │  ${JJ_LOG_EDGE_MARKER}`,
        makeNodeLine('○ │  ', {
          changeId: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
          commitId: '2222222222222222222222222222222222222222',
          parents: [
            '3333333333333333333333333333333333333333',
            '4444444444444444444444444444444444444444',
          ],
          description: 'Merge commit',
        }),
        `├─╮ │  ${JJ_LOG_EDGE_MARKER}`,
        makeNodeLine('│ ○ │  ', {
          changeId: 'cccccccccccccccccccccccccccccccc',
          commitId: '4444444444444444444444444444444444444444',
          description: 'Side branch',
        }),
        `│ │ │  ${JJ_LOG_EDGE_MARKER}`,
        '├─╯ │',
        '│ ~  (elided revisions)',
        makeNodeLine('◆  ', {
          changeId: 'dddddddddddddddddddddddddddddddd',
          commitId: '3333333333333333333333333333333333333333',
          isImmutable: true,
          bookmarks: ['main'],
          description: 'Base',
        }),
        `   ${JJ_LOG_EDGE_MARKER}`,
      ].join('\n');

      const { changes, graphRows } = parseJjLogGraph(raw);
      expect(changes).toHaveLength(4);

      expect(graphRows[0]).toMatchObject({
        type: 'commit',
        graphCols: [' ', ' ', '@'],
        downCols: [2],
      });

      // Merge commit emits downCols for upward-connecting glyphs and a connector row for ├─╮ │
      expect(graphRows[1]).toMatchObject({
        type: 'commit',
        graphCols: ['○', ' ', '│'],
        downCols: [0, 4],
      });
      expect(graphRows[2]).toMatchObject({
        type: 'connector',
        graphCols: ['├', '─', '╮', ' ', '│'],
        isElided: false,
        bridgesElided: false,
      });

      // Side branch commit & merge-back connector & bridging elision
      expect(graphRows[3]).toMatchObject({
        type: 'commit',
        graphCols: ['│', ' ', '○', ' ', '│'],
        downCols: [0, 2, 4],
      });
      expect(graphRows[4]).toMatchObject({
        type: 'connector',
        graphCols: ['├', '─', '╯', ' ', '│'],
        isElided: false,
        bridgesElided: false,
      });
      expect(graphRows[5]).toMatchObject({
        type: 'connector',
        graphCols: ['│', ' ', '~'],
        isElided: true,
        bridgesElided: true,
      });
      expect(graphRows[6]).toMatchObject({
        type: 'commit',
        graphCols: ['◆'],
        downCols: [],
      });
    });

    it('preserves leading spaces on row 0 in runJj for graph log calls', async () => {
      const tempDir = mkdtempSync(join(tmpdir(), 'tellit-jj-run-'));
      const originalPath = process.env.PATH;
      try {
        const mockJj = join(tempDir, 'jj-for-agents');
        writeFileSync(
          mockJj,
          [
            '#!/bin/sh',
            'printf "  @  __TELLIT_NODE__\\tabc\\n  │  __TELLIT_EDGE__\\n"',
            '',
          ].join('\n'),
          { mode: 0o755 },
        );
        process.env.PATH = `${tempDir}:${originalPath ?? ''}`;

        const service = new JujutsuService(tempDir);
        const graphOut = await service.runJj(['log', '--color=never', '-T', 'x']);
        expect(graphOut).toBe('  @  __TELLIT_NODE__\tabc\n  │  __TELLIT_EDGE__');

        const noGraphOut = await service.runJj(['log', '--no-graph', '-T', 'x']);
        expect(noGraphOut).toBe('@  __TELLIT_NODE__\tabc\n  │  __TELLIT_EDGE__');
      } finally {
        process.env.PATH = originalPath;
        rmSync(tempDir, { recursive: true, force: true });
      }
    });
  });

  describe('resolveInitialTargetCommit', () => {
    it('skips empty undescribed @ with zero comments and lands on @-', async () => {
      const atCommit = '1111111111111111111111111111111111111111';
      const parentCommit = '2222222222222222222222222222222222222222';
      vi.spyOn(jjService, 'runJj').mockResolvedValue(
        [
          `${atCommit}\taaaaaaaa\t1\t0\t`,
          `${parentCommit}\tbbbbbbbb\t0\t0\tImplement feature`,
        ].join('\n'),
      );
      const detailsSpy = vi
        .spyOn(jjService, 'getChangeDetails')
        .mockResolvedValue({ threads: [], approval: false });

      const resolved = await jjService.resolveInitialTargetCommit('HEAD');
      expect(resolved).toBe(parentCommit);
      expect(detailsSpy).toHaveBeenCalledWith('aaaaaaaa');
    });

    it('keeps @ when it is non-empty, even without a description', async () => {
      const atCommit = '1111111111111111111111111111111111111111';
      const parentCommit = '2222222222222222222222222222222222222222';
      vi.spyOn(jjService, 'runJj').mockResolvedValue(
        [
          `${atCommit}\taaaaaaaa\t0\t0`,
          `${parentCommit}\tbbbbbbbb\t0\t0\tParent`,
        ].join('\n'),
      );
      const detailsSpy = vi.spyOn(jjService, 'getChangeDetails');

      const resolved = await jjService.resolveInitialTargetCommit('@');
      expect(resolved).toBe(atCommit);
      expect(detailsSpy).not.toHaveBeenCalled();
    });

    it('keeps @ when it has a description, even if empty', async () => {
      const atCommit = '1111111111111111111111111111111111111111';
      vi.spyOn(jjService, 'runJj').mockResolvedValue(`${atCommit}\taaaaaaaa\t1\t0\tWIP: planned work`);

      const resolved = await jjService.resolveInitialTargetCommit('HEAD');
      expect(resolved).toBe(atCommit);
    });

    it('keeps empty undescribed @ when it already has review comments', async () => {
      const atCommit = '1111111111111111111111111111111111111111';
      const parentCommit = '2222222222222222222222222222222222222222';
      vi.spyOn(jjService, 'runJj').mockResolvedValue(
        [
          `${atCommit}\taaaaaaaa\t1\t0\t`,
          `${parentCommit}\tbbbbbbbb\t0\t0\tParent`,
        ].join('\n'),
      );
      vi.spyOn(jjService, 'getChangeDetails').mockResolvedValue({
        threads: [
          {
            thread_id: 't-1',
            change_id: 'aaaaaaaa',
            commit_id: atCommit,
            git_commit_hash: atCommit,
            filepath: 'README.md',
            comments: ['Comment on empty change'],
            resolved: false,
          },
        ],
        approval: false,
      });

      const resolved = await jjService.resolveInitialTargetCommit('HEAD');
      expect(resolved).toBe(atCommit);
    });

    it('resolves explicit change IDs and falls back to direct revset lookup', async () => {
      const targetCommit = '3333333333333333333333333333333333333333';
      const runJjSpy = vi.spyOn(jjService, 'runJj').mockImplementation(async (args) => {
        const revIdx = args.indexOf('-r');
        const rev = revIdx !== -1 ? args[revIdx + 1] : '';
        if (rev === 'change_id(kzvq)') {
          return targetCommit;
        }
        if (rev === 'change_id(main)') {
          throw new Error('invalid change id');
        }
        if (rev === 'main') {
          return '4444444444444444444444444444444444444444';
        }
        throw new Error('not found');
      });

      await expect(jjService.resolveInitialTargetCommit('kzvq')).resolves.toBe(targetCommit);
      await expect(jjService.resolveInitialTargetCommit('main')).resolves.toBe(
        '4444444444444444444444444444444444444444',
      );
      await expect(jjService.resolveInitialTargetCommit('missing-ref')).resolves.toBe('missing-ref');
      expect(runJjSpy).toHaveBeenCalled();
    });
  });
});
