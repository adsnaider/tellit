import { execFile } from 'child_process';
import { promisify } from 'util';
import { existsSync } from 'fs';
import { resolve, join } from 'path';

const execFileAsync = promisify(execFile);

export interface JjChange {
  changeId: string;
  shortChangeId: string;
  commitId: string;
  shortCommitId: string;
  isWorkingCopy: boolean;
  timestamp: string;
  description: string;
}

export interface JjCommit {
  commitId: string;
  shortCommitId: string;
  timestamp: string;
  description: string;
  isWorkingCopy: boolean;
  commentCount: number;
  unresolvedCommentCount: number;
}

export interface CommenterThreadInfo {
  thread_id: string;
  change_id: string;
  commit_id: string;
  git_commit_hash: string;
  filepath: string;
  line_range?: { start: number; end: number };
  comments: string[];
  resolved: boolean;
  side?: 'old' | 'new';
}

export class JujutsuService {
  private repoDir: string;
  private jjBin: string = 'jj-for-agents';
  private tellitBin: string;

  constructor(repoDir = process.cwd()) {
    this.repoDir = resolve(repoDir);
    this.tellitBin = this.resolveTellitBin();
  }

  private resolveTellitBin(): string {
    if (process.env.TELLIT_BIN && existsSync(process.env.TELLIT_BIN)) {
      return process.env.TELLIT_BIN;
    }
    // Look up from current directory to find target/debug/tellit or target/release/tellit
    let cur = this.repoDir;
    for (let i = 0; i < 5; i++) {
      const debugBin = join(cur, 'target', 'debug', 'tellit');
      if (existsSync(debugBin)) return debugBin;
      const releaseBin = join(cur, 'target', 'release', 'tellit');
      if (existsSync(releaseBin)) return releaseBin;
      const parent = resolve(cur, '..');
      if (parent === cur) break;
      cur = parent;
    }
    return 'tellit';
  }

  async runJj(args: string[]): Promise<string> {
    const effectiveArgs = args.includes('--ignore-working-copy')
      ? args
      : ['--ignore-working-copy', ...args];
    try {
      const { stdout } = await execFileAsync(this.jjBin, effectiveArgs, { cwd: this.repoDir });
      return stdout.trim();
    } catch {
      // Fallback to raw jj if jj-for-agents is not in PATH
      const { stdout } = await execFileAsync('jj', ['--no-pager', ...effectiveArgs], { cwd: this.repoDir });
      return stdout.trim();
    }
  }

  async runTellit(args: string[]): Promise<string> {
    const { stdout } = await execFileAsync(this.tellitBin, ['comment', ...args], { cwd: this.repoDir });
    return stdout.trim();
  }

  async isJjRepo(): Promise<boolean> {
    try {
      await this.runJj(['root']);
      return true;
    } catch {
      return false;
    }
  }

  async getRepoRoot(): Promise<string> {
    return await this.runJj(['root']);
  }

  async getChanges(): Promise<JjChange[]> {
    const template = 'change_id ++ "\t" ++ commit_id ++ "\t" ++ (if(current_working_copy, "1", "0")) ++ "\t" ++ committer.timestamp().format("%Y-%m-%d %H:%M:%S") ++ "\t" ++ description.first_line() ++ "\n"';
    // Run default jj log to match the active working stack/revisions
    const output = await this.runJj(['log', '--no-graph', '-T', template]);
    const lines = output.split('\n').filter(line => line.trim().length > 0);
    const seenChanges = new Set<string>();
    const changes: JjChange[] = [];

    for (const line of lines) {
      const parts = line.split('\t');
      if (parts.length < 5) continue;
      const [changeId, commitId, isWorkingCopy, timestamp, description] = parts;
      if (seenChanges.has(changeId) || changeId === '00000000000000000000000000000000') continue;
      seenChanges.add(changeId);

      changes.push({
        changeId,
        shortChangeId: changeId.slice(0, 8),
        commitId,
        shortCommitId: commitId.slice(0, 8),
        isWorkingCopy: isWorkingCopy === '1',
        timestamp,
        description: description || '(no description set)',
      });
    }

    return changes;
  }

