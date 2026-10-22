import { describe, it, expect, vi, beforeEach } from 'vitest';
import { JujutsuService, CommenterThreadInfo } from './jujutsu';
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
      const file = createCommitMessageDiffFile('', 'feat: add user login\n\nFirst line\nSecond line');
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
});
