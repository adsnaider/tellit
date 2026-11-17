import type { DiffLine, MergedChunk } from '../../types/diff';

import { isSafeUrl } from './markdownUtils';

export type GfmInlinePart =
  | { type: 'text'; text: string }
  | { type: 'code'; text: string }
  | { type: 'bold'; text: string }
  | { type: 'italic'; text: string }
  | { type: 'strike'; text: string }
  | { type: 'link'; text: string; href: string }
  | { type: 'image'; alt: string; src: string };

export type TableAlignment = 'left' | 'center' | 'right' | null;

export interface GfmListItem {
  checked?: boolean;
  inlines: GfmInlinePart[];
}

export type GfmBlock =
  | { type: 'heading'; level: number; inlines: GfmInlinePart[] }
  | { type: 'code'; language: string; code: string }
  | { type: 'blockquote'; lines: GfmInlinePart[][] }
  | { type: 'list'; ordered: boolean; items: GfmListItem[] }
  | {
      type: 'table';
      headers: GfmInlinePart[][];
      alignments: TableAlignment[];
      rows: GfmInlinePart[][][];
    }
  | { type: 'hr' }
  | { type: 'paragraph'; inlines: GfmInlinePart[] };

export type MarkdownPreviewBlockType = 'add' | 'delete' | 'change' | 'context';

export interface MarkdownPreviewBlock {
  type: MarkdownPreviewBlockType;
  lines: string[];
}

export interface MarkdownPreviewLineEntry {
  type: MarkdownPreviewBlockType;
  content: string;
}

export type MarkdownRenderBlock =
  | { kind: 'markdown'; type: MarkdownPreviewBlockType; lines: string[] }
  | { kind: 'fenced-code'; lines: MarkdownPreviewLineEntry[] };

interface FenceInfo {
  marker: '`' | '~';
  length: number;
  raw: string;
}

