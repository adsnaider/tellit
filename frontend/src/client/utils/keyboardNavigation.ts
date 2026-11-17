import type { CommentThread, DiffFile, DiffViewMode } from '../../types/diff';

import { getCommentKey, getElementId } from './navigation/domHelpers';
import { fixSide, hasContentOnSide } from './navigation/lineHelpers';
import { findCommentPosition } from './navigation/positionHelpers';
import type { CursorPosition } from './navigation/types';

export type NavigationDirection = 'next' | 'prev';

export type NavigationFilter = (position: CursorPosition, files: DiffFile[]) => boolean;

export interface NavigationResult {
  position: CursorPosition | null;
  scrollTarget: string | null;
}

export { fixSide } from './navigation/lineHelpers';
export type { CursorPosition };

export function getStartPosition(cursor: CursorPosition | null): CursorPosition {
  return cursor ?? { fileIndex: 0, chunkIndex: 0, lineIndex: -1, side: 'right' };
}

function hasWrappedAround(
  current: CursorPosition,
  start: CursorPosition,
  started: boolean,
): boolean {
  return (
    started &&
    current.fileIndex === start.fileIndex &&
    current.chunkIndex === start.chunkIndex &&
    current.lineIndex === start.lineIndex
  );
}

function advancePosition(
  pos: CursorPosition,
  direction: NavigationDirection,
  files: DiffFile[],
): CursorPosition | null {
  let { fileIndex, chunkIndex, lineIndex } = pos;
  const totalFiles = files.length;
  if (totalFiles === 0) return null;
  let wrappedCount = 0;

  if (direction === 'next') {
    lineIndex += 1;

    while (wrappedCount <= totalFiles) {
      if (files[fileIndex]?.chunks[chunkIndex]?.lines[lineIndex]) {
        return { ...pos, fileIndex, chunkIndex, lineIndex };
      }

      chunkIndex += 1;
      lineIndex = 0;

      if (!files[fileIndex]?.chunks[chunkIndex]) {
        fileIndex += 1;
        chunkIndex = 0;
        lineIndex = 0;

        if (fileIndex >= totalFiles) {
          fileIndex = 0;
          wrappedCount += 1;
        }
      }
    }
    return null;
  }

  lineIndex -= 1;

  while (wrappedCount <= totalFiles) {
    if (lineIndex < 0) {
      chunkIndex -= 1;

      if (chunkIndex < 0) {
        fileIndex -= 1;

        if (fileIndex < 0) {
          fileIndex = totalFiles - 1;
          wrappedCount += 1;
        }

        const file = files[fileIndex];
        if (file && file.chunks.length > 0) {
          chunkIndex = file.chunks.length - 1;
          const chunk = file.chunks[chunkIndex];
          lineIndex = chunk && chunk.lines.length > 0 ? chunk.lines.length - 1 : -1;
        } else {
          chunkIndex = -1;
          lineIndex = -1;
        }
      } else {
        const chunk = files[fileIndex]?.chunks[chunkIndex];
        lineIndex = chunk && chunk.lines.length > 0 ? chunk.lines.length - 1 : -1;
      }
    }

    if (lineIndex >= 0 && files[fileIndex]?.chunks[chunkIndex]?.lines[lineIndex]) {
      return { ...pos, fileIndex, chunkIndex, lineIndex };
    }

    if (lineIndex >= 0) {
      lineIndex -= 1;
    }
  }

  return null;
}

export function findNextMatchingPosition(
  startPos: CursorPosition,
  direction: NavigationDirection,
  filter: NavigationFilter,
  files: DiffFile[],
  viewMode: DiffViewMode,
): NavigationResult {
  let current: CursorPosition | null = startPos;
  let started = false;

  while (current) {
    const nextPos = advancePosition(current, direction, files);
    if (!nextPos) break;
    current = nextPos;

    if (hasWrappedAround(current, startPos, started)) {
      break;
    }
    started = true;

    if (filter(current, files)) {
      const fixed = fixSide(current, files);
      return {
        position: fixed,
        scrollTarget: getElementId(fixed, viewMode),
      };
    }
  }

  return { position: null, scrollTarget: null };
}

export function buildCommentIndex(threads: readonly CommentThread[]): Map<string, CommentThread[]> {
  const index = new Map<string, CommentThread[]>();
  for (const thread of threads) {
    const lineNum = Array.isArray(thread.line) ? thread.line[0] : thread.line;
    const key = getCommentKey(thread.file, lineNum, thread.side);
    const list = index.get(key);
    if (list) list.push(thread);
    else index.set(key, [thread]);
  }
  return index;
}

