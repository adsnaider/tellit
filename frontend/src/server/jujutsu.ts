import { execFile } from 'child_process';
import { promisify } from 'util';
import { existsSync, readdirSync, readFileSync, realpathSync, statSync } from 'fs';
import { dirname, join, resolve } from 'path';
import { type JjChange, type JjCommit, type JjGraphRow } from '../types/diff.js';

export type { JjChange, JjCommit, JjGraphRow };

const execFileAsync = promisify(execFile);

export const JJ_LOG_NODE_MARKER = '__TELLIT_NODE__';
export const JJ_LOG_EDGE_MARKER = '__TELLIT_EDGE__';

const ROOT_CHANGE_ID = '00000000000000000000000000000000';
const VERTICAL_EDGE_PREFIX_RE = /^[│|: ]*$/;
const PURE_SPACER_LINE_RE = /^[│|: ]+$/;
const ELIDED_REVISIONS_SUFFIX_RE = /\s*\(elided revisions\).*$/;
const TRAILING_SPACES_RE = /\s+$/;
const TRAILING_NEWLINES_RE = /[\r\n]+$/;
const LINE_SPLIT_RE = /\r?\n/;
const UPWARD_CONNECTING_GLYPHS = '│|:╯┘\'╰└`├┤┴┼+';

const JJ_CHANGE_FIELDS_TEMPLATE =
  `change_id ++ "\t" ++ change_id.shortest() ++ "\t" ++ commit_id ++ "\t" ++ ` +
  `if(current_working_copy, "1", "0") ++ "\t" ++ if(empty, "1", "0") ++ "\t" ++ ` +
  `if(immutable, "1", "0") ++ "\t" ++ if(conflict, "1", "0") ++ "\t" ++ if(root, "1", "0") ++ "\t" ++ ` +
  `bookmarks.map(|b| b.name()).join(",") ++ "\t" ++ parents.map(|p| p.commit_id()).join(",") ++ "\t" ++ ` +
  `committer.timestamp().format("%Y-%m-%d %H:%M:%S") ++ "\t" ++ description.first_line()`;

const JJ_GRAPH_LOG_TEMPLATE =
  `"${JJ_LOG_NODE_MARKER}\t" ++ ${JJ_CHANGE_FIELDS_TEMPLATE} ++ "\n${JJ_LOG_EDGE_MARKER}\n"`;

function parseJjChangeFields(parts: string[]): { change: JjChange; isRoot: boolean } | null {
  if (parts.length < 11) {
    return null;
  }
  const changeId = parts[0] ?? '';
  const shortestChangeId = parts[1] ?? '';
  const commitId = parts[2] ?? '';
  const isWorkingCopyRaw = parts[3] ?? '0';
  const isEmptyRaw = parts[4] ?? '0';
  const isImmutableRaw = parts[5] ?? '0';
  const isConflictRaw = parts[6] ?? '0';
  const isRootRaw = parts[7] ?? '0';
  const bookmarksRaw = parts[8] ?? '';
  const parentCommitIdsRaw = parts[9] ?? '';
  const timestamp = parts[10] ?? '';
  const rawDescription = parts.slice(11).join('\t').trim();

  if (!changeId || !commitId) {
    return null;
  }

  const isRoot = isRootRaw === '1' || changeId === ROOT_CHANGE_ID;
  const change: JjChange = {
    changeId,
    shortChangeId: changeId.slice(0, 8),
    shortestChangeId: shortestChangeId || changeId.slice(0, 1),
    commitId,
    shortCommitId: commitId.slice(0, 8),
    isWorkingCopy: isWorkingCopyRaw === '1',
    isEmpty: isEmptyRaw === '1',
    hasDescription: rawDescription.length > 0,
    isImmutable: isImmutableRaw === '1',
    isConflict: isConflictRaw === '1',
    bookmarks: bookmarksRaw ? bookmarksRaw.split(',').filter(Boolean) : [],
    parentCommitIds: parentCommitIdsRaw ? parentCommitIdsRaw.split(',').filter(Boolean) : [],
    timestamp,
    description: rawDescription || '(no description set)',
    commentCount: 0,
    unresolvedCommentCount: 0,
  };

  return { change, isRoot };
}

/**
 * Parses `jj log` graph output emitted with `JJ_GRAPH_LOG_TEMPLATE` into
 * ordered change entries and graph rows with downward stem columns.
 */
