import {
  GitCommit,
  GitBranch,
  ChevronDown,
  ArrowRight,
  Search,
  X,
  CheckCircle2,
  Check,
  GitMerge,
  AlertTriangle,
} from 'lucide-react';
import React, { useEffect, useState, useRef, useMemo, useCallback } from 'react';

import { type JjChange, type JjCommit, type JjGraphRow } from '../../types/diff';
import { useClickOutside } from '../hooks/useClickOutside';

import { CommentDot, JjRevisionTree } from './JjRevisionTree';

export { CommentDot };

export function commitIdsMatch(a?: string, b?: string): boolean {
  if (!a || !b) return false;
  if (a === b) return true;
  if (Math.min(a.length, b.length) >= 6) {
    return a.startsWith(b) || b.startsWith(a);
  }
  return false;
}

export function isEmptyUndescribedChange(c: JjChange): boolean {
  return Boolean(c.isEmpty && c.hasDescription === false && (c.commentCount ?? 0) === 0);
}

export function pickInitialChange(
  changes: JjChange[],
  currentCommitId?: string,
): JjChange | undefined {
  if (changes.length === 0) {
    return undefined;
  }

  const walkToNonEmptyAncestor = (start: JjChange): JjChange | undefined => {
    const visited = new Set<string>();
    const queue: JjChange[] = [start];
    while (queue.length > 0) {
      const current = queue.shift();
      if (!current || visited.has(current.commitId)) continue;
      visited.add(current.commitId);
      if (!isEmptyUndescribedChange(current)) {
        return current;
      }
      for (const parentId of current.parentCommitIds ?? []) {
        const parent = changes.find((c) => commitIdsMatch(c.commitId, parentId));
        if (parent && !visited.has(parent.commitId)) {
          queue.push(parent);
        }
      }
    }
    return undefined;
  };

  if (currentCommitId) {
    const commitMatch = changes.find((c) => commitIdsMatch(c.commitId, currentCommitId));
    if (commitMatch) {
      if (!isEmptyUndescribedChange(commitMatch)) {
        return commitMatch;
      }
      return (
        walkToNonEmptyAncestor(commitMatch) ??
        changes.find((c) => !isEmptyUndescribedChange(c)) ??
        commitMatch
      );
    }
  }

  const workingCopy = changes.find((c) => c.isWorkingCopy);
  if (workingCopy) {
    return (
      walkToNonEmptyAncestor(workingCopy) ??
      changes.find((c) => !isEmptyUndescribedChange(c)) ??
      workingCopy
    );
  }

  return changes.find((c) => !isEmptyUndescribedChange(c)) ?? changes[0];
}

function makeStandaloneCommitRow(change: JjChange): JjGraphRow {
  return {
    type: 'commit',
    key: change.commitId,
    graphCols: [change.isWorkingCopy ? '@' : change.isImmutable ? '◆' : '○'],
    downCols: [],
    change,
  };
}

function synthesizeGraphRows(changes: JjChange[]): JjGraphRow[] {
  return changes.map(makeStandaloneCommitRow);
}

function resolveGraphRows(changes: JjChange[], graphRows?: JjGraphRow[]): JjGraphRow[] {
  if (Array.isArray(graphRows) && (graphRows.length > 0 || changes.length === 0)) {
    return graphRows;
  }
  return synthesizeGraphRows(changes);
}

export interface UseJjRevisionsProps {
  currentCommitId?: string;
  onSelectDiff: (targetCommitId: string, baseCommitId?: string, ignoreRebase?: boolean) => void;
}

