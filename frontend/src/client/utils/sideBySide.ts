import type { DiffChunk, DiffLine, ExpandedLine } from '../../types/diff';

import { computeWordLevelDiff, shouldComputeWordDiff, type DiffSegment } from './wordLevelDiff';

export interface SideBySideLine {
  oldLine?: ExpandedLine;
  newLine?: ExpandedLine;
  oldLineNumber?: number;
  newLineNumber?: number;
  oldOriginalIndex?: number;
  newOriginalIndex?: number;
  oldSegments?: DiffSegment[];
  newSegments?: DiffSegment[];
}

export function isDeletionLine(line: Pick<DiffLine, 'type'> | undefined): boolean {
  return line?.type === 'delete' || line?.type === 'remove';
}

export function isExpandedDiffLine(line: ExpandedLine | undefined): boolean {
  return Boolean(line?.isExpanded);
}

interface IndexedLine {
  line: ExpandedLine;
  index: number;
}

function collectDeleteAddBlocks(
  lines: readonly ExpandedLine[],
  startIndex: number,
): {
  deletes: IndexedLine[];
  adds: IndexedLine[];
  nextIndex: number;
} {
  let cursor = startIndex;
  const deletes: IndexedLine[] = [];
  while (cursor < lines.length) {
    const candidate = lines[cursor];
    if (!candidate || !isDeletionLine(candidate)) break;
    deletes.push({ line: candidate, index: cursor });
    cursor++;
  }

  const adds: IndexedLine[] = [];
  while (cursor < lines.length) {
    const candidate = lines[cursor];
    if (!candidate || candidate.type !== 'add') break;
    adds.push({ line: candidate, index: cursor });
    cursor++;
  }

  return { deletes, adds, nextIndex: cursor };
}

function computePairSegments(
  deleteLine: ExpandedLine | undefined,
  addLine: ExpandedLine | undefined,
): { oldSegments?: DiffSegment[]; newSegments?: DiffSegment[] } {
  if (!deleteLine || !addLine) return {};
  if (!shouldComputeWordDiff(deleteLine.content, addLine.content)) return {};
  return computeWordLevelDiff(deleteLine.content, addLine.content);
}

export function computeUnifiedWordDiffMap(
  lines: readonly ExpandedLine[],
): Map<number, DiffSegment[]> {
  const map = new Map<number, DiffSegment[]>();
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    if (!line || !isDeletionLine(line)) {
      i++;
      continue;
    }

    const { deletes, adds, nextIndex } = collectDeleteAddBlocks(lines, i);
    const pairCount = Math.min(deletes.length, adds.length);
    for (let k = 0; k < pairCount; k++) {
      const del = deletes[k];
      const add = adds[k];
      if (!del || !add) continue;
      const { oldSegments, newSegments } = computePairSegments(del.line, add.line);
      if (oldSegments && newSegments) {
        map.set(del.index, oldSegments);
        map.set(add.index, newSegments);
      }
    }
    i = nextIndex;
  }

  return map;
}

export function convertChunkToSideBySide(chunk: DiffChunk): SideBySideLine[] {
  const result: SideBySideLine[] = [];
  const lines = chunk.lines as ExpandedLine[];
  let oldLineNum = chunk.oldStart;
  let newLineNum = chunk.newStart;
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    if (!line) {
      i++;
      continue;
    }

    if (isDeletionLine(line)) {
      const { deletes, adds, nextIndex } = collectDeleteAddBlocks(lines, i);
      const maxLen = Math.max(deletes.length, adds.length);

      for (let k = 0; k < maxLen; k++) {
        const del = deletes[k];
        const add = adds[k];
        const { oldSegments, newSegments } = computePairSegments(del?.line, add?.line);

        result.push({
          oldLine: del?.line,
          newLine: add?.line,
          oldLineNumber: del ? (del.line.oldLineNumber ?? oldLineNum++) : undefined,
          newLineNumber: add ? (add.line.newLineNumber ?? newLineNum++) : undefined,
          oldOriginalIndex: del?.index,
          newOriginalIndex: add?.index,
          oldSegments,
          newSegments,
        });
      }

      i = nextIndex;
      continue;
    }

    if (line.type === 'add') {
      result.push({
        newLine: line,
        newLineNumber: line.newLineNumber ?? newLineNum++,
        newOriginalIndex: i,
      });
      i++;
      continue;
    }

    result.push({
      oldLine: line,
      newLine: { ...line },
      oldLineNumber: line.oldLineNumber ?? oldLineNum++,
      newLineNumber: line.newLineNumber ?? newLineNum++,
      oldOriginalIndex: i,
      newOriginalIndex: i,
    });
    i++;
  }

  return result;
}