export function parseJjLogGraph(rawOutput: string): {
  changes: JjChange[];
  graphRows: JjGraphRow[];
} {
  const lines = rawOutput.split(LINE_SPLIT_RE);
  const changes: JjChange[] = [];
  const graphRows: JjGraphRow[] = [];

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (!line || line.trim().length === 0) {
      continue;
    }

    const nodeIdx = line.indexOf(JJ_LOG_NODE_MARKER);
    if (nodeIdx !== -1) {
      const rawPrefix = line.slice(0, nodeIdx);
      const graphCols = Array.from(rawPrefix.replace(TRAILING_SPACES_RE, ''));
      const parts = line.slice(nodeIdx).split('\t').slice(1);
      const parsed = parseJjChangeFields(parts);

      const nextLine = lines[i + 1] ?? '';
      const edgeIdx = nextLine.indexOf(JJ_LOG_EDGE_MARKER);

      if (!parsed || parsed.isRoot) {
        if (edgeIdx !== -1) {
          i++;
        }
        continue;
      }

      const { change } = parsed;
      changes.push(change);

      const downCols: number[] = [];
      let edgeConnectorCols: string[] | null = null;

      if (edgeIdx !== -1) {
        i++;
        const edgePrefix = nextLine.slice(0, edgeIdx).replace(TRAILING_SPACES_RE, '');
        const edgeChars = Array.from(edgePrefix);
        if (VERTICAL_EDGE_PREFIX_RE.test(edgePrefix)) {
          for (let col = 0; col < edgeChars.length; col++) {
            const ch = edgeChars[col];
            if (ch === '│' || ch === '|' || ch === ':') {
              downCols.push(col);
            }
          }
        } else {
          for (let col = 0; col < edgeChars.length; col++) {
            const ch = edgeChars[col];
            if (ch && UPWARD_CONNECTING_GLYPHS.includes(ch)) {
              downCols.push(col);
            }
          }
          edgeConnectorCols = edgeChars;
        }
      }

      graphRows.push({
        type: 'commit',
        key: change.commitId,
        graphCols,
        downCols,
        change,
      });

      if (edgeConnectorCols) {
        graphRows.push({
          type: 'connector',
          key: `connector-${graphRows.length}`,
          graphCols: edgeConnectorCols,
          isElided: false,
          bridgesElided: false,
        });
      }
      continue;
    }

    if (line.includes(JJ_LOG_EDGE_MARKER)) {
      continue;
    }

    const bridgesElided = line.includes('(elided revisions)');
    const trimmedRight = line
      .replace(ELIDED_REVISIONS_SUFFIX_RE, '')
      .replace(TRAILING_SPACES_RE, '');
    if (!trimmedRight || (!bridgesElided && PURE_SPACER_LINE_RE.test(trimmedRight))) {
      continue;
    }

    graphRows.push({
      type: 'connector',
      key: `connector-${graphRows.length}`,
      graphCols: Array.from(trimmedRight),
      isElided: trimmedRight.includes('~'),
      bridgesElided,
    });
  }

  // Filter downward stems that have no connecting glyph in the next row
  // (e.g. when the root commit or trailing spacer lines were dropped).
  for (let r = 0; r < graphRows.length; r++) {
    const row = graphRows[r];
    if (!row || row.type !== 'commit') {
      continue;
    }
    if (r === graphRows.length - 1) {
      row.downCols = [];
      continue;
    }
    const nextRow = graphRows[r + 1];
    row.downCols = row.downCols.filter((col) => (nextRow?.graphCols[col] ?? ' ') !== ' ');
  }

  return { changes, graphRows };
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

export interface JjDiffResult {
  rawDiff: string;
  isRebaseDiff: boolean;
  rebaseIgnored: boolean;
  rebaseConflict: boolean;
}

export interface ChangeDetails {
  threads: CommenterThreadInfo[];
  approval: boolean;
  approvedCommits?: string[];
}

function commitMatches(a: string, b: string): boolean {
  return a === b || (a.length >= 6 && b.length >= 6 && (a.startsWith(b) || b.startsWith(a)));
}

