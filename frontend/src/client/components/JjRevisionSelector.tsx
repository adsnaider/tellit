import {
  GitCommit,
  GitBranch,
  MessageSquare,
  CheckCircle2,
  ChevronDown,
  Layers,
  FileDiff,
  Search,
  X,
} from 'lucide-react';
import React, { useEffect, useState, useRef } from 'react';

import { type JjChange, type JjCommit } from '../../types/diff';
import { useClickOutside } from '../hooks/useClickOutside';

interface JjRevisionSelectorProps {
  currentCommitId?: string;
  onSelectDiff: (targetCommitId: string, baseCommitId?: string) => void;
}

function CommentDot({
  commentCount,
  unresolvedCommentCount,
}: {
  commentCount?: number;
  unresolvedCommentCount?: number;
}) {
  const count = commentCount ?? 0;
  const unresolved = unresolvedCommentCount ?? 0;
  if (count === 0) return null;
  const isUnresolved = unresolved > 0;
  return (
    <span
      className={`inline-block w-2 h-2 rounded-full shrink-0 ${
        isUnresolved ? 'bg-yellow-400 shadow-sm' : 'bg-gray-400'
      }`}
      title={
        isUnresolved
          ? `${unresolved} unresolved comment${unresolved > 1 ? 's' : ''}`
          : 'All comments resolved'
      }
    />
  );
}

