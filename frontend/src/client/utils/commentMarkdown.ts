import { parseSuggestionBlocks } from '../../utils/suggestionUtils';
import { isSafeUrl } from './markdownUtils';

export type InlinePart =
  | { type: 'text'; text: string }
  | { type: 'bold'; text: string }
  | { type: 'code'; text: string }
  | { type: 'link'; text: string; href: string };

export type MarkdownBlock =
  | { type: 'heading'; level: number; inlines: InlinePart[] }
  | { type: 'code'; language: string; code: string }
  | { type: 'blockquote'; lines: InlinePart[][] }
  | { type: 'list'; ordered: boolean; items: InlinePart[][] }
  | { type: 'hr' }
  | { type: 'paragraph'; lines: InlinePart[][] };

export type CommentBodySegment =
  | { type: 'markdown'; content: string }
  | { type: 'suggestion'; code: string };

const INLINE_TOKEN_RE =
  /(`[^`\n]+`|\*\*[^*\n]+?\*\*|\[[^\]\n]+\]\(((?:[^\s()]|\([^\s()]*\))+)\)|https?:\/\/[^\s<>()]+[^\s<>().,;:!?'"])/g;

export function parseInlineMarkdown(text: string): InlinePart[] {
  if (!text) return [];
  const parts: InlinePart[] = [];
  let lastIndex = 0;

  INLINE_TOKEN_RE.lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = INLINE_TOKEN_RE.exec(text)) !== null) {
    if (match.index > lastIndex) {
      parts.push({ type: 'text', text: text.slice(lastIndex, match.index) });
    }

    const raw = match[0];
    if (raw.startsWith('`') && raw.endsWith('`') && raw.length >= 2) {
      parts.push({ type: 'code', text: raw.slice(1, -1) });
    } else if (raw.startsWith('**') && raw.endsWith('**') && raw.length >= 4) {
      parts.push({ type: 'bold', text: raw.slice(2, -2) });
    } else if (raw.startsWith('[')) {
      const closeBracket = raw.indexOf('](');
      const label = raw.slice(1, closeBracket);
      const href = raw.slice(closeBracket + 2, -1);
      if (isSafeUrl(href)) {
        parts.push({ type: 'link', text: label, href });
      } else {
        parts.push({ type: 'text', text: label });
      }
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

export function parseMarkdownBlocks(markdown: string): MarkdownBlock[] {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n');
  const blocks: MarkdownBlock[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i] ?? '';
    const trimmed = line.trim();

    if (!trimmed) {
      i++;
      continue;
    }

    const fenceMatch = line.match(/^```([^\s`]*)\s*$/);
    if (fenceMatch) {
      const language = (fenceMatch[1] ?? '').trim() || 'text';
      const codeLines: string[] = [];
      i++;
      while (i < lines.length && !(lines[i] ?? '').match(/^```\s*$/)) {
        codeLines.push(lines[i] ?? '');
        i++;
      }
      if (i < lines.length) {
        i++;
      }
      blocks.push({ type: 'code', language, code: codeLines.join('\n') });
      continue;
    }

    const headingMatch = line.match(/^(#{1,6})\s+(.+)$/);
    if (headingMatch) {
      blocks.push({
        type: 'heading',
        level: headingMatch[1]?.length ?? 1,
        inlines: parseInlineMarkdown(headingMatch[2] ?? ''),
      });
      i++;
      continue;
    }

    if (/^[-*_]{3,}\s*$/.test(trimmed)) {
      blocks.push({ type: 'hr' });
      i++;
      continue;
    }

    if (line.startsWith('>')) {
      const quoteLines: InlinePart[][] = [];
      while (i < lines.length && (lines[i] ?? '').startsWith('>')) {
        quoteLines.push(parseInlineMarkdown((lines[i] ?? '').replace(/^>\s?/, '')));
        i++;
      }
      blocks.push({ type: 'blockquote', lines: quoteLines });
      continue;
    }

    const unorderedMatch = line.match(/^\s*[-*]\s+(.+)$/);
    const orderedMatch = line.match(/^\s*\d+\.\s+(.+)$/);
    if (unorderedMatch || orderedMatch) {
      const ordered = Boolean(orderedMatch);
      const items: InlinePart[][] = [];
      const itemRe = ordered ? /^\s*\d+\.\s+(.+)$/ : /^\s*[-*]\s+(.+)$/;
      while (i < lines.length) {
        const m = (lines[i] ?? '').match(itemRe);
        if (!m) break;
        items.push(parseInlineMarkdown(m[1] ?? ''));
        i++;
      }
      blocks.push({ type: 'list', ordered, items });
      continue;
    }

    const paraLines: InlinePart[][] = [];
    while (i < lines.length) {
      const current = lines[i] ?? '';
      if (
        !current.trim() ||
        /^```/.test(current) ||
        /^#{1,6}\s+/.test(current) ||
        /^[-*_]{3,}\s*$/.test(current.trim()) ||
        current.startsWith('>') ||
        /^\s*[-*]\s+/.test(current) ||
        /^\s*\d+\.\s+/.test(current)
      ) {
        break;
      }
      paraLines.push(parseInlineMarkdown(current));
      i++;
    }
    if (paraLines.length > 0) {
      blocks.push({ type: 'paragraph', lines: paraLines });
    }
  }

  return blocks;
}

export function splitCommentBodySegments(body: string): CommentBodySegment[] {
  const suggestions = parseSuggestionBlocks(body);
  if (suggestions.length === 0) {
    return body ? [{ type: 'markdown', content: body }] : [];
  }

  const segments: CommentBodySegment[] = [];
  let lastIndex = 0;

  for (const suggestion of suggestions) {
    if (suggestion.startIndex > lastIndex) {
      const markdown = body.slice(lastIndex, suggestion.startIndex);
      if (markdown.trim()) {
        segments.push({ type: 'markdown', content: markdown });
      }
    }
    segments.push({ type: 'suggestion', code: suggestion.suggestedCode });
    lastIndex = suggestion.endIndex;
  }

  if (lastIndex < body.length) {
    const trailing = body.slice(lastIndex);
    if (trailing.trim()) {
      segments.push({ type: 'markdown', content: trailing });
    }
  }

  return segments;
}