  async getChange(query: string): Promise<JjChange | null> {
    const template = 'change_id ++ "\t" ++ commit_id ++ "\t" ++ (if(current_working_copy, "1", "0")) ++ "\t" ++ committer.timestamp().format("%Y-%m-%d %H:%M:%S") ++ "\t" ++ description.first_line() ++ "\n"';
    let output = '';
    try {
      output = await this.runJj(['log', '-r', `change_id(${query})`, '-n', '1', '--no-graph', '-T', template]);
    } catch {
      try {
        output = await this.runJj(['log', '-r', query, '-n', '1', '--no-graph', '-T', template]);
      } catch {
        return null;
      }
    }
    const lines = output.split('\n').filter(l => l.trim().length > 0);
    if (lines.length === 0) return null;
    const parts = lines[0].split('\t');
    if (parts.length < 5) return null;
    const [changeId, commitId, isWorkingCopy, timestamp, description] = parts;
    return {
      changeId,
      shortChangeId: changeId.slice(0, 8),
      commitId,
      shortCommitId: commitId.slice(0, 8),
      isWorkingCopy: isWorkingCopy === '1',
      timestamp,
      description: description || '(no description set)',
    };
  }

  async getChangeThreads(changeId: string): Promise<CommenterThreadInfo[]> {
    try {
      const output = await this.runTellit(['get', 'change', changeId, '--threads', '--json']);
      if (!output) return [];
      return JSON.parse(output);
    } catch {
      return [];
    }
  }

  async getCommitThreads(commitId: string): Promise<CommenterThreadInfo[]> {
    try {
      const output = await this.runTellit(['get', 'commit', commitId, '--threads', '--json']);
      if (!output) return [];
      return JSON.parse(output);
    } catch {
      return [];
    }
  }

  async getCommitsForChange(changeId: string): Promise<JjCommit[]> {
    // 1. Get visible commits from Jujutsu
    const template = 'commit_id ++ "\t" ++ committer.timestamp().format("%Y-%m-%d %H:%M:%S") ++ "\t" ++ (if(current_working_copy, "1", "0")) ++ "\t" ++ description.first_line() ++ "\n"';
    let jjCommitsRaw = '';
    try {
      jjCommitsRaw = await this.runJj(['log', '-r', `change_id(${changeId})`, '--reversed', '--no-graph', '-T', template]);
    } catch {
      // Ignored if revset fails
    }

    const commitMap = new Map<string, JjCommit>();
    for (const line of jjCommitsRaw.split('\n').filter(l => l.trim().length > 0)) {
      const parts = line.split('\t');
      if (parts.length < 4) continue;
      const [commitId, timestamp, isWorkingCopy, description] = parts;
      commitMap.set(commitId, {
        commitId,
        shortCommitId: commitId.slice(0, 8),
        timestamp,
        isWorkingCopy: isWorkingCopy === '1',
        description: description || '(no description set)',
        commentCount: 0,
        unresolvedCommentCount: 0,
      });
    }

    // 2. Fetch all threads for the change
    const changeThreads = await this.getChangeThreads(changeId);

    // 3. For any historical commit that has threads but isn't in jj log:
    for (const thread of changeThreads) {
      if (!commitMap.has(thread.commit_id)) {
        commitMap.set(thread.commit_id, {
          commitId: thread.commit_id,
          shortCommitId: thread.commit_id.slice(0, 8),
          timestamp: '',
          isWorkingCopy: false,
          description: '(historical revision)',
          commentCount: 0,
          unresolvedCommentCount: 0,
        });
      }
    }

    // 4. Populate comment counts
    for (const thread of changeThreads) {
      const commit = commitMap.get(thread.commit_id);
      if (commit) {
        commit.commentCount++;
        if (!thread.resolved) {
          commit.unresolvedCommentCount++;
        }
      }
    }

    // Convert map to array and sort chronologically (oldest to newest)
    const result = Array.from(commitMap.values());
    result.sort((a, b) => a.timestamp.localeCompare(b.timestamp));
    return result;
  }

  async getDiff(targetCommitId: string, baseCommitId?: string): Promise<string> {
    if (
      baseCommitId &&
      baseCommitId !== 'parent' &&
      baseCommitId !== `${targetCommitId}-` &&
      baseCommitId !== `${targetCommitId}^`
    ) {
      return await this.runJj(['diff', '--from', baseCommitId, '--to', targetCommitId, '--git']);
    }
    return await this.runJj(['diff', '-r', targetCommitId, '--git']);
  }

