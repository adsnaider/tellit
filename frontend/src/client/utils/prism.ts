import Prism, { type Token, type TokenStream } from 'prismjs';

// Expose Prism globally so dynamically imported Prism component scripts register on Prism.languages.
const globalRecord = globalThis as Record<string, unknown>;
globalRecord.Prism = Prism;
if (typeof window !== 'undefined') {
  (window as unknown as Record<string, unknown>).Prism = Prism;
}

export interface HighlightToken {
  types: string[];
  content: string;
}

const PLAIN_LANGUAGES = new Set(['', 'text', 'plain', 'plaintext', 'txt']);

function normalizePlainLines(code: string): HighlightToken[][] {
  return code.split('\n').map((line) => (line ? [{ types: [], content: line }] : []));
}

function appendTextWithNewlines(lines: HighlightToken[][], text: string, types: string[]): void {
  const parts = text.split('\n');
  for (let i = 0; i < parts.length; i++) {
    if (i > 0) {
      lines.push([]);
    }
    const part = parts[i];
    if (part) {
      const currentLine = lines[lines.length - 1];
      currentLine?.push({ types, content: part });
    }
  }
}

function collectTokenStream(
  lines: HighlightToken[][],
  stream: TokenStream,
  parentTypes: string[],
): void {
  if (typeof stream === 'string') {
    appendTextWithNewlines(lines, stream, parentTypes);
    return;
  }

  if (Array.isArray(stream)) {
    for (const item of stream) {
      collectTokenStream(lines, item, parentTypes);
    }
    return;
  }

  const token: Token = stream;
  const aliases = Array.isArray(token.alias) ? token.alias : token.alias ? [token.alias] : [];
  // Reset outer string/template styling inside template interpolations so nested expressions keep normal colors.
  const baseTypes = token.type === 'interpolation' ? [] : parentTypes;
  const nextTypes = [...baseTypes, token.type, ...aliases];
  collectTokenStream(lines, token.content, nextTypes);
}

export function isPlainLanguage(language: string): boolean {
  return PLAIN_LANGUAGES.has(language.toLowerCase());
}

export function isPrismLanguageLoaded(language: string): boolean {
  if (isPlainLanguage(language)) return true;
  return Boolean(Prism.languages[language]);
}

export function tokenizeCode(code: string, language: string): HighlightToken[][] {
  if (!code) {
    return [[]];
  }
  if (isPlainLanguage(language)) {
    return normalizePlainLines(code);
  }
  const grammar = Prism.languages[language];
  if (!grammar) {
    return normalizePlainLines(code);
  }

  try {
    const raw = Prism.tokenize(code, grammar);
    const lines: HighlightToken[][] = [[]];
    collectTokenStream(lines, raw, []);
    return lines;
  } catch {
    return normalizePlainLines(code);
  }
}

export function tokenizeLine(line: string, language: string): HighlightToken[] {
  return tokenizeCode(line, language)[0] ?? [];
}

export default Prism;