export function useJjRevisions({
  currentCommitId,
  onSelectDiff,
}: UseJjRevisionsProps) {
  const [changes, setChanges] = useState<JjChange[]>([]);
  const [graphRows, setGraphRows] = useState<JjGraphRow[]>([]);
  const [selectedChangeId, setSelectedChangeId] = useState<string>('');
  const [commits, setCommits] = useState<JjCommit[]>([]);
  const [targetCommitId, setTargetCommitId] = useState<string>('');
  const [baseCommitId, setBaseCommitId] = useState<string>('parent');
  const [ignoreRebase, setIgnoreRebase] = useState<boolean>(true);
  const [loadingChanges, setLoadingChanges] = useState(true);
  const [loadingCommits, setLoadingCommits] = useState(false);
  const [isCustomInput, setIsCustomInput] = useState(false);
  const [customInput, setCustomInput] = useState('');
  const [customError, setCustomError] = useState<string | null>(null);

  const currentCommitIdRef = useRef(currentCommitId);
  const onSelectDiffRef = useRef(onSelectDiff);
  useEffect(() => {
    currentCommitIdRef.current = currentCommitId;
    onSelectDiffRef.current = onSelectDiff;
  }, [currentCommitId, onSelectDiff]);

  const loadChanges = useCallback(async () => {
    try {
      const res = await fetch('/api/jj/changes');
      if (res.ok) {
        const data = (await res.json()) as { changes?: JjChange[]; graphRows?: JjGraphRow[] };
        const fetchedChanges = data.changes || [];
        setChanges(fetchedChanges);
        setGraphRows(resolveGraphRows(fetchedChanges, data.graphRows));
      }
    } catch (err) {
      console.error('Failed to reload jj changes:', err);
    }
  }, []);

  const loadCommits = useCallback(async (changeId: string) => {
    if (!changeId) return;
    try {
      const res = await fetch(`/api/jj/changes/${encodeURIComponent(changeId)}/commits`);
      if (res.ok) {
        const data = (await res.json()) as { commits?: JjCommit[] };
        setCommits(data.commits || []);
      }
    } catch (err) {
      console.error('Failed to reload jj commits:', err);
    }
  }, []);

  const reloadRevisions = useCallback(async () => {
    await Promise.all([
      loadChanges(),
      selectedChangeId ? loadCommits(selectedChangeId) : Promise.resolve(),
    ]);
  }, [loadChanges, loadCommits, selectedChangeId]);

  // Initial load of changes
  useEffect(() => {
    let mounted = true;
    fetch('/api/jj/changes')
      .then((res) => res.json())
      .then((data: { changes?: JjChange[]; graphRows?: JjGraphRow[] }) => {
        if (!mounted) return;
        const fetchedChanges = data.changes || [];
        setChanges(fetchedChanges);
        setGraphRows(resolveGraphRows(fetchedChanges, data.graphRows));
        setLoadingChanges(false);

        const initialChange = pickInitialChange(fetchedChanges, currentCommitIdRef.current);
        if (initialChange) {
          setSelectedChangeId(initialChange.changeId);
        }
      })
      .catch((err) => {
        console.error('Failed to load jj changes:', err);
        setLoadingChanges(false);
      });
    return () => {
      mounted = false;
    };
  }, []);

  const handleCustomSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const query = customInput.trim();
    if (!query) return;
    setCustomError(null);
    try {
      const res = await fetch(`/api/jj/changes?query=${encodeURIComponent(query)}`);
      if (res.ok) {
        const data = (await res.json()) as { changes?: JjChange[] };
        const found = data.changes?.[0];
        if (found) {
          setChanges((prev) => {
            const exists = prev.some((c) => c.changeId === found.changeId);
            return exists ? prev : [found, ...prev];
          });
          setGraphRows((prev) => {
            const exists = prev.some(
              (row) => row.type === 'commit' && row.change.changeId === found.changeId,
            );
            return exists ? prev : [makeStandaloneCommitRow(found), ...prev];
          });
          setSelectedChangeId(found.changeId);
          setIsCustomInput(false);
          setCustomInput('');
          return;
        }
      }
      setCustomError('Not found');
    } catch {
      setCustomError('Query failed');
    }
  };

  // When selected change changes, load its commits
  useEffect(() => {
    if (!selectedChangeId) return;
    let mounted = true;
    setLoadingCommits(true);
    fetch(`/api/jj/changes/${encodeURIComponent(selectedChangeId)}/commits`)
      .then((res) => res.json())
      .then((data: { commits?: JjCommit[] }) => {
        if (!mounted) return;
        const fetchedCommits = data.commits || [];
        setCommits(fetchedCommits);
        setLoadingCommits(false);

        const match = fetchedCommits.find((c) =>
          commitIdsMatch(c.commitId, currentCommitIdRef.current),
        );
        const latestCommit = fetchedCommits[fetchedCommits.length - 1];
        const selectedTarget = match?.commitId ?? latestCommit?.commitId;
        if (selectedTarget) {
          setTargetCommitId(selectedTarget);
          setBaseCommitId('parent');
          onSelectDiffRef.current(selectedTarget, undefined);
        }
      })
      .catch((err) => {
        console.error('Failed to load jj commits:', err);
        setLoadingCommits(false);
      });
    return () => {
      mounted = false;
    };
  }, [selectedChangeId]);

  const handleTargetChange = (newTarget: string) => {
    setTargetCommitId(newTarget);
    onSelectDiff(newTarget, baseCommitId === 'parent' ? undefined : baseCommitId, ignoreRebase);
  };

  const handleBaseChange = (newBase: string) => {
    setBaseCommitId(newBase);
    onSelectDiff(targetCommitId, newBase === 'parent' ? undefined : newBase, ignoreRebase);
  };

  const handleToggleIgnoreRebase = () => {
    const nextVal = !ignoreRebase;
    setIgnoreRebase(nextVal);
    onSelectDiff(targetCommitId, baseCommitId === 'parent' ? undefined : baseCommitId, nextVal);
  };

  const handleToggleApprove = async (commitId: string, currentApproved: boolean) => {
    try {
      const res = await fetch('/api/jj/approve', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ commitId, disapprove: currentApproved }),
      });
      if (res.ok) {
        await loadCommits(selectedChangeId);
        await loadChanges();
      }
    } catch (err) {
      console.error('Failed to toggle approval:', err);
    }
  };

  return {
    changes,
    graphRows,
    selectedChangeId,
    setSelectedChangeId,
    commits,
    targetCommitId,
    baseCommitId,
    ignoreRebase,
    loadingChanges,
    loadingCommits,
    isCustomInput,
    setIsCustomInput,
    customInput,
    setCustomInput,
    customError,
    setCustomError,
    handleCustomSubmit,
    handleTargetChange,
    handleBaseChange,
    handleToggleIgnoreRebase,
    handleToggleApprove,
    reloadRevisions,
  };
}