  async getDiffForCommit(commitId: string): Promise<string> {
    return await this.getDiff(commitId);
  }

  async getComments(commitId: string): Promise<CommenterThreadInfo[]> {
    return await this.getCommitThreads(commitId);
  }

  async getCommentsForDiff(params: {
    targetCommit: string;
    baseCommit?: string;
  }): Promise<(CommenterThreadInfo & { side: 'old' | 'new' })[]> {
    const targetInfo = await this.getChange(params.targetCommit);
    const targetCommitId = targetInfo?.commitId || params.targetCommit;
    const changeId = targetInfo?.changeId;

    let baseCommitId: string | undefined;
    if (
      params.baseCommit &&
      params.baseCommit !== 'parent' &&
      params.baseCommit !== `${params.targetCommit}-` &&
      params.baseCommit !== `${params.targetCommit}^`
    ) {
      const baseInfo = await this.getChange(params.baseCommit);
      baseCommitId = baseInfo?.commitId || params.baseCommit;
    }

    const resultMap = new Map<string, CommenterThreadInfo & { side: 'old' | 'new' }>();

    // If we have changeId, get all threads in change
    const allChangeThreads = changeId ? await this.getChangeThreads(changeId) : [];

    const commitMatches = (a: string, b: string) =>
      a === b || (a.length >= 6 && b.length >= 6 && (a.startsWith(b) || b.startsWith(a)));

    // 1. If diffing against base commit (Requirement 3):
    // All comments on baseCommit (resolved or unresolved) get side: 'old'
    if (baseCommitId) {
      const baseThreads = allChangeThreads.length > 0
        ? allChangeThreads.filter(t => commitMatches(t.commit_id, baseCommitId!))
        : await this.getCommitThreads(baseCommitId);

      for (const t of baseThreads) {
        resultMap.set(t.thread_id, { ...t, side: 'old' });
      }
    }

    // 2. Comments on targetCommit -> side: 'new'
    const targetThreads = allChangeThreads.length > 0
      ? allChangeThreads.filter(t => commitMatches(t.commit_id, targetCommitId))
      : await this.getCommitThreads(targetCommitId);

    for (const t of targetThreads) {
      resultMap.set(t.thread_id, { ...t, side: 'new' });
    }

    // 3. Any other unresolved comments in the change (Requirement 2):
    // When stepping to a later commit in the same change, unresolved comments
    // from earlier commits are carried forward so open comments don't disappear.
    for (const t of allChangeThreads) {
      if (!t.resolved && !resultMap.has(t.thread_id)) {
        resultMap.set(t.thread_id, { ...t, side: 'new' });
      }
    }

    return Array.from(resultMap.values());
  }

  async addComment(params: {
    filePath: string;
    lineRange?: { start?: number; end?: number } | number | string;
    content: string;
    commitId: string;
  }): Promise<string> {
    const args = ['add', '--file', params.filePath, '--content', params.content, '-r', params.commitId];
    if (params.lineRange !== undefined && params.lineRange !== null) {
      if (typeof params.lineRange === 'number') {
        args.push('--at', `${params.lineRange}`);
      } else if (typeof params.lineRange === 'string') {
        const cleaned = params.lineRange.replace('-', '..');
        args.push('--at', cleaned);
      } else if (typeof params.lineRange === 'object') {
        const start = params.lineRange.start;
        const end = params.lineRange.end ?? start;
        if (typeof start === 'number') {
          if (start === end || end === undefined) {
            args.push('--at', `${start}`);
          } else {
            args.push('--at', `${start}..${end}`);
          }
        }
      }
    }
    const output = await this.runTellit(args);
    const firstLine = output.trim().split('\n')[0].trim();
    return firstLine;
  }

  async replyComment(threadId: string, content: string): Promise<string> {
    return await this.runTellit(['reply', '-t', threadId, '-c', content]);
  }

  async resolveComment(threadId: string, unresolve = false): Promise<string> {
    const args = ['resolve', '-t', threadId];
    if (unresolve) {
      args.push('--unresolve');
    }
    return await this.runTellit(args);
  }
}
