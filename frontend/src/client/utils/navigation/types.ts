import type { DiffViewMode } from '../../../types/diff';

/**
 * Represents the current cursor position in the diff viewer.
 */
export interface CursorPosition {
  fileIndex: number;
  chunkIndex: number;
  lineIndex: number;
  side: 'left' | 'right';
}

export type ViewMode = DiffViewMode;