function hasCommentAtLine(
  filePath: string,
  line: DiffFile['chunks'][number]['lines'][number],
  commentIndex: Map<string, CommentThread[]>,
): boolean {
  const keys: string[] = [];
  if (line.oldLineNumber !== undefined) {
    keys.push(getCommentKey(filePath, line.oldLineNumber, 'old'));
    keys.push(getCommentKey(filePath, line.oldLineNumber));
  }
  if (line.newLineNumber !== undefined) {
    keys.push(getCommentKey(filePath, line.newLineNumber, 'new'));
    keys.push(getCommentKey(filePath, line.newLineNumber));
  }
  return keys.some((key) => commentIndex.has(key));
}

export function createDiffNavigationFilters(
  files: DiffFile[],
  commentIndex: Map<string, CommentThread[]>,
  viewMode: DiffViewMode,
  collapsedFiles?: ReadonlySet<string>,
) {
  return {
    line: (pos: CursorPosition): boolean => {
      const file = files[pos.fileIndex];
      if (!file || collapsedFiles?.has(file.path)) return false;
      return viewMode === 'unified' || hasContentOnSide(pos, files);
    },
    chunk: (pos: CursorPosition): boolean => {
      const file = files[pos.fileIndex];
      if (!file || collapsedFiles?.has(file.path)) return false;
      const line = file.chunks[pos.chunkIndex]?.lines[pos.lineIndex];
      if (!line || line.type === 'normal') return false;
      if (pos.lineIndex === 0) return true;
      const prevLine = file.chunks[pos.chunkIndex]?.lines[pos.lineIndex - 1];
      return !prevLine || prevLine.type === 'normal';
    },
    comment: (pos: CursorPosition): boolean => {
      const file = files[pos.fileIndex];
      if (!file || collapsedFiles?.has(file.path)) return false;
      const line = file.chunks[pos.chunkIndex]?.lines[pos.lineIndex];
      if (!line) return false;
      return hasCommentAtLine(file.path, line, commentIndex);
    },
    file: (pos: CursorPosition): boolean => pos.chunkIndex === 0 && pos.lineIndex === 0,
  };
}

export function switchSplitCursorSide(
  cursor: CursorPosition,
  side: 'left' | 'right',
  files: DiffFile[],
): CursorPosition {
  let next: CursorPosition = { ...cursor, side };
  const currentLine = files[cursor.fileIndex]?.chunks[cursor.chunkIndex]?.lines[cursor.lineIndex];

  if (currentLine) {
    if (side === 'left' && currentLine.type === 'add' && cursor.lineIndex > 0) {
      const prevLine =
        files[cursor.fileIndex]?.chunks[cursor.chunkIndex]?.lines[cursor.lineIndex - 1];
      if (prevLine?.type === 'delete') {
        return { ...next, lineIndex: cursor.lineIndex - 1 };
      }
    } else if (side === 'right' && currentLine.type === 'delete') {
      const nextLine =
        files[cursor.fileIndex]?.chunks[cursor.chunkIndex]?.lines[cursor.lineIndex + 1];
      if (nextLine?.type === 'add') {
        return { ...next, lineIndex: cursor.lineIndex + 1 };
      }
    }
  }

  if (hasContentOnSide(next, files)) {
    return next;
  }

  const file = files[cursor.fileIndex];
  const chunk = file?.chunks[cursor.chunkIndex];
  if (chunk) {
    for (let i = cursor.lineIndex + 1; i < chunk.lines.length; i++) {
      const candidate = { ...next, lineIndex: i };
      if (hasContentOnSide(candidate, files)) {
        return candidate;
      }
    }
    for (let i = cursor.lineIndex - 1; i >= 0; i--) {
      const candidate = { ...next, lineIndex: i };
      if (hasContentOnSide(candidate, files)) {
        return candidate;
      }
    }
  }

  return next;
}

export function findCursorForCommentThread(
  thread: CommentThread,
  files: DiffFile[],
): CursorPosition | null {
  return findCommentPosition(thread, files);
}

export function findNextUnviewedFileIndex(
  afterIndex: number,
  files: DiffFile[],
  viewedFiles: ReadonlySet<string>,
): number | null {
  const totalFiles = files.length;
  for (let offset = 1; offset < totalFiles; offset++) {
    const fileIndex = (afterIndex + offset) % totalFiles;
    const file = files[fileIndex];
    if (!file || !file.chunks[0]?.lines[0]) continue;
    if (!viewedFiles.has(file.path)) {
      return fileIndex;
    }
  }
  return null;
}

export function scrollCursorIntoView(elementId: string): void {
  if (typeof document === 'undefined') return;
  const element = document.getElementById(elementId);
  if (!element) return;
  if (typeof element.scrollIntoView === 'function') {
    element.scrollIntoView({ block: 'nearest' });
  }
}

export function isEditableEventTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
}