export interface JjChangeSelectorProps {
  changes: JjChange[];
  graphRows?: JjGraphRow[];
  selectedChangeId: string;
  onSelectChange: (changeId: string) => void;
  isCustomInput: boolean;
  setIsCustomInput: (val: boolean) => void;
  customInput: string;
  setCustomInput: (val: string) => void;
  customError: string | null;
  handleCustomSubmit: (e: React.FormEvent) => void;
  loading?: boolean;
}

export function JjChangeSelector({
  changes,
  graphRows,
  selectedChangeId,
  onSelectChange,
  isCustomInput,
  setIsCustomInput,
  customInput,
  setCustomInput,
  customError,
  handleCustomSubmit,
  loading = false,
}: JjChangeSelectorProps) {
  const [isOpen, setIsOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);
  useClickOutside(dropdownRef, () => setIsOpen(false));

  const effectiveGraphRows = useMemo(
    () => resolveGraphRows(changes, graphRows),
    [changes, graphRows],
  );
  const selectedChange = changes.find((c) => c.changeId === selectedChangeId);

  if (loading && changes.length === 0) {
    return <div className="text-xs text-github-text-secondary">Loading...</div>;
  }

  if (changes.length === 0) return null;

  return (
    <div className="relative" ref={dropdownRef}>
      <div className="flex items-center gap-1.5 bg-github-bg-tertiary border border-github-border rounded px-2 py-1 text-xs">
        <GitBranch size={13} className="text-github-accent shrink-0" />
        <span className="font-semibold text-github-text-secondary">Change:</span>
        {isCustomInput ? (
          <form onSubmit={handleCustomSubmit} className="flex items-center gap-1">
            <input
              type="text"
              value={customInput}
              onChange={(e) => setCustomInput(e.target.value)}
              placeholder="e.g. kkmpptxz or @-"
              className="bg-github-bg-secondary text-github-text-primary text-xs font-mono px-1.5 py-0.5 rounded border border-github-border outline-none w-[150px]"
              autoFocus
            />
            <button
              type="submit"
              className="bg-github-accent text-white px-1.5 py-0.5 rounded text-[11px] font-semibold hover:opacity-90 cursor-pointer"
            >
              Go
            </button>
            <button
              type="button"
              onClick={() => setIsCustomInput(false)}
              className="text-github-text-muted hover:text-github-text-primary px-0.5 cursor-pointer"
              title="Cancel"
            >
              <X size={12} />
            </button>
            {customError && (
              <span className="text-red-400 text-[10px] whitespace-nowrap">{customError}</span>
            )}
          </form>
        ) : (
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={() => setIsOpen((prev) => !prev)}
              className="flex items-center gap-1.5 text-github-text-primary text-xs font-mono outline-none cursor-pointer max-w-[210px] truncate hover:text-github-accent"
              title="Select Jujutsu Change"
            >
              <span className="truncate">
                {selectedChange ? (
                  <>
                    {selectedChange.isWorkingCopy ? '@ ' : ''}
                    {selectedChange.shortChangeId} {selectedChange.description.slice(0, 20)}
                  </>
                ) : (
                  'Select change...'
                )}
              </span>
              {selectedChange && (
                <CommentDot
                  commentCount={selectedChange.commentCount}
                  unresolvedCommentCount={selectedChange.unresolvedCommentCount}
                  approved={selectedChange.approved}
                  cleanRebaseApproved={selectedChange.cleanRebaseApproved}
                />
              )}
              <ChevronDown size={12} className="text-github-text-muted shrink-0" />
            </button>
            <button
              type="button"
              onClick={() => setIsCustomInput(true)}
              className="text-github-text-muted hover:text-github-text-primary p-0.5 cursor-pointer"
              title="Enter specific change ID"
            >
              <Search size={12} />
            </button>
          </div>
        )}
      </div>

      {isOpen && (
        <div className="absolute left-0 top-full mt-1 z-50 bg-github-bg-secondary border border-github-border rounded-md shadow-xl py-1 w-80">
          <div className="px-2.5 py-1 text-[11px] font-semibold text-github-text-muted uppercase tracking-wider border-b border-github-border/50">
            Changes
          </div>
          <JjRevisionTree
            graphRows={effectiveGraphRows}
            selectedChangeId={selectedChangeId}
            onSelectChange={(changeId) => {
              onSelectChange(changeId);
              setIsOpen(false);
            }}
            className="max-h-60 overflow-y-auto overflow-x-auto"
          />
          <div className="border-t border-github-border/50 mt-1 pt-1">
            <button
              type="button"
              onClick={() => {
                setIsCustomInput(true);
                setIsOpen(false);
              }}
              className="w-full px-2.5 py-1.5 text-left text-xs text-github-accent hover:bg-github-bg-tertiary flex items-center gap-1.5 cursor-pointer font-sans"
            >
              <Search size={12} />
              <span>Enter Change ID / Rev...</span>
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

export interface IndexedJjCommit {
  commit: JjCommit;
  commitNumber: number;
}

export interface IsInterestingCommitOptions {
  isLatest: boolean;
  targetCommitId?: string;
  baseCommitId?: string;
}

export function isInterestingCommit(
  commit: JjCommit,
  { isLatest, targetCommitId, baseCommitId }: IsInterestingCommitOptions,
): boolean {
  if (isLatest) return true;
  if (Boolean(commit.approved) || commit.commentCount > 0) return true;
  if (commitIdsMatch(commit.commitId, targetCommitId)) return true;
  return (
    Boolean(baseCommitId) &&
    baseCommitId !== 'parent' &&
    commitIdsMatch(commit.commitId, baseCommitId)
  );
}

export function filterInterestingCommits(
  commits: JjCommit[],
  options: { targetCommitId?: string; baseCommitId?: string } = {},
): IndexedJjCommit[] {
  const { targetCommitId, baseCommitId } = options;
  return commits
    .map((commit, originalIndex) => ({
      commit,
      commitNumber: originalIndex + 1,
      isLatest: originalIndex === commits.length - 1,
    }))
    .filter(({ commit, isLatest }) =>
      isInterestingCommit(commit, { isLatest, targetCommitId, baseCommitId }),
    )
    .map(({ commit, commitNumber }) => ({ commit, commitNumber }))
    .reverse();
}

function CommitOptionRow({
  commit,
  commitNumber,
  isSelected,
  isTarget = false,
  onSelect,
}: {
  commit: JjCommit;
  commitNumber: number;
  isSelected: boolean;
  isTarget?: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      className={`w-full px-2.5 py-1.5 text-left text-xs font-mono flex items-center justify-between gap-2 transition-colors cursor-pointer ${
        isSelected
          ? 'bg-github-bg-tertiary text-github-accent font-medium'
          : 'text-github-text-primary hover:bg-github-bg-tertiary'
      }`}
    >
      <div className="flex items-center gap-1.5 min-w-0 truncate">
        <span className="text-github-text-muted shrink-0 text-[10px]">#{commitNumber}</span>
        <span className="font-semibold text-github-text-primary shrink-0">
          {commit.shortCommitId}
        </span>
        {commit.timestamp && (
          <span className="text-github-text-muted shrink-0 text-[10px]">
            [{commit.timestamp.slice(5, 16)}]
          </span>
        )}
        <span className="text-github-text-secondary truncate text-[11px]">{commit.description}</span>
        {isTarget && (
          <span className="text-[10px] text-github-text-muted italic shrink-0">(head)</span>
        )}
      </div>
      <CommentDot
        commentCount={commit.commentCount}
        unresolvedCommentCount={commit.unresolvedCommentCount}
        approved={commit.approved}
        cleanRebaseApproved={commit.cleanRebaseApproved}
        cleanRebaseSourceNumber={commit.cleanRebaseSourceCommitNumber}
      />
    </button>
  );
}

function RevisionVisibilityFooter({
  totalCount,
  hiddenCount,
  showAll,
  onToggle,
}: {
  totalCount: number;
  hiddenCount: number;
  showAll: boolean;
  onToggle: () => void;
}) {
  if (hiddenCount <= 0) return null;
  return (
    <div className="border-t border-github-border/50 mt-1 pt-1">
      <button
        type="button"
        onClick={onToggle}
        className="w-full px-2.5 py-1.5 text-left text-xs text-github-accent hover:bg-github-bg-tertiary flex items-center justify-between gap-2 cursor-pointer font-sans"
      >
        {showAll ? (
          <span>
            Hide {hiddenCount} unreviewed {hiddenCount === 1 ? 'revision' : 'revisions'}
          </span>
        ) : (
          <>
            <span>Show all {totalCount} revisions</span>
            <span className="text-[10px] text-github-text-muted">{hiddenCount} hidden</span>
          </>
        )}
      </button>
    </div>
  );
}

export interface JjCommitComparisonSelectorProps {
  commits: JjCommit[];
  targetCommitId: string;
  baseCommitId: string;
  onSelectTarget: (targetId: string) => void;
  onSelectBase: (baseId: string) => void;
  onToggleApprove?: (commitId: string, currentApproved: boolean) => void;
  isRebaseDiff?: boolean;
  rebaseIgnored?: boolean;
  rebaseConflict?: boolean;
  ignoreRebase?: boolean;
  onToggleIgnoreRebase?: () => void;
  loading?: boolean;
}

export function JjCommitComparisonSelector({
  commits,
  targetCommitId,
  baseCommitId,
  onSelectTarget,
  onSelectBase,
  onToggleApprove,
  isRebaseDiff,
  rebaseIgnored,
  rebaseConflict,
  ignoreRebase: _ignoreRebase = true,
  onToggleIgnoreRebase,
  loading = false,
}: JjCommitComparisonSelectorProps) {
  const [isFromOpen, setIsFromOpen] = useState(false);
  const [isToOpen, setIsToOpen] = useState(false);
  const [showAllFrom, setShowAllFrom] = useState(false);
  const [showAllTo, setShowAllTo] = useState(false);
  const fromDropdownRef = useRef<HTMLDivElement>(null);
  const toDropdownRef = useRef<HTMLDivElement>(null);

  const closeFromDropdown = useCallback(() => {
    setIsFromOpen(false);
    setShowAllFrom(false);
  }, []);

  const closeToDropdown = useCallback(() => {
    setIsToOpen(false);
    setShowAllTo(false);
  }, []);

  useClickOutside(fromDropdownRef, closeFromDropdown);
  useClickOutside(toDropdownRef, closeToDropdown);

  // Reverse chronological order for dropdown menus (top is latest commit)
  const reversedCommits = useMemo<IndexedJjCommit[]>(() => {
    return commits
      .map((commit, originalIndex) => ({
        commit,
        commitNumber: originalIndex + 1,
      }))
      .reverse();
  }, [commits]);

  const interestingReversedCommits = useMemo<IndexedJjCommit[]>(
    () => filterInterestingCommits(commits, { targetCommitId, baseCommitId }),
    [commits, targetCommitId, baseCommitId],
  );

  const hiddenCount = reversedCommits.length - interestingReversedCommits.length;
  const visibleFromCommits =
    showAllFrom || hiddenCount === 0 ? reversedCommits : interestingReversedCommits;
  const visibleToCommits =
    showAllTo || hiddenCount === 0 ? reversedCommits : interestingReversedCommits;

  const activeTargetIndex = commits.findIndex((c) => commitIdsMatch(c.commitId, targetCommitId));
  const activeTargetCommit = activeTargetIndex >= 0 ? commits[activeTargetIndex] : null;

  const activeBaseIndex = commits.findIndex((c) => commitIdsMatch(c.commitId, baseCommitId));
  const activeBaseCommit = activeBaseIndex >= 0 ? commits[activeBaseIndex] : null;
  const isBaseParent = baseCommitId === 'parent';

  if (loading && commits.length === 0) {
    return <div className="text-xs text-github-text-secondary">Loading commits...</div>;
  }

  if (commits.length === 0) return null;

  return (
    <div className="flex items-center gap-1.5 text-xs font-mono">
      {/* From (Diff Base) Selector */}
      <div className="relative" ref={fromDropdownRef}>
        <button
          type="button"
          onClick={() => {
            if (isFromOpen) {
              closeFromDropdown();
            } else {
              setIsFromOpen(true);
            }
          }}
          className="flex items-center gap-1.5 bg-github-bg-tertiary border border-github-border rounded px-2 py-1 text-github-text-primary hover:text-github-accent cursor-pointer max-w-[190px] truncate"
          title="Select diff base revision (From)"
        >
          <span className="truncate">
            {isBaseParent || !activeBaseCommit ? (
              'Parent'
            ) : (
              <>#{activeBaseIndex + 1}: {activeBaseCommit.shortCommitId}</>
            )}
          </span>
          {activeBaseCommit && !isBaseParent && (
            <CommentDot
              commentCount={activeBaseCommit.commentCount}
              unresolvedCommentCount={activeBaseCommit.unresolvedCommentCount}
              approved={activeBaseCommit.approved}
              cleanRebaseApproved={activeBaseCommit.cleanRebaseApproved}
              cleanRebaseSourceNumber={activeBaseCommit.cleanRebaseSourceCommitNumber}
            />
          )}
          <ChevronDown size={12} className="text-github-text-muted shrink-0" />
        </button>

        {isFromOpen && (
          <div className="absolute left-0 top-full mt-1 z-50 bg-github-bg-secondary border border-github-border rounded-md shadow-xl py-1 w-84">
            <div className="px-2.5 py-1 text-[11px] font-semibold text-github-text-muted uppercase tracking-wider border-b border-github-border/50">
              Diff Base (From)
            </div>
            <div className="max-h-60 overflow-y-auto">
              {/* Parent option */}
              <button
                type="button"
                onClick={() => {
                  onSelectBase('parent');
                  closeFromDropdown();
                }}
                className={`w-full px-2.5 py-1.5 text-left text-xs font-mono flex items-center justify-between gap-2 transition-colors cursor-pointer ${
                  isBaseParent
                    ? 'bg-github-bg-tertiary text-github-accent font-medium'
                    : 'text-github-text-primary hover:bg-github-bg-tertiary'
                }`}
              >
                <div className="flex items-center gap-1.5 min-w-0">
                  <span className="font-semibold text-github-text-primary">Parent</span>
                  <span className="text-github-text-secondary text-[11px]">(Entire Change)</span>
                </div>
              </button>

              {/* Commits in reverse chronological order */}
              {visibleFromCommits.map(({ commit, commitNumber }) => (
                <CommitOptionRow
                  key={commit.commitId}
                  commit={commit}
                  commitNumber={commitNumber}
                  isSelected={!isBaseParent && commitIdsMatch(commit.commitId, baseCommitId)}
                  isTarget={commitIdsMatch(commit.commitId, targetCommitId)}
                  onSelect={() => {
                    onSelectBase(commit.commitId);
                    closeFromDropdown();
                  }}
                />
              ))}
            </div>
            <RevisionVisibilityFooter
              totalCount={commits.length}
              hiddenCount={hiddenCount}
              showAll={showAllFrom}
              onToggle={() => setShowAllFrom((prev) => !prev)}
            />
          </div>
        )}
      </div>

      {/* Right Arrow */}
      <ArrowRight size={13} className="text-github-text-muted shrink-0" />

      {/* To (Diff Head / Target) Selector */}
      <div className="relative" ref={toDropdownRef}>
        <button
          type="button"
          onClick={() => {
            if (isToOpen) {
              closeToDropdown();
            } else {
              setIsToOpen(true);
            }
          }}
          className="flex items-center gap-1.5 bg-github-bg-tertiary border border-github-border rounded px-2 py-1 text-github-text-primary hover:text-github-accent cursor-pointer max-w-[240px] truncate"
          title="Select diff head revision (To)"
        >
          <GitCommit size={13} className="text-github-text-secondary shrink-0" />
          <span className="truncate">
            {activeTargetCommit ? (
              <>
                #{activeTargetIndex + 1}: {activeTargetCommit.shortCommitId}
                {activeTargetCommit.description ? ` - ${activeTargetCommit.description.slice(0, 16)}` : ''}
              </>
            ) : (
              'Select commit...'
            )}
          </span>
          {activeTargetCommit && (
            <CommentDot
              commentCount={activeTargetCommit.commentCount}
              unresolvedCommentCount={activeTargetCommit.unresolvedCommentCount}
              approved={activeTargetCommit.approved}
              cleanRebaseApproved={activeTargetCommit.cleanRebaseApproved}
              cleanRebaseSourceNumber={activeTargetCommit.cleanRebaseSourceCommitNumber}
            />
          )}
          <ChevronDown size={12} className="text-github-text-muted shrink-0" />
        </button>

        {isToOpen && (
          <div className="absolute right-0 top-full mt-1 z-50 bg-github-bg-secondary border border-github-border rounded-md shadow-xl py-1 w-84">
            <div className="px-2.5 py-1 text-[11px] font-semibold text-github-text-muted uppercase tracking-wider border-b border-github-border/50">
              {`Diff Head / Commits (${
                showAllTo || hiddenCount === 0
                  ? commits.length
                  : `${interestingReversedCommits.length} of ${commits.length}`
              })`}
            </div>
            <div className="max-h-60 overflow-y-auto">
              {/* Commits in reverse chronological order */}
              {visibleToCommits.map(({ commit, commitNumber }) => (
                <CommitOptionRow
                  key={commit.commitId}
                  commit={commit}
                  commitNumber={commitNumber}
                  isSelected={commitIdsMatch(commit.commitId, targetCommitId)}
                  onSelect={() => {
                    onSelectTarget(commit.commitId);
                    closeToDropdown();
                  }}
                />
              ))}
            </div>
            <RevisionVisibilityFooter
              totalCount={commits.length}
              hiddenCount={hiddenCount}
              showAll={showAllTo}
              onToggle={() => setShowAllTo((prev) => !prev)}
            />
          </div>
        )}
      </div>

      {/* Rebase Diff Filter Toggle */}
      {isBaseParent ? (
        <span
          className="flex items-center gap-1 px-2 py-1 rounded text-xs font-sans text-github-text-muted/60 border border-github-border/40 bg-github-bg-secondary/40 cursor-not-allowed select-none"
          title="Upstream rebase filtering is available when comparing two patchsets (select an earlier commit in 'From')"
        >
          <GitMerge size={12} className="text-github-text-muted/50 shrink-0" />
          <span>Rebase filter (interdiff only)</span>
        </span>
      ) : isRebaseDiff && onToggleIgnoreRebase ? (
        <button
          type="button"
          onClick={onToggleIgnoreRebase}
          className={`flex items-center gap-1 px-2 py-1 rounded text-xs font-sans font-medium border transition-colors cursor-pointer ${
            rebaseConflict
              ? 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30 hover:bg-yellow-500/25'
              : rebaseIgnored
              ? 'bg-blue-500/15 text-blue-400 border-blue-500/30 hover:bg-blue-500/25'
              : 'bg-github-bg-tertiary text-github-text-secondary border-github-border hover:text-github-text-primary'
          }`}
          title={
            rebaseConflict
              ? 'Rebase conflict detected — showing full diff including upstream changes.'
              : rebaseIgnored
              ? 'Upstream rebase/squash changes are hidden. Click to show full diff.'
              : 'Showing full diff including upstream changes. Click to hide rebase changes.'
          }
        >
          {rebaseConflict ? (
            <>
              <AlertTriangle size={12} className="text-yellow-400 shrink-0" />
              <span>Rebase conflict</span>
            </>
          ) : rebaseIgnored ? (
            <>
              <Check size={12} className="text-blue-400 shrink-0" />
              <span>Rebase hidden</span>
            </>
          ) : (
            <>
              <GitMerge size={12} className="text-github-text-secondary shrink-0" />
              <span>Show rebase</span>
            </>
          )}
        </button>
      ) : (
        <span
          className="flex items-center gap-1 px-2 py-1 rounded text-xs font-sans font-medium text-github-text-muted border border-github-border/50 bg-github-bg-tertiary/50 cursor-not-allowed select-none"
          title={`Patchsets #${activeBaseIndex + 1} and #${activeTargetIndex + 1} share the same parent commit — no upstream rebase changes to hide.`}
        >
          <GitMerge size={12} className="text-github-text-muted shrink-0 opacity-70" />
          <span>No rebase (same base)</span>
        </span>
      )}

      {/* Approve Button */}
      {onToggleApprove && activeTargetCommit && (
        <button
          type="button"
          onClick={() => {
            if (activeTargetCommit.cleanRebaseApproved && !activeTargetCommit.approved) {
              // Direct reapproval upgrade
              onToggleApprove(targetCommitId, false);
            } else {
              onToggleApprove(targetCommitId, Boolean(activeTargetCommit.approved));
            }
          }}
          className={`flex items-center gap-1.5 px-2.5 py-1 rounded text-xs font-medium border transition-all cursor-pointer ${
            activeTargetCommit.approved
              ? 'bg-green-500/15 text-green-400 border-green-500/40 hover:bg-green-500/25'
              : activeTargetCommit.cleanRebaseApproved
              ? 'bg-emerald-500/15 text-emerald-400 border-emerald-500/40 hover:bg-emerald-500/25'
              : 'bg-github-bg-tertiary text-github-text-secondary border-github-border hover:text-github-text-primary hover:border-github-accent'
          }`}
          title={
            activeTargetCommit.approved
              ? 'Approved - click to revoke approval'
              : activeTargetCommit.cleanRebaseApproved
              ? `Approved (Clean Rebase from #${activeTargetCommit.cleanRebaseSourceCommitNumber || ''}) - click to upgrade to direct approval`
              : 'Approve this revision'
          }
        >
          <CheckCircle2
            size={13}
            className={
              activeTargetCommit.approved
                ? 'text-green-400'
                : activeTargetCommit.cleanRebaseApproved
                ? 'text-emerald-400'
                : 'text-github-text-muted'
            }
          />
          <span>
            {activeTargetCommit.approved
              ? 'Approved'
              : activeTargetCommit.cleanRebaseApproved
              ? 'Approved (Clean Rebase)'
              : 'Approve'}
          </span>
        </button>
      )}
    </div>
  );
}

export function JjRevisionSelector({
  currentCommitId,
  onSelectDiff,
}: {
  currentCommitId?: string;
  onSelectDiff: (targetCommitId: string, baseCommitId?: string, ignoreRebase?: boolean) => void;
}) {
  const revisions = useJjRevisions({ currentCommitId, onSelectDiff });

  return (
    <div className="flex flex-wrap items-center gap-2">
      <JjChangeSelector
        changes={revisions.changes}
        selectedChangeId={revisions.selectedChangeId}
        onSelectChange={revisions.setSelectedChangeId}
        isCustomInput={revisions.isCustomInput}
        setIsCustomInput={revisions.setIsCustomInput}
        customInput={revisions.customInput}
        setCustomInput={revisions.setCustomInput}
        customError={revisions.customError}
        handleCustomSubmit={revisions.handleCustomSubmit}
        loading={revisions.loadingChanges}
      />
      <JjCommitComparisonSelector
        commits={revisions.commits}
        targetCommitId={revisions.targetCommitId}
        baseCommitId={revisions.baseCommitId}
        onSelectTarget={revisions.handleTargetChange}
        onSelectBase={revisions.handleBaseChange}
        onToggleApprove={revisions.handleToggleApprove}
        ignoreRebase={revisions.ignoreRebase}
        onToggleIgnoreRebase={revisions.handleToggleIgnoreRebase}
        loading={revisions.loadingCommits}
      />
    </div>
  );
}
