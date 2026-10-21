import { describe, it, expect, vi, beforeEach } from 'vitest';
import { JujutsuService, CommenterThreadInfo } from './jujutsu';

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
    });

    vi.spyOn(jjService, 'getChangeThreads').mockResolvedValue([
      unresolvedOnEarlierCommit,
      resolvedOnEarlierCommit,
      threadOnCurrentCommit,
    ]);

    const result = await jjService.getCommentsForDiff({
      targetCommit: 'commit-current-bbb',
    });

    expect(result).toHaveLength(2);
    const carriedOver = result.find((t) => t.thread_id === 't-earlier-unresolved');
    const current = result.find((t) => t.thread_id === 't-current-new');
    const oldResolved = result.find((t) => t.thread_id === 't-earlier-resolved');

    expect(carriedOver).toBeDefined();
    expect(carriedOver?.side).toBe('new');
    expect(carriedOver?.resolved).toBe(false);

    expect(current).toBeDefined();
    expect(current?.side).toBe('new');

    expect(oldResolved).toBeUndefined();
  });
});
