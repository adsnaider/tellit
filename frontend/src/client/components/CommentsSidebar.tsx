import {
  Check,
  Copy,
  MessageSquare,
  PanelRightClose,
} from 'lucide-react';
import React, { useMemo, useState } from 'react';

import { type CommentThread } from '../../types/diff';
import { CommentThreadCard } from './CommentThreadCard';
import type { AppearanceSettings } from './SettingsModal';

export interface CommentsSidebarProps {
  isOpen: boolean;
  onClose: () => void;
  width: number;
  comments: CommentThread[];
  showAuthorBadges?: boolean;
  showResolvedInline: boolean;
  onToggleShowResolvedInline: (show: boolean) => void;
  onNavigateToComment: (thread: CommentThread) => void;
  onGenerateThreadPrompt: (thread: CommentThread) => string;
  onRemoveThread: (threadId: string) => void;
  onToggleResolve?: (threadId: string, currentResolved: boolean) => void;
  onReplyToThread: (threadId: string, body: string) => Promise<void>;
  onRemoveMessage: (threadId: string, messageId: string) => void;
  onUpdateMessage: (threadId: string, messageId: string, newBody: string) => void;
  syntaxTheme?: AppearanceSettings['syntaxTheme'];
  onCopyAllComments?: () => void;
  isCopiedAll?: boolean;
  isMobile?: boolean;
}

