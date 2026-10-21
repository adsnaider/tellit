import React, { useEffect, useState, useCallback } from 'react';
import { GitCommit, GitBranch, MessageSquare, CheckCircle2, ArrowRight, Layers, FileDiff, Search, X } from 'lucide-react';
import { type JjChange, type JjCommit } from '../../types/diff';

interface JjRevisionSelectorProps {
  currentCommitId?: string;
  onSelectDiff: (targetCommitId: string, baseCommitId?: string) => void;
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

  const currentTargetIndex = commits.findIndex((c) => c.commitId === targetCommitId);
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
              className="bg-github-accent text-white px-1.5 py-0.5 rounded text-[11px] font-semibold hover:opacity-90"
            >
              Go
            </button>
            <button
              type="button"
              onClick={() => {
                setIsCustomInput(false);
                setCustomError(null);
              }}
              className="text-github-text-muted hover:text-github-text-primary px-0.5"
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
            <select
              value={selectedChangeId}
              onChange={(e) => {
                if (e.target.value === '__custom__') {
                  setIsCustomInput(true);
                } else {
                  setSelectedChangeId(e.target.value);
                }
              }}
              className="bg-transparent text-github-text-primary text-xs font-mono outline-none cursor-pointer max-w-[190px] truncate"
              title="Select Jujutsu Change"
            >
              {changes.map((change) => (
                <option key={change.changeId} value={change.changeId} className="bg-github-bg-secondary text-github-text-primary font-mono">
                  {change.isWorkingCopy ? '@ ' : ''}
                  {change.shortChangeId} {change.description.slice(0, 28)}
                </option>
              ))}
              <option value="__custom__" className="bg-github-bg-secondary text-github-accent font-sans">
                ✏️ Enter Change ID...
              </option>
            </select>
            <button
              type="button"
              onClick={() => setIsCustomInput(true)}
              className="text-github-text-muted hover:text-github-text-primary p-0.5"
              title="Enter specific change ID"
            >
              <Search size={12} />
            </button>
          </div>
        )}
      </div>

      {/* Target Commit Selector (Chronological) */}
      <div className="flex items-center gap-1.5 bg-github-bg-tertiary border border-github-border rounded px-2 py-1">
        <GitCommit size={13} className="text-github-text-secondary shrink-0" />
        <span className="font-semibold text-github-text-secondary">Commit:</span>
        {loadingCommits ? (
          <span className="text-github-text-muted">Loading...</span>
        ) : (
          <select
            value={targetCommitId}
            onChange={(e) => handleTargetChange(e.target.value)}
            className="bg-transparent text-github-text-primary text-xs font-mono outline-none cursor-pointer max-w-[210px] truncate"
            title="Select Git Commit (sorted chronologically)"
          >
            {commits.map((commit, index) => {
              const commentBadge =
                commit.commentCount > 0
                  ? commit.unresolvedCommentCount > 0
                    ? ` (${commit.unresolvedCommentCount} open)`
                    : ' (✓ resolved)'
                  : '';
              const orderLabel = `#${index + 1}: `;
              return (
                <option key={commit.commitId} value={commit.commitId} className="bg-github-bg-secondary text-github-text-primary font-mono">
                  {orderLabel}
                  {commit.shortCommitId}
                  {commit.timestamp ? ` [${commit.timestamp.slice(5, 16)}]` : ''}
                  {commit.description ? ` - ${commit.description.slice(0, 20)}` : ''}
                  {commentBadge}
                </option>
              );
            })}
          </select>
        )}
      </div>

      {/* Compare Mode Presets: Entire Change vs Inter-Diff */}
      {commits.length > 1 && (
        <div className="flex items-center bg-github-bg-tertiary border border-github-border rounded p-0.5">
          <button
            type="button"
            onClick={setEntireChangeMode}
            className={`px-2 py-0.5 rounded text-xs transition-colors flex items-center gap-1 ${
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
                ? 'bg-github-bg-primary text-github-accent font-medium shadow-sm'
                : currentTargetIndex > 0
                  ? 'text-github-text-secondary hover:text-github-text-primary'
                  : 'text-github-text-muted opacity-50 cursor-not-allowed'
            }`}
            title={currentTargetIndex > 0 ? "View diff between this commit and previous update" : "First commit in change (no prior update)"}
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
        const activeCommit = commits.find((c) => c.commitId === targetCommitId);
        if (!activeCommit || activeCommit.commentCount === 0) return null;
        return (
          <div
            className={`flex items-center gap-1 px-2 py-0.5 rounded text-[11px] font-medium border ${
              activeCommit.unresolvedCommentCount > 0
                ? 'bg-amber-500/10 text-amber-500 border-amber-500/30'
                : 'bg-green-500/10 text-green-500 border-green-500/30'
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
