import React, { useEffect, useState } from 'react';
import { GitCommit, GitBranch, MessageSquare, CheckCircle2, Clock } from 'lucide-react';
import { type JjChange, type JjCommit } from '../../types/diff';

interface JjRevisionSelectorProps {
  currentCommitId?: string;
  onSelectCommit: (commitId: string) => void;
}

export function JjRevisionSelector({
  currentCommitId,
  onSelectCommit,
}: JjRevisionSelectorProps) {
  const [changes, setChanges] = useState<JjChange[]>([]);
  const [selectedChangeId, setSelectedChangeId] = useState<string>('');
  const [commits, setCommits] = useState<JjCommit[]>([]);
  const [loadingChanges, setLoadingChanges] = useState(true);
  const [loadingCommits, setLoadingCommits] = useState(false);

  // Load changes
  useEffect(() => {
    let mounted = true;
    fetch('/api/jj/changes')
      .then((res) => res.json())
      .then((data: { changes: JjChange[] }) => {
        if (!mounted) return;
        setChanges(data.changes || []);
        setLoadingChanges(false);

        // Find change matching currentCommitId or default to working copy
        if (data.changes && data.changes.length > 0) {
          const match = data.changes.find((c) => c.commitId === currentCommitId || c.isWorkingCopy);
          const initialChange = match || data.changes[0];
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

  // When selected change changes, load its commits
  useEffect(() => {
    if (!selectedChangeId) return;
    let mounted = true;
    setLoadingCommits(true);
    fetch(`/api/jj/changes/${selectedChangeId}/commits`)
      .then((res) => res.json())
      .then((data: { commits: JjCommit[] }) => {
        if (!mounted) return;
        setCommits(data.commits || []);
        setLoadingCommits(false);

        // If currentCommitId is in this change's commits, keep it, else select the newest commit
        if (data.commits && data.commits.length > 0) {
          const exists = data.commits.some((c) => c.commitId === currentCommitId);
          if (!exists) {
            // Newest commit is last in chronological order
            const newest = data.commits[data.commits.length - 1];
            onSelectCommit(newest.commitId);
          }
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

  if (loadingChanges) {
    return <div className="text-xs text-github-text-secondary">Loading Jujutsu revisions...</div>;
  }

  if (changes.length === 0) {
    return null;
  }

  return (
    <div className="flex items-center gap-2 text-xs">
      {/* Change ID Selector */}
      <div className="flex items-center gap-1.5 bg-github-bg-tertiary border border-github-border rounded px-2 py-1">
        <GitBranch size={13} className="text-github-accent shrink-0" />
        <span className="font-semibold text-github-text-secondary">Change:</span>
        <select
          value={selectedChangeId}
          onChange={(e) => setSelectedChangeId(e.target.value)}
          className="bg-transparent text-github-text-primary text-xs font-mono outline-none cursor-pointer max-w-[200px] truncate"
          title="Select Jujutsu Change"
        >
          {changes.map((change) => (
            <option key={change.changeId} value={change.changeId} className="bg-github-bg-secondary text-github-text-primary font-mono">
              {change.isWorkingCopy ? '@ ' : ''}
              {change.shortChangeId} {change.description.slice(0, 30)}
            </option>
          ))}
        </select>
      </div>

      {/* Commit Hash Selector (Chronological) */}
      <div className="flex items-center gap-1.5 bg-github-bg-tertiary border border-github-border rounded px-2 py-1">
        <GitCommit size={13} className="text-github-text-secondary shrink-0" />
        <span className="font-semibold text-github-text-secondary">Commit:</span>
        {loadingCommits ? (
          <span className="text-github-text-muted">Loading...</span>
        ) : (
          <select
            value={currentCommitId}
            onChange={(e) => onSelectCommit(e.target.value)}
            className="bg-transparent text-github-text-primary text-xs font-mono outline-none cursor-pointer max-w-[260px] truncate"
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

      {/* Commit Comment Indicator Badge */}
      {(() => {
        const activeCommit = commits.find((c) => c.commitId === currentCommitId);
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
