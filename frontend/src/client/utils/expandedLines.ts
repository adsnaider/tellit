import type {
  DiffChunk,
  DiffFile,
  DiffLine,
  ExpandedLine,
  FileExpandedState,
  MergedChunk,
} from '../../types/diff';

export const DEFAULT_EXPAND_COUNT = 20;

export function splitBlobLines(text: string): string[] {
  const lines = text.split('\n');
  if (lines.length > 0 && lines[lines.length - 1] === '') {
    lines.pop();
  }
  return lines;
}

function getChunkEndOld(chunk: DiffChunk): number {
  return chunk.oldLines > 0 ? chunk.oldStart + chunk.oldLines - 1 : chunk.oldStart;
}

function getChunkEndNew(chunk: DiffChunk): number {
  return chunk.newLines > 0 ? chunk.newStart + chunk.newLines - 1 : chunk.newStart;
}

export function isExpandableFile(file: DiffFile): boolean {
  return file.status !== 'added' && file.status !== 'deleted' && file.path !== 'Commit Message';
}

export function addExpandRange(
  ranges: FileExpandedState['expandedRanges'],
  chunkIndex: number,
  direction: 'up' | 'down',
  count: number,
): FileExpandedState['expandedRanges'] {
  const existingIndex = ranges.findIndex(
    (range) => range.chunkIndex === chunkIndex && range.direction === direction,
  );
  if (existingIndex < 0) {
    return [...ranges, { chunkIndex, direction, count }];
  }
  return ranges.map((range, index) =>
    index === existingIndex ? { ...range, count: range.count + count } : range,
  );
}

export function consolidateBetweenChunkRanges(
  ranges: FileExpandedState['expandedRanges'],
  chunkIndex: number,
  hiddenLines: number,
): FileExpandedState['expandedRanges'] {
  const existingUp = ranges.find((r) => r.chunkIndex === chunkIndex && r.direction === 'up');
  const existingDownPrev = ranges.find(
    (r) => r.chunkIndex === chunkIndex - 1 && r.direction === 'down',
  );
  const totalToExpand = (existingUp?.count ?? 0) + (existingDownPrev?.count ?? 0) + hiddenLines;

  const filtered = ranges.filter(
    (r) => !(r.chunkIndex === chunkIndex - 1 && r.direction === 'down'),
  );
  const upIndex = filtered.findIndex((r) => r.chunkIndex === chunkIndex && r.direction === 'up');
  if (upIndex >= 0) {
    return filtered.map((r, idx) => (idx === upIndex ? { ...r, count: totalToExpand } : r));
  }
  return [...filtered, { chunkIndex, direction: 'up', count: totalToExpand }];
}

function getExpandedCount(
  fileState: FileExpandedState | undefined,
  chunkIndex: number,
  direction: 'up' | 'down',
): number {
  if (!fileState) return 0;
  return (
    fileState.expandedRanges.find(
      (range) => range.chunkIndex === chunkIndex && range.direction === direction,
    )?.count ?? 0
  );
}

export function getHiddenLinesBefore(
  file: DiffFile,
  chunk: DiffChunk,
  chunkIndex: number,
  fileState: FileExpandedState | undefined,
): number {
  if (!isExpandableFile(file)) {
    return 0;
  }

  const prevChunk = file.chunks[chunkIndex - 1];
  const endOld = chunk.oldLines > 0 ? chunk.oldStart - 1 : chunk.oldStart;
  const hiddenLines = !prevChunk ? endOld : endOld - getChunkEndOld(prevChunk);

  const expandedUp = getExpandedCount(fileState, chunkIndex, 'up');
  const expandedDownPrev = chunkIndex > 0 ? getExpandedCount(fileState, chunkIndex - 1, 'down') : 0;

  return Math.max(0, hiddenLines - expandedUp - expandedDownPrev);
}

export function getHiddenLinesAfter(
  file: DiffFile,
  chunk: DiffChunk,
  chunkIndex: number,
  fileState: FileExpandedState | undefined,
): number {
  if (!isExpandableFile(file)) {
    return 0;
  }

  const nextChunk = file.chunks[chunkIndex + 1];
  if (nextChunk) {
    return 0;
  }

  const totalLines = fileState?.oldTotalLines ?? fileState?.newTotalLines;
  if (totalLines === undefined) {
    return -1;
  }

  const chunkEnd = getChunkEndOld(chunk);
  const hiddenLines = totalLines - chunkEnd;
  const expandedDown = getExpandedCount(fileState, chunkIndex, 'down');

  return Math.max(0, hiddenLines - expandedDown);
}