export class JujutsuService {
  private repoDir: string;
  private jjBin: string = 'jj-for-agents';
  private tellitBin: string;
  private cachedGitDir: string | null | undefined = undefined;

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
    const preserveLeading = args.includes('log') && !args.includes('--no-graph');
    const cleanOutput = (stdout: string) =>
      preserveLeading ? stdout.replace(TRAILING_NEWLINES_RE, '') : stdout.trim();
    try {
      const { stdout } = await execFileAsync(this.jjBin, effectiveArgs, { cwd: this.repoDir });
      return cleanOutput(stdout);
    } catch {
      // Fallback to raw jj if jj-for-agents is not in PATH
      const { stdout } = await execFileAsync('jj', ['--no-pager', ...effectiveArgs], { cwd: this.repoDir });
      return cleanOutput(stdout);
    }
  }

  async runTellit(args: string[]): Promise<string> {
    try {
      const { stdout } = await execFileAsync(this.tellitBin, ['comment', ...args], {
        cwd: this.repoDir,
      });
      return stdout.trim();
    } catch (error: unknown) {
      const rawStderr =
        typeof error === 'object' && error !== null && 'stderr' in error ? error.stderr : undefined;
      const stderr =
        typeof rawStderr === 'string'
          ? rawStderr.trim()
          : Buffer.isBuffer(rawStderr)
            ? rawStderr.toString('utf8').trim()
            : '';
      if (stderr) {
        throw new Error(stderr.replace(/^Error:\s*/, ''), { cause: error });
      }
      throw error instanceof Error ? error : new Error('Failed to run tellit');
    }
  }

  private resolveJjRepoPaths(): { mainRepoRoot: string; jjRepoDir: string } | null {
    let cur = this.repoDir;
    while (true) {
      const jjDir = join(cur, '.jj');
      const jjRepoPath = join(jjDir, 'repo');
      if (existsSync(jjRepoPath)) {
        try {
          const st = statSync(jjRepoPath);
          if (st.isFile()) {
            const target = readFileSync(jjRepoPath, 'utf8').trim();
            if (!target) return null;
            const resolvedRepo = resolve(jjDir, target);
            const canonicalRepo = existsSync(resolvedRepo) ? realpathSync(resolvedRepo) : resolvedRepo;
            return {
              jjRepoDir: canonicalRepo,
              mainRepoRoot: dirname(dirname(canonicalRepo)),
            };
          }
          if (st.isDirectory()) {
            return {
              jjRepoDir: jjRepoPath,
              mainRepoRoot: cur,
            };
          }
        } catch {
          return null;
        }
      }
      const parent = dirname(cur);
      if (parent === cur) break;
      cur = parent;
    }
    return null;
  }

  private resolveGitDir(): string | null {
    if (this.cachedGitDir !== undefined) {
      return this.cachedGitDir;
    }

    let cur = this.repoDir;
    while (true) {
      if (existsSync(join(cur, '.git'))) {
        this.cachedGitDir = null;
        return null;
      }
      const parent = dirname(cur);
      if (parent === cur) break;
      cur = parent;
    }

    const jjPaths = this.resolveJjRepoPaths();
    if (jjPaths) {
      const storeDir = join(jjPaths.jjRepoDir, 'store');
      const directGit = join(storeDir, 'git');
      if (existsSync(directGit)) {
        this.cachedGitDir = directGit;
        return directGit;
      }
      const gitTargetFile = join(storeDir, 'git_target');
      if (existsSync(gitTargetFile)) {
        try {
          const target = readFileSync(gitTargetFile, 'utf8').trim();
          if (target) {
            const resolvedGit = resolve(storeDir, target);
            if (existsSync(resolvedGit)) {
              this.cachedGitDir = resolvedGit;
              return resolvedGit;
            }
          }
        } catch {
          // Ignore unreadable git_target
        }
      }
    }

    this.cachedGitDir = null;
    return null;
  }

  private isApprovalFileApproved(approvalFile: string): boolean {
    if (!existsSync(approvalFile)) {
      return false;
    }
    try {
      if (!statSync(approvalFile).isFile()) {
        return false;
      }
      const raw = readFileSync(approvalFile, 'utf8');
      try {
        const parsed: unknown = JSON.parse(raw);
        if (typeof parsed === 'object' && parsed !== null && 'approved' in parsed) {
          return Boolean((parsed as { approved?: unknown }).approved);
        }
        return true;
      } catch {
        // Non-JSON marker files represent legacy unrevoked approvals.
        return true;
      }
    } catch {
      return false;
    }
  }

  private findTellitStoreDir(): string | null {
    const jjPaths = this.resolveJjRepoPaths();
    const candidateRoots = jjPaths?.mainRepoRoot
      ? [join(jjPaths.mainRepoRoot, '.tellit'), join(this.repoDir, '.tellit')]
      : [join(this.repoDir, '.tellit')];
    return candidateRoots.find((dir) => existsSync(dir)) ?? null;
  }

  private readStoreApprovedCommits(changeId: string): string[] {
    const storeDir = this.findTellitStoreDir();
    if (!storeDir) {
      return [];
    }

    let exactMatch: string | null = null;
    const matchedChangeDirs: string[] = [];
    try {
      const entries = readdirSync(storeDir, { withFileTypes: true });
      for (const entry of entries) {
        if (!entry.isDirectory()) continue;
        const name = entry.name;
        if (name === changeId) {
          const path = join(storeDir, name);
          exactMatch = path;
          matchedChangeDirs.push(path);
        } else if (
          name.startsWith(changeId) ||
          (name.length >= 6 && changeId.startsWith(name))
        ) {
          matchedChangeDirs.push(join(storeDir, name));
        }
      }
    } catch {
      return [];
    }

    const dirsToScan = changeId.length < 6 && exactMatch ? [exactMatch] : matchedChangeDirs;

    const approved = new Set<string>();
    for (const changeDir of dirsToScan) {
      try {
        const commitEntries = readdirSync(changeDir, { withFileTypes: true });
        for (const commitEntry of commitEntries) {
          if (!commitEntry.isDirectory()) continue;
          const approvalFile = join(changeDir, commitEntry.name, 'approval.json');
          if (this.isApprovalFileApproved(approvalFile)) {
            approved.add(commitEntry.name);
          }
        }
      } catch {
        // Ignore unreadable change directory
      }
    }

    return Array.from(approved).sort();
  }

  private isStoreCommitApproved(commitId: string): boolean {
    const storeDir = this.findTellitStoreDir();
    if (!storeDir) {
      return false;
    }

    try {
      const changeEntries = readdirSync(storeDir, { withFileTypes: true });
      for (const changeDir of changeEntries) {
        if (!changeDir.isDirectory()) continue;
        const changePath = join(storeDir, changeDir.name);
        try {
          const commitEntries = readdirSync(changePath, { withFileTypes: true });
          for (const commitDir of commitEntries) {
            if (!commitDir.isDirectory()) continue;
            if (
              commitMatches(commitDir.name, commitId) &&
              this.isApprovalFileApproved(join(changePath, commitDir.name, 'approval.json'))
            ) {
              return true;
            }
          }
        } catch {
          // Ignore unreadable change directory
        }
      }
    } catch {
      return false;
    }

    return false;
  }

  async runGit(args: string[]): Promise<{ stdout: string; stderr: string; exitCode: number }> {
    const gitDir = this.resolveGitDir();
    try {
      const { stdout, stderr } = await execFileAsync('git', args, {
        cwd: this.repoDir,
        maxBuffer: 20 * 1024 * 1024,
        ...(gitDir ? { env: { ...process.env, GIT_DIR: gitDir } } : {}),
      });
      return { stdout: stdout.toString(), stderr: stderr.toString(), exitCode: 0 };
    } catch (error: unknown) {
      const errObj =
        typeof error === 'object' && error !== null
          ? (error as { stdout?: unknown; stderr?: unknown; code?: unknown })
          : {};
      const toStr = (val: unknown) =>
        typeof val === 'string' ? val : Buffer.isBuffer(val) ? val.toString('utf8') : '';
      return {
        stdout: toStr(errObj.stdout),
        stderr: toStr(errObj.stderr),
        exitCode: typeof errObj.code === 'number' ? errObj.code : 1,
      };
    }
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

  private async getParentsForCommit(
    commitId: string,
    parentCache?: Map<string, Promise<string[]>>,
  ): Promise<string[]> {
    const loadParents = async (): Promise<string[]> => {
      try {
        const pOut = await this.runJj([
          'log',
          '-r',
          commitId,
          '--no-graph',
          '-T',
          'parents.map(|p| p.commit_id()).join(",")',
        ]);
        return pOut.trim().split(',').filter(Boolean);
      } catch {
        return [];
      }
    };

    if (!parentCache) {
      return loadParents();
    }
    let pending = parentCache.get(commitId);
    if (!pending) {
      pending = loadParents();
      parentCache.set(commitId, pending);
    }
    return pending;
  }

  private async isCleanRebaseBetweenCommits(params: {
    earlierCommitId: string;
    earlierParentId: string;
    targetCommitId: string;
    targetParentId: string;
    earlierDescription?: string;
    targetDescription?: string;
  }): Promise<boolean> {
    const {
      earlierCommitId,
      earlierParentId,
      targetCommitId,
      targetParentId,
      earlierDescription,
      targetDescription,
    } = params;

    try {
      if (earlierParentId !== targetParentId) {
        const mergeResult = await this.runGit([
          'merge-tree',
          '--write-tree',
          '--merge-base',
          earlierParentId,
          targetParentId,
          earlierCommitId,
        ]);
        if (mergeResult.exitCode !== 0) {
          return false;
        }
        const virtualTreeOid = mergeResult.stdout.trim().split('\n')[0]?.trim() ?? '';
        if (virtualTreeOid.length !== 40 && virtualTreeOid.length !== 64) {
          return false;
        }
        const diffResult = await this.runGit([
          'diff',
          '--no-color',
          '--no-ext-diff',
          virtualTreeOid,
          targetCommitId,
        ]);
        return diffResult.exitCode === 0 && diffResult.stdout.trim().length === 0;
      }

      const diffResult = await this.runGit([
        'diff',
        '--no-color',
        '--no-ext-diff',
        earlierCommitId,
        targetCommitId,
      ]);
      if (diffResult.exitCode !== 0 || diffResult.stdout.trim().length !== 0) {
        return false;
      }
      const [earlierDesc, targetDesc] = await Promise.all([
        earlierDescription !== undefined
          ? Promise.resolve(earlierDescription)
          : this.getCommitDescription(earlierCommitId),
        targetDescription !== undefined
          ? Promise.resolve(targetDescription)
          : this.getCommitDescription(targetCommitId),
      ]);
      return earlierDesc === targetDesc;
    } catch (err) {
      console.warn('Failed to evaluate clean rebase approval:', err);
      return false;
    }
  }

  private async populateChangeReviewState(
    change: JjChange,
    details: ChangeDetails,
    parentCache?: Map<string, Promise<string[]>>,
  ): Promise<void> {
    change.commentCount = details.threads.length;
    change.unresolvedCommentCount = details.threads.filter((t) => !t.resolved).length;

    const approvedCommits =
      details.approvedCommits !== undefined
        ? details.approvedCommits
        : this.readStoreApprovedCommits(change.changeId);

    const markDirectlyApproved = () => {
      change.approved = true;
      delete change.cleanRebaseApproved;
    };

    if (
      approvedCommits.some((id) => commitMatches(id, change.commitId)) ||
      this.isStoreCommitApproved(change.commitId)
    ) {
      markDirectlyApproved();
      return;
    }

    if (details.approval) {
      if (details.approvedCommits === undefined && approvedCommits.length === 0) {
        markDirectlyApproved();
        return;
      }
      const commitDetails = await this.getCommitDetails(change.commitId);
      if (commitDetails.approval) {
        markDirectlyApproved();
        return;
      }
    }

    change.approved = false;
    delete change.cleanRebaseApproved;

    if (approvedCommits.length === 0) {
      return;
    }

    const targetParents =
      change.parentCommitIds !== undefined
        ? change.parentCommitIds
        : await this.getParentsForCommit(change.commitId, parentCache);
    if (targetParents.length !== 1) {
      return;
    }
    const targetParentId = targetParents[0];
    if (!targetParentId) {
      return;
    }

    for (const earlierCommitId of [...approvedCommits].reverse()) {
      const earlierParents = await this.getParentsForCommit(earlierCommitId, parentCache);
      if (earlierParents.length !== 1) {
        continue;
      }
      const earlierParentId = earlierParents[0];
      if (!earlierParentId) {
        continue;
      }
      if (
        await this.isCleanRebaseBetweenCommits({
          earlierCommitId,
          earlierParentId,
          targetCommitId: change.commitId,
          targetParentId,
        })
      ) {
        change.cleanRebaseApproved = true;
        return;
      }
    }
  }

  async getChangesGraph(): Promise<{ changes: JjChange[]; graphRows: JjGraphRow[] }> {
    const output = await this.runJj(['log', '--color=never', '-T', JJ_GRAPH_LOG_TEMPLATE]);
    const { changes, graphRows } = parseJjLogGraph(output);

    const detailsByChangeId = new Map<string, Promise<ChangeDetails>>();
    const getDetails = (changeId: string) => {
      let pending = detailsByChangeId.get(changeId);
      if (!pending) {
        pending = this.getChangeDetails(changeId);
        detailsByChangeId.set(changeId, pending);
      }
      return pending;
    };

    const parentCache = new Map<string, Promise<string[]>>();
    await Promise.all(
      changes.map(async (c) => {
        const details = await getDetails(c.changeId);
        await this.populateChangeReviewState(c, details, parentCache);
      }),
    );

    return { changes, graphRows };
  }

  async getChanges(): Promise<JjChange[]> {
    const { changes } = await this.getChangesGraph();
    return changes;
  }

  async getChange(query: string): Promise<JjChange | null> {
    const template = `${JJ_CHANGE_FIELDS_TEMPLATE} ++ "\n"`;
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
    const lines = output.split(LINE_SPLIT_RE).filter((l) => l.trim().length > 0);
    const firstLine = lines[0];
    if (!firstLine) return null;
    const parsed = parseJjChangeFields(firstLine.split('\t'));
    if (!parsed) return null;
    const { change } = parsed;
    const details = await this.getChangeDetails(change.changeId);
    await this.populateChangeReviewState(change, details);
    return change;
  }

  /**
   * Resolves the initial target commit ID for CLI launch. When given `HEAD` or
   * `@`, skips empty undescribed commits with no review comments so launching
   * from a fresh working copy lands on the latest meaningful change.
   */
  async resolveInitialTargetCommit(commitish: string): Promise<string> {
    if (commitish === 'HEAD' || commitish === '@') {
      const template =
        'commit_id ++ "\t" ++ change_id ++ "\t" ++ if(empty, "1", "0") ++ "\t" ++ if(root, "1", "0") ++ "\t" ++ description.first_line() ++ "\n"';
      try {
        const output = await this.runJj(['log', '-r', 'ancestors(@, 20)', '--no-graph', '-T', template]);
        const lines = output.split(LINE_SPLIT_RE).filter((l) => l.trim().length > 0);
        let firstNonRootCommitId: string | undefined;

        for (const line of lines) {
          const parts = line.split('\t');
          if (parts.length < 4) continue;
          const commitId = parts[0] ?? '';
          const changeId = parts[1] ?? '';
          const isEmpty = parts[2] ?? '0';
          const isRoot = parts[3] ?? '0';
          const rawDescription = parts.slice(4).join('\t').trim();

          if (!commitId || !changeId || isRoot === '1' || changeId === ROOT_CHANGE_ID) {
            continue;
          }
          if (!firstNonRootCommitId) {
            firstNonRootCommitId = commitId;
          }
          if (!(isEmpty === '1' && rawDescription === '')) {
            return commitId;
          }
          const details = await this.getChangeDetails(changeId);
          if (details.threads.length > 0) {
            return commitId;
          }
        }

        return firstNonRootCommitId || commitish;
      } catch {
        return commitish;
      }
    }

    try {
      const byChangeId = await this.runJj([
        'log',
        '-r',
        `change_id(${commitish})`,
        '-n',
        '1',
        '--no-graph',
        '-T',
        'commit_id',
      ]);
      if (byChangeId.trim()) {
        return byChangeId.trim();
      }
    } catch {
      // Fall back to direct revset lookup
    }

    try {
      const direct = await this.runJj([
        'log',
        '-r',
        commitish,
        '-n',
        '1',
        '--no-graph',
        '-T',
        'commit_id',
      ]);
      if (direct.trim()) {
        return direct.trim();
      }
    } catch {
      // Fall back to original commitish
    }

    return commitish;
  }

  private parseDetailsOutput(output: string): ChangeDetails {
    if (!output) return { threads: [], approval: false };
    const parsed: unknown = JSON.parse(output);
    if (Array.isArray(parsed)) {
      return { threads: parsed as CommenterThreadInfo[], approval: false };
    }
    const obj =
      typeof parsed === 'object' && parsed !== null
        ? (parsed as {
            threads?: unknown;
            approval?: unknown;
            approved_commits?: unknown;
          })
        : {};
    return {
      threads: Array.isArray(obj.threads) ? (obj.threads as CommenterThreadInfo[]) : [],
      approval: Boolean(obj.approval),
      ...(Array.isArray(obj.approved_commits)
        ? {
            approvedCommits: obj.approved_commits.filter(
              (s: unknown): s is string => typeof s === 'string',
            ),
          }
        : {}),
    };
  }

  async getChangeDetails(changeId: string): Promise<ChangeDetails> {
    try {
      const output = await this.runTellit(['get', 'change', changeId, '--threads', '--json']);
      return this.parseDetailsOutput(output);
    } catch {
      return { threads: [], approval: false };
    }
  }

  async getChangeThreads(changeId: string): Promise<CommenterThreadInfo[]> {
    const details = await this.getChangeDetails(changeId);
    return details.threads;
  }

  async getCommitDetails(commitId: string): Promise<{ threads: CommenterThreadInfo[]; approval: boolean }> {
    try {
      const output = await this.runTellit(['get', 'commit', commitId, '--threads', '--json']);
      const { threads, approval } = this.parseDetailsOutput(output);
      return { threads, approval };
    } catch {
      return { threads: [], approval: false };
    }
  }

  async getCommitThreads(commitId: string): Promise<CommenterThreadInfo[]> {
    const details = await this.getCommitDetails(commitId);
    return details.threads;
  }

  async getCommitsForChange(changeId: string): Promise<JjCommit[]> {
    // 1. Get all commits for the change using Jujutsu evolution log (evolog)
    const evologTemplate =
      'commit.commit_id() ++ "\t" ++ commit.committer().timestamp().format("%Y-%m-%d %H:%M:%S") ++ "\t" ++ (if(commit.current_working_copy(), "1", "0")) ++ "\t" ++ commit.description().first_line() ++ "\t" ++ commit.parents().map(|p| p.commit_id()).join(",") ++ "\n"';
    let jjCommitsRaw = '';
    try {
      jjCommitsRaw = await this.runJj([
        'evolog',
        '-r',
        `change_id(${changeId})`,
        '--reversed',
        '--no-graph',
        '-T',
        evologTemplate,
      ]);
    } catch {
      try {
        jjCommitsRaw = await this.runJj([
          'evolog',
          '-r',
          changeId,
          '--reversed',
          '--no-graph',
          '-T',
          evologTemplate,
        ]);
      } catch {
        // Fallback to standard jj log if evolog fails
        const fallbackTemplate =
          'commit_id ++ "\t" ++ committer.timestamp().format("%Y-%m-%d %H:%M:%S") ++ "\t" ++ (if(current_working_copy, "1", "0")) ++ "\t" ++ description.first_line() ++ "\t" ++ parents.map(|p| p.commit_id()).join(",") ++ "\n"';
        try {
          jjCommitsRaw = await this.runJj([
            'log',
            '-r',
            `change_id(${changeId})`,
            '--reversed',
            '--no-graph',
            '-T',
            fallbackTemplate,
          ]);
        } catch {
          // Ignored
        }
      }
    }

    const evologCommits = new Map<string, JjCommit>();
    const commitParentsCache = new Map<string, Promise<string[]>>();
    for (const line of jjCommitsRaw.split('\n').filter((l) => l.trim().length > 0)) {
      const parts = line.split('\t');
      if (parts.length < 4) continue;
      const [commitId, timestamp, isWorkingCopy, description, parentsRaw] = parts;
      if (!commitId) continue;
      if (parentsRaw) {
        commitParentsCache.set(commitId, Promise.resolve(parentsRaw.split(',').filter(Boolean)));
      }
      if (evologCommits.has(commitId)) {
        evologCommits.delete(commitId);
      }
      evologCommits.set(commitId, {
        commitId,
        shortCommitId: commitId.slice(0, 8),
        timestamp: timestamp ?? '',
        isWorkingCopy: isWorkingCopy === '1',
        description: description || '(no description set)',
        commentCount: 0,
        unresolvedCommentCount: 0,
      });
    }

    // 2. Fetch all threads for the change
    const changeThreads = await this.getChangeThreads(changeId);

    const historicalCommits = new Map<string, JjCommit>();
    const findMatchingCommit = (cid: string): JjCommit | undefined => {
      const exact = evologCommits.get(cid) ?? historicalCommits.get(cid);
      if (exact) {
        return exact;
      }
      for (const commit of evologCommits.values()) {
        if (commitMatches(commit.commitId, cid)) {
          return commit;
        }
      }
      for (const commit of historicalCommits.values()) {
        if (commitMatches(commit.commitId, cid)) {
          return commit;
        }
      }
      return undefined;
    };

    // 3. For any historical commit that has threads but isn't in jj log:
    for (const thread of changeThreads) {
      if (!findMatchingCommit(thread.commit_id)) {
        historicalCommits.set(thread.commit_id, {
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
      const commit = findMatchingCommit(thread.commit_id);
      if (commit) {
        commit.commentCount++;
        if (!thread.resolved) {
          commit.unresolvedCommentCount++;
        }
      }
    }

    // Preserve causal evolog order (prepending historical non-evolog commits)
    const result = [...historicalCommits.values(), ...evologCommits.values()];

    await Promise.all(
      result.map(async (c) => {
        const details = await this.getCommitDetails(c.commitId);
        c.approved = details.approval;
      }),
    );

    // 5. Detect clean rebase approvals chronologically
    for (let i = 0; i < result.length; i++) {
      const current = result[i];
      if (!current || current.approved) {
        continue;
      }
      const currentParents = await this.getParentsForCommit(current.commitId, commitParentsCache);
      if (currentParents.length !== 1) {
        continue;
      }
      const pi = currentParents[0];
      if (!pi) {
        continue;
      }

      // Check against earlier approved commits (starting from most recent approved)
      for (let j = i - 1; j >= 0; j--) {
        const earlier = result[j];
        if (!earlier || (!earlier.approved && !earlier.cleanRebaseApproved)) {
          continue;
        }
        const earlierParents = await this.getParentsForCommit(earlier.commitId, commitParentsCache);
        if (earlierParents.length !== 1) {
          continue;
        }
        const pj = earlierParents[0];
        if (!pj) {
          continue;
        }

        if (
          await this.isCleanRebaseBetweenCommits({
            earlierCommitId: earlier.commitId,
            earlierParentId: pj,
            targetCommitId: current.commitId,
            targetParentId: pi,
          })
        ) {
          current.cleanRebaseApproved = true;
          current.cleanRebaseSourceCommitId =
            earlier.cleanRebaseSourceCommitId || earlier.commitId;
          current.cleanRebaseSourceCommitNumber =
            earlier.cleanRebaseSourceCommitNumber || (j + 1);
          break;
        }
      }
    }

    return result;
  }

  async getCommitDescription(rev: string): Promise<string> {
    try {
      return await this.runJj(['log', '-r', rev, '--no-graph', '-T', 'description']);
    } catch {
      return '';
    }
  }

  async getDiff(
    targetCommitId: string,
    baseCommitId?: string,
    ignoreRebase = true,
  ): Promise<JjDiffResult> {
    const isInterDiff = Boolean(
      baseCommitId &&
      baseCommitId !== 'parent' &&
      baseCommitId !== `${targetCommitId}-` &&
      baseCommitId !== `${targetCommitId}^`
    );

    if (!isInterDiff || !baseCommitId) {
      const rawDiff = await this.runJj(['diff', '-r', targetCommitId, '--git']);
      return {
        rawDiff,
        isRebaseDiff: false,
        rebaseIgnored: false,
        rebaseConflict: false,
      };
    }

    // Check if baseCommitId and targetCommitId belong to the same change
    try {
      const template = 'commit_id ++ "\t" ++ change_id ++ "\t" ++ parents.map(|p| p.commit_id()).join(",") ++ "\n"';
      const logOutput = await this.runJj([
        'log',
        '-r',
        `${baseCommitId} | ${targetCommitId}`,
        '--no-graph',
        '-T',
        template,
      ]);

      const lines = logOutput.trim().split('\n').filter((l) => l.trim().length > 0);
      const commitInfoMap = new Map<string, { changeId: string; parents: string[] }>();
      for (const line of lines) {
        const [cid, chid, parentsRaw] = line.split('\t');
        if (cid && chid) {
          commitInfoMap.set(cid, {
            changeId: chid,
            parents: parentsRaw ? parentsRaw.split(',').filter(Boolean) : [],
          });
        }
      }

      // Find matching base and target by prefix or exact match
      const baseEntry = Array.from(commitInfoMap.entries()).find(
        ([cid]) => cid === baseCommitId || cid.startsWith(baseCommitId) || baseCommitId.startsWith(cid),
      );
      const targetEntry = Array.from(commitInfoMap.entries()).find(
        ([cid]) => cid === targetCommitId || cid.startsWith(targetCommitId) || targetCommitId.startsWith(cid),
      );

      if (baseEntry && targetEntry && baseEntry[1].changeId === targetEntry[1].changeId) {
        const baseParents = baseEntry[1].parents;
        const targetParents = targetEntry[1].parents;

        // If both have single parents and those parents are different -> rebase occurred!
        if (
          baseParents.length === 1 &&
          targetParents.length === 1 &&
          baseParents[0] !== targetParents[0]
        ) {
          if (ignoreRebase) {
            const p1 = baseParents[0];
            const p2 = targetParents[0];
            const c1 = baseEntry[0];
            const c2 = targetEntry[0];

            // In-memory 3-way merge: rebase c1 onto p2 using merge-base p1
            const mergeResult = await this.runGit([
              'merge-tree',
              '--write-tree',
              '--merge-base',
              p1,
              p2,
              c1,
            ]);

            if (mergeResult.exitCode === 0) {
              const treeOid = mergeResult.stdout.trim().split('\n')[0].trim();
              if (treeOid.length === 40 || treeOid.length === 64) {
                const diffResult = await this.runGit([
                  'diff',
                  '--no-color',
                  '--no-ext-diff',
                  treeOid,
                  c2,
                ]);
                if (diffResult.exitCode === 0) {
                  return {
                    rawDiff: diffResult.stdout,
                    isRebaseDiff: true,
                    rebaseIgnored: true,
                    rebaseConflict: false,
                  };
                }
              }
            }

            // Conflict occurred during merge-tree! Fall back to direct diff
            const rawDiff = await this.runJj(['diff', '--from', baseCommitId, '--to', targetCommitId, '--git']);
            return {
              rawDiff,
              isRebaseDiff: true,
              rebaseIgnored: false,
              rebaseConflict: true,
            };
          } else {
            // User toggled off ignoreRebase
            const rawDiff = await this.runJj(['diff', '--from', baseCommitId, '--to', targetCommitId, '--git']);
            return {
              rawDiff,
              isRebaseDiff: true,
              rebaseIgnored: false,
              rebaseConflict: false,
            };
          }
        }
      }
    } catch (err) {
      console.warn('Failed to detect rebase in inter-diff, falling back to direct diff:', err);
    }

    const rawDiff = await this.runJj(['diff', '--from', baseCommitId, '--to', targetCommitId, '--git']);
    return {
      rawDiff,
      isRebaseDiff: false,
      rebaseIgnored: false,
      rebaseConflict: false,
    };
  }

  async getDiffForCommit(commitId: string): Promise<string> {
    const res = await this.getDiff(commitId);
    return res.rawDiff;
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

    // 1. If diffing against base commit (Requirement 3):
    // All comments on baseCommit (resolved or unresolved) get side: 'old'
    if (baseCommitId) {
      const resolvedBaseId = baseCommitId;
      const baseThreads = allChangeThreads.length > 0
        ? allChangeThreads.filter((t) => commitMatches(t.commit_id, resolvedBaseId))
        : await this.getCommitThreads(resolvedBaseId);

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

    // 3. Any other comments in the change:
    // Include all comments for the change (both unresolved and resolved)
    // so they are accessible in the UI comments sidebar and diff view.
    for (const t of allChangeThreads) {
      if (!resultMap.has(t.thread_id)) {
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

  async approve(target: string, disapprove = false): Promise<boolean> {
    const args = ['approve', target];
    if (disapprove) {
      args.push('--disapprove');
    }
    await this.runTellit(args);
    return !disapprove;
  }
}