export function JjRevisionSelector({
  currentCommitId,
  onSelectDiff,
}: JjRevisionSelectorProps) {
  const [changes, setChanges] = useState<JjChange[]>([]);
  const [selectedChangeId, setSelectedChangeId] = useState<string>('');
  const [commits, setCommits] = useState<JjCommit[]>([]);
  const [targetCommitId, setTargetCommitId] = useState<string>('');
  const [baseCommitId, setBaseCommitId] = useState<string>('parent');
  const [loadingChanges, setLoadingChanges] = useState(true);
  const [loadingCommits, setLoadingCommits] = useState(false);
  const [isCustomInput, setIsCustomInput] = useState(false);
  const [customInput, setCustomInput] = useState('');
  const [customError, setCustomError] = useState<string | null>(null);

  const [isChangeOpen, setIsChangeOpen] = useState(false);
  const [isCommitOpen, setIsCommitOpen] = useState(false);
  const changeDropdownRef = useRef<HTMLDivElement>(null);
  const commitDropdownRef = useRef<HTMLDivElement>(null);

  useClickOutside(changeDropdownRef, () => setIsChangeOpen(false));
  useClickOutside(commitDropdownRef, () => setIsCommitOpen(false));

  // Load changes (default jj log set)
  useEffect(() => {
    let mounted = true;
    fetch('/api/jj/changes')
      .then((res) => res.json())
      .then((data: { changes?: JjChange[] }) => {
        if (!mounted) return;
        const fetchedChanges = data.changes || [];
        setChanges(fetchedChanges);
        setLoadingChanges(false);

        // Find change matching currentCommitId or default to working copy
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
    fetch(`/api/jj/changes/${selectedChangeId}/commits`)
      .then((res) => res.json())
      .then((data: { commits?: JjCommit[] }) => {
        if (!mounted) return;
        const fetchedCommits = data.commits || [];
        setCommits(fetchedCommits);
        setLoadingCommits(false);

        if (fetchedCommits.length > 0) {
          // If currentCommitId is in this change's commits, select it, else newest
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
    onSelectDiff(newTarget, baseCommitId === 'parent' ? undefined : baseCommitId);
  };

  const handleBaseChange = (newBase: string) => {
    setBaseCommitId(newBase);
    onSelectDiff(targetCommitId, newBase === 'parent' ? undefined : newBase);
  };

  const setEntireChangeMode = () => {
    setBaseCommitId('parent');
    onSelectDiff(targetCommitId, undefined);
  };

  const setInterDiffMode = () => {
    const currentIndex = commits.findIndex((c) => c.commitId === targetCommitId);
    if (currentIndex > 0) {
      const prevCommit = commits[currentIndex - 1];
      setBaseCommitId(prevCommit.commitId);
      onSelectDiff(targetCommitId, prevCommit.commitId);
    }
  };

  const selectedChange = changes.find((c) => c.changeId === selectedChangeId);
  const currentTargetIndex = commits.findIndex((c) => c.commitId === targetCommitId);
  const activeCommit = currentTargetIndex >= 0 ? commits[currentTargetIndex] : null;
  const isInterDiffActive = baseCommitId !== 'parent';

  if (loadingChanges) {
    return <div className="text-xs text-github-text-secondary">Loading revisions...</div>;
  }

  if (changes.length === 0) {
    return null;
  }

  return (
    <div className="flex flex-wrap items-center gap-2 text-xs">
      {/* Change ID Selector */}
      <div className="relative" ref={changeDropdownRef}>
        <div className="flex items-center gap-1.5 bg-github-bg-tertiary border border-github-border rounded px-2 py-1">
          <GitBranch size={13} className="text-github-accent shrink-0" />
          <span className="font-semibold text-github-text-secondary">Change:</span>
          {isCustomInput ? (
            <form onSubmit={handleCustomSubmit} className="flex items-center gap-1">
              <input
                type="text"
                value={customInput}
                onChange={(e) => {
                  setCustomInput(e.target.value);
                  setCustomError(null);
                }}
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
                onClick={() => {
                  setIsCustomInput(false);
                  setCustomError(null);
                }}
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
                onClick={() => setIsChangeOpen((prev) => !prev)}
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

        {/* Changes Dropdown Menu */}
        {isChangeOpen && (
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
                    setSelectedChangeId(change.changeId);
                    setIsChangeOpen(false);
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
                  />
                </button>
              );
            })}
            <div className="border-t border-github-border/50 mt-1 pt-1">
              <button
                type="button"
                onClick={() => {
                  setIsCustomInput(true);
                  setIsChangeOpen(false);
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

      {/* Target Commit Selector (Chronological) */}
      <div className="relative" ref={commitDropdownRef}>
        <div className="flex items-center gap-1.5 bg-github-bg-tertiary border border-github-border rounded px-2 py-1">
          <GitCommit size={13} className="text-github-text-secondary shrink-0" />
          <span className="font-semibold text-github-text-secondary">Commit:</span>
          {loadingCommits ? (
            <span className="text-github-text-muted">Loading...</span>
          ) : (
            <button
              type="button"
              onClick={() => setIsCommitOpen((prev) => !prev)}
              className="flex items-center gap-1.5 text-github-text-primary text-xs font-mono outline-none cursor-pointer max-w-[230px] truncate hover:text-github-accent"
              title="Select Git Commit (sorted chronologically)"
            >
              <span className="truncate">
                {activeCommit ? (
                  <>
                    #{currentTargetIndex + 1}: {activeCommit.shortCommitId}
                    {activeCommit.description ? ` - ${activeCommit.description.slice(0, 16)}` : ''}
                  </>
                ) : (
                  'Select commit...'
                )}
              </span>
              {activeCommit && (
                <CommentDot
                  commentCount={activeCommit.commentCount}
                  unresolvedCommentCount={activeCommit.unresolvedCommentCount}
                />
              )}
              <ChevronDown size={12} className="text-github-text-muted shrink-0" />
            </button>
          )}
        </div>

        {/* Commits Dropdown Menu */}
        {isCommitOpen && !loadingCommits && (
          <div className="absolute left-0 top-full mt-1 z-50 bg-github-bg-secondary border border-github-border rounded-md shadow-xl py-1 w-84 max-h-72 overflow-y-auto">
            <div className="px-2.5 py-1 text-[11px] font-semibold text-github-text-muted uppercase tracking-wider border-b border-github-border/50">
              Commits in change ({commits.length})
            </div>
            {commits.map((commit, index) => {
              const isSelected = commit.commitId === targetCommitId;
              return (
                <button
                  key={commit.commitId}
                  type="button"
                  onClick={() => {
                    handleTargetChange(commit.commitId);
                    setIsCommitOpen(false);
                  }}
                  className={`w-full px-2.5 py-1.5 text-left text-xs font-mono flex items-center justify-between gap-2 transition-colors cursor-pointer ${
                    isSelected
                      ? 'bg-github-bg-tertiary text-github-accent font-medium'
                      : 'text-github-text-primary hover:bg-github-bg-tertiary'
                  }`}
                >
                  <div className="flex items-center gap-1.5 min-w-0 truncate">
                    <span className="text-github-text-muted shrink-0 text-[10px]">
                      #{index + 1}
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
                  />
                </button>
              );
            })}
          </div>
        )}
      </div>

      {/* Compare Mode Presets: Entire Change vs Inter-Diff */}
      {commits.length > 1 && (
        <div className="flex items-center bg-github-bg-tertiary border border-github-border rounded p-0.5">
          <button
            type="button"
            onClick={setEntireChangeMode}
            className={`px-2 py-0.5 rounded text-xs transition-colors flex items-center gap-1 cursor-pointer ${
              !isInterDiffActive
                ? 'bg-github-bg-primary text-github-text-primary font-medium shadow-sm'
                : 'text-github-text-secondary hover:text-github-text-primary'
            }`}
            title="View entire change against parent revision"
          >
            <Layers size={12} />
            <span>Entire Change</span>
          </button>
          <button
            type="button"
            onClick={setInterDiffMode}
            disabled={currentTargetIndex <= 0}
            className={`px-2 py-0.5 rounded text-xs transition-colors flex items-center gap-1 ${
              isInterDiffActive
                ? 'bg-github-bg-primary text-github-accent font-medium shadow-sm cursor-pointer'
                : currentTargetIndex > 0
                  ? 'text-github-text-secondary hover:text-github-text-primary cursor-pointer'
                  : 'text-github-text-muted opacity-50 cursor-not-allowed'
            }`}
            title={currentTargetIndex > 0 ? 'View diff between this commit and previous update' : 'First commit in change (no prior update)'}
          >
            <FileDiff size={12} />
            <span>Update Diff</span>
          </button>
        </div>
      )}

      {/* Base / Compare Against Selector (When multiple commits exist) */}
      {commits.length > 1 && (
        <div className="flex items-center gap-1 bg-github-bg-tertiary border border-github-border rounded px-2 py-1">
          <span className="font-semibold text-github-text-secondary">vs:</span>
          <select
            value={baseCommitId}
            onChange={(e) => handleBaseChange(e.target.value)}
            className="bg-transparent text-github-text-primary text-xs font-mono outline-none cursor-pointer max-w-[170px] truncate"
            title="Compare against base revision"
          >
            <option value="parent" className="bg-github-bg-secondary text-github-text-primary">
              Parent (Entire Change)
            </option>
            {commits
              .filter((c) => c.commitId !== targetCommitId)
              .map((commit, index) => (
                <option key={commit.commitId} value={commit.commitId} className="bg-github-bg-secondary text-github-text-primary font-mono">
                  #{index + 1}: {commit.shortCommitId} {commit.timestamp ? `[${commit.timestamp.slice(5, 16)}]` : ''}
                </option>
              ))}
          </select>
        </div>
      )}

      {/* Commit Comment Indicator Badge */}
      {(() => {
        if (!activeCommit || activeCommit.commentCount === 0) return null;
        return (
          <div
            className={`flex items-center gap-1 px-2 py-0.5 rounded text-[11px] font-medium border ${
              activeCommit.unresolvedCommentCount > 0
                ? 'bg-yellow-500/10 text-yellow-400 border-yellow-500/30'
                : 'bg-zinc-500/10 text-zinc-400 border-zinc-500/30'
            }`}
            title={`${activeCommit.commentCount} total comments, ${activeCommit.unresolvedCommentCount} unresolved`}
          >
            {activeCommit.unresolvedCommentCount > 0 ? (
              <>
                <MessageSquare size={11} />
                <span>{activeCommit.unresolvedCommentCount} open</span>
              </>
            ) : (
              <>
                <CheckCircle2 size={11} />
                <span>All resolved</span>
              </>
            )}
          </div>
        );
      })()}
    </div>
  );
}