export function CommentsSidebar({
  isOpen,
  onClose,
  width,
  comments,
  showAuthorBadges = false,
  showResolvedInline,
  onToggleShowResolvedInline,
  onNavigateToComment,
  onGenerateThreadPrompt,
  onRemoveThread,
  onToggleResolve,
  onReplyToThread,
  onRemoveMessage,
  onUpdateMessage,
  syntaxTheme,
  onCopyAllComments,
  isCopiedAll = false,
  isMobile = false,
}: CommentsSidebarProps) {
  const [filter, setFilter] = useState<'all' | 'unresolved' | 'resolved'>('all');

  const unresolvedCount = useMemo(
    () => comments.filter((c) => !c.resolved).length,
    [comments],
  );
  const resolvedCount = comments.length - unresolvedCount;

  const sortedComments = useMemo(() => {
    return [...comments].sort((a, b) => {
      // 1. Unresolved before resolved
      const aResolved = Boolean(a.resolved);
      const bResolved = Boolean(b.resolved);
      if (aResolved !== bResolved) {
        return aResolved ? 1 : -1;
      }
      // 2. By file path
      const fileCompare = a.file.localeCompare(b.file);
      if (fileCompare !== 0) return fileCompare;
      // 3. By line number
      const aLine = Array.isArray(a.line) ? a.line[0] : a.line;
      const bLine = Array.isArray(b.line) ? b.line[0] : b.line;
      if (aLine !== bLine) return aLine - bLine;
      // 4. By created timestamp
      return a.createdAt.localeCompare(b.createdAt);
    });
  }, [comments]);

  const filteredComments = useMemo(() => {
    if (filter === 'unresolved') {
      return sortedComments.filter((c) => !c.resolved);
    }
    if (filter === 'resolved') {
      return sortedComments.filter((c) => c.resolved);
    }
    return sortedComments;
  }, [sortedComments, filter]);

  if (!isOpen) return null;

  return (
    <aside
      id="comments-sidebar-panel"
      className={`bg-github-bg-secondary flex flex-col border-l border-github-border ${
        isMobile
          ? 'fixed inset-y-0 right-0 z-40 w-[min(90vw,420px)] shadow-xl'
          : 'relative h-full'
      }`}
      style={{
        width: isMobile ? 'min(90vw, 420px)' : `${width}px`,
        minWidth: isMobile ? '0px' : '260px',
        maxWidth: isMobile ? 'none' : '700px',
      }}
    >
      {/* Sidebar Header */}
      <div className="p-3 border-b border-github-border flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <MessageSquare size={16} className="text-github-text-secondary shrink-0" />
          <h2 className="text-sm font-semibold text-github-text-primary truncate">
            Comments
          </h2>
          <span className="text-xs text-github-text-muted">
            ({comments.length})
          </span>
          {unresolvedCount > 0 ? (
            <span className="inline-flex items-center px-1.5 py-0.5 rounded-full text-[10px] font-medium bg-yellow-500/15 text-yellow-500 border border-yellow-500/30">
              {unresolvedCount} open
            </span>
          ) : comments.length > 0 ? (
            <span className="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded-full text-[10px] font-medium bg-green-500/15 text-green-500 border border-green-500/30">
              <Check size={10} />
              All resolved
            </span>
          ) : null}
        </div>

        <div className="flex items-center gap-1 shrink-0">
          {onCopyAllComments && comments.length > 0 && (
            <button
              type="button"
              onClick={onCopyAllComments}
              className="p-1.5 text-xs text-github-text-secondary hover:text-github-text-primary hover:bg-github-bg-tertiary rounded transition-colors inline-flex items-center gap-1 border border-github-border"
              title="Copy all comments prompt for AI agent"
            >
              <Copy size={13} />
              <span className="text-[11px]">{isCopiedAll ? 'Copied!' : 'Prompt'}</span>
            </button>
          )}
          <button
            type="button"
            onClick={onClose}
            className="p-1.5 text-github-text-secondary hover:text-github-text-primary hover:bg-github-bg-tertiary rounded transition-colors"
            title="Close comments sidebar (c)"
            aria-label="Close comments sidebar"
          >
            <PanelRightClose size={16} />
          </button>
        </div>
      </div>

      {/* Filter Tabs & In-Diff Toggle */}
      <div className="px-3 py-2 border-b border-github-border flex flex-wrap items-center justify-between gap-2 bg-github-bg-primary/50 text-xs">
        <div className="inline-flex rounded-md bg-github-bg-tertiary p-0.5 border border-github-border">
          <button
            type="button"
            onClick={() => setFilter('all')}
            className={`px-2 py-0.5 rounded text-[11px] font-medium transition-colors ${
              filter === 'all'
                ? 'bg-github-bg-primary text-github-text-primary shadow-xs'
                : 'text-github-text-secondary hover:text-github-text-primary'
            }`}
          >
            All ({comments.length})
          </button>
          <button
            type="button"
            onClick={() => setFilter('unresolved')}
            className={`px-2 py-0.5 rounded text-[11px] font-medium transition-colors ${
              filter === 'unresolved'
                ? 'bg-github-bg-primary text-yellow-500 shadow-xs'
                : 'text-github-text-secondary hover:text-github-text-primary'
            }`}
          >
            Open ({unresolvedCount})
          </button>
          <button
            type="button"
            onClick={() => setFilter('resolved')}
            className={`px-2 py-0.5 rounded text-[11px] font-medium transition-colors ${
              filter === 'resolved'
                ? 'bg-github-bg-primary text-green-500 shadow-xs'
                : 'text-github-text-secondary hover:text-github-text-primary'
            }`}
          >
            Resolved ({resolvedCount})
          </button>
        </div>

        <label className="inline-flex items-center gap-1.5 text-xs text-github-text-secondary cursor-pointer select-none">
          <input
            type="checkbox"
            checked={showResolvedInline}
            onChange={(e) => onToggleShowResolvedInline(e.target.checked)}
            className="rounded border-github-border text-blue-600 focus:ring-0 cursor-pointer"
          />
          <span>Show resolved in diff</span>
        </label>
      </div>

      {/* Comments List */}
      <div className="flex-1 overflow-y-auto p-3 space-y-3">
        {filteredComments.length === 0 ? (
          <div className="py-12 text-center text-xs text-github-text-secondary">
            {comments.length === 0 ? (
              <p>No comments on this change yet.</p>
            ) : (
              <p>No {filter} comments found.</p>
            )}
          </div>
        ) : (
          filteredComments.map((thread) => (
            <div
              key={thread.id}
              className="transition-transform duration-150"
            >
              <CommentThreadCard
                thread={thread}
                showAuthorBadges={showAuthorBadges}
                confirmRootAction={false}
                onGeneratePrompt={onGenerateThreadPrompt}
                onRemoveThread={onRemoveThread}
                onToggleResolve={onToggleResolve}
                onReplyToThread={onReplyToThread}
                onRemoveMessage={onRemoveMessage}
                onUpdateMessage={onUpdateMessage}
                syntaxTheme={syntaxTheme}
                onClick={() => onNavigateToComment(thread)}
              />
            </div>
          ))
        )}
      </div>
    </aside>
  );
}