function getExpandedChunk(
  file: DiffFile,
  chunk: DiffChunk,
  chunkIndex: number,
  fileState: FileExpandedState | undefined,
): DiffChunk {
  if (!fileState) return chunk;

  const expandedUp = getExpandedCount(fileState, chunkIndex, 'up');
  const expandedDown = getExpandedCount(fileState, chunkIndex, 'down');
  if (expandedUp === 0 && expandedDown === 0) {
    return chunk;
  }

  const oldContent = fileState.oldContent ?? [];
  const newContent = fileState.newContent ?? [];
  const lines: ExpandedLine[] = [];

  if (expandedUp > 0) {
    const prevChunk = file.chunks[chunkIndex - 1];
    const expandedDownPrev =
      chunkIndex > 0 ? getExpandedCount(fileState, chunkIndex - 1, 'down') : 0;
    const minOldLineExclusive = (prevChunk ? getChunkEndOld(prevChunk) : 0) + expandedDownPrev;
    const endOld = chunk.oldLines > 0 ? chunk.oldStart - 1 : chunk.oldStart;
    const endNew = chunk.newLines > 0 ? chunk.newStart - 1 : chunk.newStart;
    const available = Math.max(0, endOld - minOldLineExclusive);
    const linesToShow = Math.min(expandedUp, available);
    const startOld = endOld - linesToShow + 1;
    const startNew = endNew - linesToShow + 1;

    for (let i = 0; i < linesToShow; i++) {
      const oldLineNumber = startOld + i;
      const newLineNumber = startNew + i;
      const content = oldContent[oldLineNumber - 1] ?? newContent[newLineNumber - 1] ?? '';
      lines.push({
        type: 'normal',
        content,
        oldLineNumber,
        newLineNumber,
        isExpanded: true,
      });
    }
  }

  lines.push(...chunk.lines);

  if (expandedDown > 0) {
    const chunkEndOld = getChunkEndOld(chunk);
    const chunkEndNew = getChunkEndNew(chunk);
    const nextChunk = file.chunks[chunkIndex + 1];
    const maxOld = nextChunk
      ? nextChunk.oldLines > 0
        ? nextChunk.oldStart - 1
        : nextChunk.oldStart
      : (fileState.oldTotalLines ?? chunkEndOld);
    const linesToShow = Math.min(expandedDown, Math.max(0, maxOld - chunkEndOld));

    for (let i = 1; i <= linesToShow; i++) {
      const oldLineNumber = chunkEndOld + i;
      const newLineNumber = chunkEndNew + i;
      const content = oldContent[oldLineNumber - 1] ?? newContent[newLineNumber - 1] ?? '';
      lines.push({
        type: 'normal',
        content,
        oldLineNumber,
        newLineNumber,
        isExpanded: true,
      });
    }
  }

  const firstLine = lines[0];
  const lastLine = lines[lines.length - 1];
  const oldStart = firstLine?.oldLineNumber ?? chunk.oldStart;
  const newStart = firstLine?.newLineNumber ?? chunk.newStart;

  return {
    ...chunk,
    lines: lines as DiffLine[],
    oldStart,
    newStart,
    oldLines:
      lastLine?.oldLineNumber !== undefined
        ? lastLine.oldLineNumber - oldStart + 1
        : chunk.oldLines,
    newLines:
      lastLine?.newLineNumber !== undefined
        ? lastLine.newLineNumber - newStart + 1
        : chunk.newLines,
  };
}

export function getMergedChunksForFile(
  file: DiffFile,
  fileState: FileExpandedState | undefined,
): MergedChunk[] {
  const mergedChunks: MergedChunk[] = [];

  for (let i = 0; i < file.chunks.length; i++) {
    const chunk = file.chunks[i];
    if (!chunk) continue;

    const expandedChunk = getExpandedChunk(file, chunk, i, fileState);
    const hiddenBefore = getHiddenLinesBefore(file, chunk, i, fileState);
    const hiddenAfter = getHiddenLinesAfter(file, chunk, i, fileState);

    const lastMerged = mergedChunks[mergedChunks.length - 1];
    if (lastMerged && hiddenBefore === 0) {
      lastMerged.lines = [...lastMerged.lines, ...expandedChunk.lines];
      lastMerged.originalIndices.push(i);
      lastMerged.hiddenLinesAfter = hiddenAfter;
      const lastLine = lastMerged.lines[lastMerged.lines.length - 1];
      if (lastLine?.oldLineNumber !== undefined) {
        lastMerged.oldLines = lastLine.oldLineNumber - lastMerged.oldStart + 1;
      }
      if (lastLine?.newLineNumber !== undefined) {
        lastMerged.newLines = lastLine.newLineNumber - lastMerged.newStart + 1;
      }
    } else {
      mergedChunks.push({
        ...expandedChunk,
        originalIndices: [i],
        hiddenLinesBefore: hiddenBefore,
        hiddenLinesAfter: hiddenAfter,
      });
    }
  }

  return mergedChunks;
}
