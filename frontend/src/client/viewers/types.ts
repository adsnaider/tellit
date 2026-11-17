import type {
  CommentThread,
  DiffFile,
  DiffLine,
  DiffSide,
  DiffViewMode,
  LineNumber,
  MergedChunk,
} from '../../types/diff';
import type { CursorPosition } from '../utils/navigation/types';

export interface PendingCursorCommentRequest {
  filePath: string;
  side: DiffSide;
  line: LineNumber;
  seq: number;
}

export interface DiffViewerBodyProps {
  file: DiffFile;
  mergedChunks: MergedChunk[];
  diffMode: DiffViewMode;
  threads: CommentThread[];
  showAuthorBadges?: boolean;
  syntaxTheme?: string;
  baseCommitish?: string;
  targetCommitish?: string;
  openInEditorAvailable?: boolean;
  isExpandLoading?: boolean;
  isCollapsed?: boolean;
  activeWholeFileComment?: boolean;
  highlightedThreadId?: string | null;
  fileIndex?: number;
  cursor?: CursorPosition | null;
  pendingLineComment?: PendingCursorCommentRequest | null;
  onExpandLines: (
    file: DiffFile,
    chunkIndex: number,
    direction: 'up' | 'down',
    count?: number,
  ) => void;
  onExpandAllBetween: (file: DiffFile, chunkIndex: number, hiddenLines: number) => void;
  onPrefetchLineCount?: (file: DiffFile) => void;
  onAddComment: (
    filePath: string,
    line: LineNumber | undefined,
    body: string,
    codeContent?: string,
    side?: DiffSide,
  ) => Promise<void>;
  onReplyToThread: (threadId: string, body: string) => Promise<void>;
  onToggleResolve?: (threadId: string, currentResolved: boolean) => Promise<void> | void;
  onRemoveThread?: (threadId: string) => void;
  onOpenInEditor?: (filePath: string, lineNumber: number) => void;
  onCancelWholeFileComment?: () => void;
  onConsumePendingComment?: (seq: number) => void;
  onCursorSelectLine?: (
    fileIndex: number,
    chunkIndex: number,
    lineIndex: number,
    side: 'left' | 'right',
  ) => void;
  onCreateWholeFileThread?: (filePath: string, line?: DiffLine) => void;
}
