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

import { type JjChange, type JjCommit } from '../../types/diff';
import { useClickOutside } from '../hooks/useClickOutside';

export function CommentDot({
  commentCount,
  unresolvedCommentCount,
  approved,
}: {
  commentCount?: number;
  unresolvedCommentCount?: number;
  approved?: boolean;
}) {
  const count = commentCount ?? 0;
  const unresolved = unresolvedCommentCount ?? 0;

  // Comments take precedence. Only if all comments are resolved should it show green.
  if (unresolved > 0) {
    return (
      <span
        className="inline-block w-2 h-2 rounded-full shrink-0 bg-yellow-400 shadow-sm"
        title={`${unresolved} unresolved comment${unresolved > 1 ? 's' : ''}`}
      />
    );
  }

  if (approved) {
    return (
      <span
        className="inline-block w-2 h-2 rounded-full shrink-0 bg-green-500 shadow-sm"
        title={count > 0 ? 'Approved (all comments resolved)' : 'Approved'}
      />
    );
  }

  if (count > 0) {
    return (
      <span
        className="inline-block w-2 h-2 rounded-full shrink-0 bg-gray-400"
        title="All comments resolved"
      />
    );
  }

  return null;
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

  const loadChanges = useCallback(async () => {
    try {
      const res = await fetch('/api/jj/changes');
      if (res.ok) {
        const data = (await res.json()) as { changes?: JjChange[] };
        setChanges(data.changes || []);
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

  // Initial load of changes
  useEffect(() => {
    let mounted = true;
    fetch('/api/jj/changes')
      .then((res) => res.json())
      .then((data: { changes?: JjChange[] }) => {
        if (!mounted) return;
        const fetchedChanges = data.changes || [];
        setChanges(fetchedChanges);
        setLoadingChanges(false);

        if (fetchedChanges.length > 0) {
          const match = fetchedChanges.find((c) => c.commitId === currentCommitId || c.isWorkingCopy);
          const initialChange = match || fetchedChanges[0];
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
        if (data.changes && data.changes.length > 0) {
          const found = data.changes[0];
          setChanges((prev) => {
            const exists = prev.some((c) => c.changeId === found.changeId);
            return exists ? prev : [found, ...prev];
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

        if (fetchedCommits.length > 0) {
          const match = fetchedCommits.find((c) => c.commitId === currentCommitId);
          const selectedTarget = match ? match.commitId : fetchedCommits[fetchedCommits.length - 1].commitId;
          setTargetCommitId(selectedTarget);
          setBaseCommitId('parent');
          onSelectDiff(selectedTarget, undefined);
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
  };
}

export interface JjChangeSelectorProps {
  changes: JjChange[];
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
        <div className="absolute left-0 top-full mt-1 z-50 bg-github-bg-secondary border border-github-border rounded-md shadow-xl py-1 w-80 max-h-72 overflow-y-auto">
          <div className="px-2.5 py-1 text-[11px] font-semibold text-github-text-muted uppercase tracking-wider border-b border-github-border/50">
            Changes
          </div>
          {changes.map((change) => {
            const isSelected = change.changeId === selectedChangeId;
            return (
              <button
                key={change.changeId}
                type="button"
                onClick={() => {
                  onSelectChange(change.changeId);
                  setIsOpen(false);
                }}
                className={`w-full px-2.5 py-1.5 text-left text-xs font-mono flex items-center justify-between gap-2 transition-colors cursor-pointer ${
                  isSelected
                    ? 'bg-github-bg-tertiary text-github-accent font-medium'
                    : 'text-github-text-primary hover:bg-github-bg-tertiary'
                }`}
              >
                <div className="flex items-center gap-1.5 min-w-0 truncate">
                  <span className="text-github-accent font-bold">
                    {change.isWorkingCopy ? '@' : '○'}
                  </span>
                  <span className="font-semibold text-github-text-primary">
                    {change.shortChangeId}
                  </span>
                  <span className="text-github-text-secondary truncate text-[11px]">
                    {change.description}
                  </span>
                </div>
                <CommentDot
                  commentCount={change.commentCount}
                  unresolvedCommentCount={change.unresolvedCommentCount}
                  approved={change.approved}
                />
              </button>
            );
          })}
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
  ignoreRebase = true,
  onToggleIgnoreRebase,
  loading = false,
}: JjCommitComparisonSelectorProps) {
  const [isFromOpen, setIsFromOpen] = useState(false);
  const [isToOpen, setIsToOpen] = useState(false);
  const fromDropdownRef = useRef<HTMLDivElement>(null);
  const toDropdownRef = useRef<HTMLDivElement>(null);

  useClickOutside(fromDropdownRef, () => setIsFromOpen(false));
  useClickOutside(toDropdownRef, () => setIsToOpen(false));

  // Reverse chronological order for dropdown menus (top is latest commit)
  const reversedCommits = useMemo(() => {
    return commits
      .map((commit, originalIndex) => ({
        commit,
        commitNumber: originalIndex + 1,
      }))
      .reverse();
  }, [commits]);

  const activeTargetIndex = commits.findIndex((c) => c.commitId === targetCommitId);
  const activeTargetCommit = activeTargetIndex >= 0 ? commits[activeTargetIndex] : null;

  const activeBaseIndex = commits.findIndex((c) => c.commitId === baseCommitId);
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
          onClick={() => setIsFromOpen((prev) => !prev)}
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
            />
          )}
          <ChevronDown size={12} className="text-github-text-muted shrink-0" />
        </button>

        {isFromOpen && (
          <div className="absolute left-0 top-full mt-1 z-50 bg-github-bg-secondary border border-github-border rounded-md shadow-xl py-1 w-84 max-h-72 overflow-y-auto">
            <div className="px-2.5 py-1 text-[11px] font-semibold text-github-text-muted uppercase tracking-wider border-b border-github-border/50">
              Diff Base (From)
            </div>
            {/* Parent option */}
            <button
              type="button"
              onClick={() => {
                onSelectBase('parent');
                setIsFromOpen(false);
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
            {reversedCommits.map(({ commit, commitNumber }) => {
              const isSelected = !isBaseParent && commit.commitId === baseCommitId;
              const isTarget = commit.commitId === targetCommitId;
              return (
                <button
                  key={commit.commitId}
                  type="button"
                  onClick={() => {
                    onSelectBase(commit.commitId);
                    setIsFromOpen(false);
                  }}
                  className={`w-full px-2.5 py-1.5 text-left text-xs font-mono flex items-center justify-between gap-2 transition-colors cursor-pointer ${
                    isSelected
                      ? 'bg-github-bg-tertiary text-github-accent font-medium'
                      : 'text-github-text-primary hover:bg-github-bg-tertiary'
                  }`}
                >
                  <div className="flex items-center gap-1.5 min-w-0 truncate">
                    <span className="text-github-text-muted shrink-0 text-[10px]">
                      #{commitNumber}
                    </span>
                    <span className="font-semibold text-github-text-primary shrink-0">
                      {commit.shortCommitId}
                    </span>
                    {commit.timestamp && (
                      <span className="text-github-text-muted shrink-0 text-[10px]">
                        [{commit.timestamp.slice(5, 16)}]
                      </span>
                    )}
                    <span className="text-github-text-secondary truncate text-[11px]">
                      {commit.description}
                    </span>
                    {isTarget && (
                      <span className="text-[10px] text-github-text-muted italic shrink-0">(head)</span>
                    )}
                  </div>
                  <CommentDot
                    commentCount={commit.commentCount}
                    unresolvedCommentCount={commit.unresolvedCommentCount}
                    approved={commit.approved}
                  />
                </button>
              );
            })}
          </div>
        )}
      </div>

      {/* Right Arrow */}
      <ArrowRight size={13} className="text-github-text-muted shrink-0" />

      {/* To (Diff Head / Target) Selector */}
      <div className="relative" ref={toDropdownRef}>
        <button
          type="button"
          onClick={() => setIsToOpen((prev) => !prev)}
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
            />
          )}
          <ChevronDown size={12} className="text-github-text-muted shrink-0" />
        </button>

        {isToOpen && (
          <div className="absolute right-0 top-full mt-1 z-50 bg-github-bg-secondary border border-github-border rounded-md shadow-xl py-1 w-84 max-h-72 overflow-y-auto">
            <div className="px-2.5 py-1 text-[11px] font-semibold text-github-text-muted uppercase tracking-wider border-b border-github-border/50">
              Diff Head / Commits ({commits.length})
            </div>
            {/* Commits in reverse chronological order */}
            {reversedCommits.map(({ commit, commitNumber }) => {
              const isSelected = commit.commitId === targetCommitId;
              return (
                <button
                  key={commit.commitId}
                  type="button"
                  onClick={() => {
                    onSelectTarget(commit.commitId);
                    setIsToOpen(false);
                  }}
                  className={`w-full px-2.5 py-1.5 text-left text-xs font-mono flex items-center justify-between gap-2 transition-colors cursor-pointer ${
                    isSelected
                      ? 'bg-github-bg-tertiary text-github-accent font-medium'
                      : 'text-github-text-primary hover:bg-github-bg-tertiary'
                  }`}
                >
                  <div className="flex items-center gap-1.5 min-w-0 truncate">
                    <span className="text-github-text-muted shrink-0 text-[10px]">
                      #{commitNumber}
                    </span>
                    <span className="font-semibold text-github-text-primary shrink-0">
                      {commit.shortCommitId}
                    </span>
                    {commit.timestamp && (
                      <span className="text-github-text-muted shrink-0 text-[10px]">
                        [{commit.timestamp.slice(5, 16)}]
                      </span>
                    )}
                    <span className="text-github-text-secondary truncate text-[11px]">
                      {commit.description}
                    </span>
                  </div>
                  <CommentDot
                    commentCount={commit.commentCount}
                    unresolvedCommentCount={commit.unresolvedCommentCount}
                    approved={commit.approved}
                  />
                </button>
              );
            })}
          </div>
        )}
      </div>

      {/* Rebase Diff Filter Toggle */}
      {isRebaseDiff && onToggleIgnoreRebase && (
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
      )}

      {/* Approve Button */}
      {onToggleApprove && activeTargetCommit && (
        <button
          type="button"
          onClick={() => onToggleApprove(targetCommitId, Boolean(activeTargetCommit.approved))}
          className={`flex items-center gap-1.5 px-2.5 py-1 rounded text-xs font-medium border transition-all cursor-pointer ${
            activeTargetCommit.approved
              ? 'bg-green-500/15 text-green-400 border-green-500/40 hover:bg-green-500/25'
              : 'bg-github-bg-tertiary text-github-text-secondary border-github-border hover:text-github-text-primary hover:border-github-accent'
          }`}
          title={
            activeTargetCommit.approved
              ? 'Approved - click to revoke approval'
              : 'Approve this revision'
          }
        >
          <CheckCircle2
            size={13}
            className={activeTargetCommit.approved ? 'text-green-400' : 'text-github-text-muted'}
          />
          <span>{activeTargetCommit.approved ? 'Approved' : 'Approve'}</span>
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
  onSelectDiff: (targetCommitId: string, baseCommitId?: string) => void;
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
        loading={revisions.loadingCommits}
      />
    </div>
  );
}