// Single-char alternatives inside nested (...)+ prevent backtracking on malformed links.
const GFM_INLINE_RE =
  /(`[^`\n]+`|!\[[^\]\n]*\]\(((?:[^\s()]|\([^\s()]*\))+)\)|\[[^\]\n]+\]\(((?:[^\s()]|\([^\s()]*\))+)\)|\*\*[^*\n]+?\*\*|__[^_\n]+?__|~~[^~\n]+?~~|\*[^*\n]+?\*|(?<=^|[\s([{"'])_[^_\n]+?_(?=$|[\s.,;:!?)\]}"'])|https?:\/\/[^\s<>()]+[^\s<>().,;:!?'"])/g;

function extractParenUrlTarget(raw: string, openMarker: string): { label: string; url: string } {
  const splitIdx = raw.indexOf(openMarker);
  return {
    label: raw.slice(openMarker === '![' ? 0 : 1, splitIdx),
    url: raw.slice(splitIdx + 2, -1),
  };
}

export function parseGfmInline(text: string): GfmInlinePart[] {
  if (!text) return [];
  const parts: GfmInlinePart[] = [];
  let lastIndex = 0;

  GFM_INLINE_RE.lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = GFM_INLINE_RE.exec(text)) !== null) {
    if (match.index > lastIndex) {
      parts.push({ type: 'text', text: text.slice(lastIndex, match.index) });
    }

    const raw = match[0];
    if (raw.startsWith('`') && raw.endsWith('`') && raw.length >= 2) {
      parts.push({ type: 'code', text: raw.slice(1, -1) });
    } else if (raw.startsWith('![')) {
      const closeBracket = raw.indexOf('](');
      const alt = raw.slice(2, closeBracket);
      const src = raw.slice(closeBracket + 2, -1);
      if (isSafeUrl(src)) {
        parts.push({ type: 'image', alt, src });
      }
    } else if (raw.startsWith('[')) {
      const { label, url } = extractParenUrlTarget(raw, '](');
      if (isSafeUrl(url)) {
        parts.push({ type: 'link', text: label, href: url });
      } else {
        parts.push({ type: 'text', text: label });
      }
    } else if (
      (raw.startsWith('**') && raw.endsWith('**') && raw.length >= 4) ||
      (raw.startsWith('__') && raw.endsWith('__') && raw.length >= 4)
    ) {
      parts.push({ type: 'bold', text: raw.slice(2, -2) });
    } else if (raw.startsWith('~~') && raw.endsWith('~~') && raw.length >= 4) {
      parts.push({ type: 'strike', text: raw.slice(2, -2) });
    } else if (
      (raw.startsWith('*') && raw.endsWith('*') && raw.length >= 2) ||
      (raw.startsWith('_') && raw.endsWith('_') && raw.length >= 2)
    ) {
      parts.push({ type: 'italic', text: raw.slice(1, -1) });
    } else if (isSafeUrl(raw)) {
      parts.push({ type: 'link', text: raw, href: raw });
    } else {
      parts.push({ type: 'text', text: raw });
    }

    lastIndex = match.index + raw.length;
  }

  if (lastIndex < text.length) {
    parts.push({ type: 'text', text: text.slice(lastIndex) });
  }

  return parts;
}

function isHorizontalRule(line: string): boolean {
  const trimmed = line.trim();
  if (trimmed.length < 3) return false;
  const compact = trimmed.replace(/\s+/g, '');
  if (compact.length < 3) return false;
  const ch = compact[0];
  return (ch === '-' || ch === '*' || ch === '_') && compact.split('').every((c) => c === ch);
}

function parseFenceStart(
  line: string,
): { marker: '`' | '~'; length: number; language: string; raw: string } | null {
  const match = line.match(/^\s{0,3}(`{3,}|~{3,})(.*)$/);
  if (!match?.[1]) return null;
  const markerStr = match[1];
  const marker = markerStr[0] as '`' | '~';
  const info = (match[2] ?? '').trim();
  if (marker === '`' && info.includes('`')) return null;
  return {
    marker,
    length: markerStr.length,
    language: info.split(/\s+/)[0] ?? '',
    raw: line,
  };
}

function isFenceEnd(line: string, fence: Pick<FenceInfo, 'marker' | 'length'>): boolean {
  const match = line.match(/^\s{0,3}(`{3,}|~{3,})\s*$/);
  if (!match?.[1]) return false;
  return match[1][0] === fence.marker && match[1].length >= fence.length;
}

function splitTableRow(line: string): string[] {
  let trimmed = line.trim();
  if (trimmed.startsWith('|')) trimmed = trimmed.slice(1);
  if (trimmed.endsWith('|')) trimmed = trimmed.slice(0, -1);

  const cells: string[] = [];
  let current = '';
  for (let i = 0; i < trimmed.length; i++) {
    const ch = trimmed[i] ?? '';
    if (ch === '\\' && trimmed[i + 1] === '|') {
      current += '|';
      i += 1;
    } else if (ch === '|') {
      cells.push(current.trim());
      current = '';
    } else {
      current += ch;
    }
  }
  cells.push(current.trim());
  return cells;
}

function parseTableSeparator(line: string, expectedCols: number): TableAlignment[] | null {
  if (!line.includes('|') && !line.trim().startsWith('-')) return null;
  const rawCells = splitTableRow(line);
  if (rawCells.length !== expectedCols || rawCells.length === 0) return null;

  const alignments: TableAlignment[] = [];
  for (const cell of rawCells) {
    if (!/^:?-{3,}:?$/.test(cell)) {
      return null;
    }
    const leftColon = cell.startsWith(':');
    const rightColon = cell.endsWith(':');
    if (leftColon && rightColon) {
      alignments.push('center');
    } else if (rightColon) {
      alignments.push('right');
    } else if (leftColon) {
      alignments.push('left');
    } else {
      alignments.push(null);
    }
  }
  return alignments;
}

function isListItemLine(line: string): { ordered: boolean; rawBody: string } | null {
  const ulMatch = line.match(/^\s{0,3}[-*+]\s+(.*)$/);
  if (ulMatch?.[1] !== undefined) {
    return { ordered: false, rawBody: ulMatch[1] };
  }
  const olMatch = line.match(/^\s{0,3}\d+[.)]\s+(.*)$/);
  if (olMatch?.[1] !== undefined) {
    return { ordered: true, rawBody: olMatch[1] };
  }
  return null;
}

function parseListItem(rawBody: string): GfmListItem {
  const taskMatch = rawBody.match(/^\[([ xX])\]\s+(.*)$/);
  if (taskMatch?.[1] !== undefined && taskMatch[2] !== undefined) {
    return {
      checked: taskMatch[1].toLowerCase() === 'x',
      inlines: parseGfmInline(taskMatch[2]),
    };
  }
  return { inlines: parseGfmInline(rawBody) };
}

function isBlockStarter(lines: string[], idx: number): boolean {
  const line = lines[idx];
  if (line === undefined || line.trim() === '') return true;
  if (/^\s{0,3}#{1,6}\s+/.test(line)) return true;
  if (parseFenceStart(line)) return true;
  if (isHorizontalRule(line)) return true;
  if (/^\s{0,3}>\s?/.test(line)) return true;
  if (isListItemLine(line)) return true;
  if (line.includes('|') && idx + 1 < lines.length) {
    const headerCells = splitTableRow(line);
    if (parseTableSeparator(lines[idx + 1] ?? '', headerCells.length)) {
      return true;
    }
  }
  return false;
}

export function parseGfmBlocks(markdown: string): GfmBlock[] {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n');
  const blocks: GfmBlock[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i] ?? '';
    if (!line.trim()) {
      i += 1;
      continue;
    }

    const fence = parseFenceStart(line);
    if (fence) {
      i += 1;
      const codeLines: string[] = [];
      while (i < lines.length && !isFenceEnd(lines[i] ?? '', fence)) {
        codeLines.push(lines[i] ?? '');
        i += 1;
      }
      if (i < lines.length && isFenceEnd(lines[i] ?? '', fence)) {
        i += 1;
      }
      blocks.push({ type: 'code', language: fence.language, code: codeLines.join('\n') });
      continue;
    }

    const headingMatch = line.match(/^\s{0,3}(#{1,6})\s+(.*?)(?:\s+#+)?\s*$/);
    if (headingMatch?.[1]) {
      blocks.push({
        type: 'heading',
        level: headingMatch[1].length,
        inlines: parseGfmInline(headingMatch[2] ?? ''),
      });
      i += 1;
      continue;
    }

    if (isHorizontalRule(line)) {
      blocks.push({ type: 'hr' });
      i += 1;
      continue;
    }

    if (line.includes('|') && i + 1 < lines.length) {
      const headerCells = splitTableRow(line);
      const alignments = parseTableSeparator(lines[i + 1] ?? '', headerCells.length);
      if (alignments) {
        const headers = headerCells.map((cell) => parseGfmInline(cell));
        i += 2;
        const rows: GfmInlinePart[][][] = [];
        while (i < lines.length) {
          const rowLine = lines[i] ?? '';
          if (!rowLine.trim() || !rowLine.includes('|')) break;
          const rowCells = splitTableRow(rowLine);
          const normalized = Array.from({ length: headerCells.length }, (_, colIdx) =>
            parseGfmInline(rowCells[colIdx] ?? ''),
          );
          rows.push(normalized);
          i += 1;
        }
        blocks.push({ type: 'table', headers, alignments, rows });
        continue;
      }
    }

    if (/^\s{0,3}>\s?/.test(line)) {
      const quoteLines: GfmInlinePart[][] = [];
      while (i < lines.length && /^\s{0,3}>\s?/.test(lines[i] ?? '')) {
        const cleaned = (lines[i] ?? '').replace(/^\s{0,3}>\s?/, '');
        quoteLines.push(parseGfmInline(cleaned));
        i += 1;
      }
      blocks.push({ type: 'blockquote', lines: quoteLines });
      continue;
    }

    const firstList = isListItemLine(line);
    if (firstList) {
      const ordered = firstList.ordered;
      const items: GfmListItem[] = [];
      while (i < lines.length) {
        const item = isListItemLine(lines[i] ?? '');
        if (!item || item.ordered !== ordered) break;
        items.push(parseListItem(item.rawBody));
        i += 1;
      }
      blocks.push({ type: 'list', ordered, items });
      continue;
    }

    const paragraphLines: string[] = [];
    while (i < lines.length && (paragraphLines.length === 0 || !isBlockStarter(lines, i))) {
      paragraphLines.push((lines[i] ?? '').trim());
      i += 1;
    }
    blocks.push({ type: 'paragraph', inlines: parseGfmInline(paragraphLines.join(' ')) });
  }

  return blocks;
}

function isDeletionLine(line: DiffLine): boolean {
  return line.type === 'delete' || line.type === 'remove';
}

function isContextLine(line: DiffLine): boolean {
  return line.type === 'normal' || line.type === 'context';
}

function appendPreviewBlockLine(
  blocks: MarkdownPreviewBlock[],
  type: MarkdownPreviewBlockType,
  line: string,
) {
  const last = blocks[blocks.length - 1];
  if (last && last.type === type) {
    last.lines.push(line);
    return;
  }
  blocks.push({ type, lines: [line] });
}

export function buildMarkdownPreviewBlocks(chunks: MergedChunk[]): MarkdownPreviewBlock[] {
  const blocks: MarkdownPreviewBlock[] = [];
  const pendingDeletes: string[] = [];
  let inChange = false;

  const flushDeletes = () => {
    for (const line of pendingDeletes) {
      appendPreviewBlockLine(blocks, 'delete', line);
    }
    pendingDeletes.length = 0;
  };

  for (const chunk of chunks) {
    for (const line of chunk.lines) {
      if (line.type === 'header' || line.type === 'hunk') continue;

      if (isDeletionLine(line)) {
        pendingDeletes.push(line.content);
        continue;
      }

      if (line.type === 'add') {
        if (pendingDeletes.length > 0) {
          flushDeletes();
          inChange = true;
        }
        appendPreviewBlockLine(blocks, inChange ? 'change' : 'add', line.content);
        continue;
      }

      if (isContextLine(line)) {
        flushDeletes();
        inChange = false;
        appendPreviewBlockLine(blocks, 'context', line.content);
      }
    }

    flushDeletes();
    inChange = false;
  }

  return blocks;
}

export function stripFrontmatterBlocks(
  blocks: MarkdownPreviewBlock[],
  baseLines: string[],
  targetLines: string[],
): MarkdownPreviewBlock[] {
  if (baseLines.length === 0 && targetLines.length === 0) {
    return blocks;
  }

  let baseIndex = 0;
  let targetIndex = 0;
  let stripping = true;
  const result: MarkdownPreviewBlock[] = [];

  for (const block of blocks) {
    if (!stripping) {
      result.push(block);
      continue;
    }

    const keptLines: string[] = [];
    for (const rawLine of block.lines) {
      if (!stripping) {
        keptLines.push(rawLine);
        continue;
      }

      const line = rawLine.replace(/\r$/, '');
      const baseMatches = baseIndex < baseLines.length && baseLines[baseIndex] === line;
      const targetMatches = targetIndex < targetLines.length && targetLines[targetIndex] === line;

      let matched = false;
      if (block.type === 'delete') {
        matched = baseMatches;
        if (matched) baseIndex += 1;
      } else if (block.type === 'add' || block.type === 'change') {
        matched = targetMatches;
        if (matched) targetIndex += 1;
      } else {
        matched =
          (baseLines.length === 0 || baseMatches) && (targetLines.length === 0 || targetMatches);
        if (matched) {
          if (baseMatches) baseIndex += 1;
          if (targetMatches) targetIndex += 1;
        }
      }

      if (!matched) {
        stripping = false;
        keptLines.push(rawLine);
        continue;
      }

      if (baseIndex >= baseLines.length && targetIndex >= targetLines.length) {
        stripping = false;
      }
    }

    if (keptLines.length > 0) {
      result.push({ type: block.type, lines: keptLines });
    }
  }

  return result;
}

export function buildMarkdownRenderBlocks(blocks: MarkdownPreviewBlock[]): MarkdownRenderBlock[] {
  const renderBlocks: MarkdownRenderBlock[] = [];
  let currentMarkdownBlock: {
    kind: 'markdown';
    type: MarkdownPreviewBlockType;
    lines: string[];
  } | null = null;
  let currentFence: { fence: FenceInfo; lines: MarkdownPreviewLineEntry[] } | null = null;

  const flushMarkdown = () => {
    if (!currentMarkdownBlock) return;
    renderBlocks.push(currentMarkdownBlock);
    currentMarkdownBlock = null;
  };

  const pushMarkdownLine = (type: MarkdownPreviewBlockType, line: string) => {
    if (currentMarkdownBlock?.type === type) {
      currentMarkdownBlock.lines.push(line);
      return;
    }
    flushMarkdown();
    currentMarkdownBlock = { kind: 'markdown', type, lines: [line] };
  };

  const flushFence = (closingFenceLine?: string) => {
    if (!currentFence) return;
    const onlyContext = currentFence.lines.every((line) => line.type === 'context');
    if (closingFenceLine && onlyContext) {
      pushMarkdownLine('context', currentFence.fence.raw);
      for (const line of currentFence.lines) {
        pushMarkdownLine('context', line.content);
      }
      pushMarkdownLine('context', closingFenceLine);
    } else {
      flushMarkdown();
      renderBlocks.push({ kind: 'fenced-code', lines: currentFence.lines });
    }
    currentFence = null;
  };

  for (const block of blocks) {
    for (const line of block.lines) {
      if (currentFence) {
        if (isFenceEnd(line, currentFence.fence)) {
          flushFence(line);
          continue;
        }
        currentFence.lines.push({ type: block.type, content: line });
        continue;
      }

      const fence = parseFenceStart(line);
      if (fence) {
        flushMarkdown();
        currentFence = { fence, lines: [] };
        continue;
      }

      pushMarkdownLine(block.type, line);
    }
  }

  flushFence();
  flushMarkdown();
  return renderBlocks;
}

export function isPlainPreviewBlock(lines: string[]): boolean {
  return lines.every((line) => {
    const trimmed = line.trim();
    if (trimmed === '') return true;
    if (/^\s*\[[^\]]+\]:\s+\S+/.test(line)) return true;
    if (/^\s*\[\^[^\]]+\]:\s+/.test(line)) return true;
    return (
      trimmed.length >= '<!---->'.length && trimmed.startsWith('<!--') && trimmed.endsWith('-->')
    );
  });
}
