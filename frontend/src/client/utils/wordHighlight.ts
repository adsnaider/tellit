import { getContext, setContext } from 'svelte';

import { isWordToken, normalizeWord } from './wordDetection';

export interface WordSegment {
  text: string;
  word?: string;
}

export function splitTextIntoWordSegments(text: string): WordSegment[] {
  const parts = text.split(/( +)/);
  if (parts.length === 1) {
    const single = parts[0];
    if (!single) return [];
    return isWordToken(single) ? [{ text: single, word: single.trim() }] : [{ text: single }];
  }

  const segments: WordSegment[] = [];
  for (const part of parts) {
    if (!part) continue;
    if (isWordToken(part)) {
      segments.push({ text: part, word: part.trim() });
    } else {
      segments.push({ text: part });
    }
  }
  return segments;
}

export interface WordHighlightContextValue {
  isWordHighlighted: (word: string) => boolean;
  onWordMouseOver: (event: MouseEvent) => void;
  onWordMouseOut: () => void;
  onWordClick: (event: MouseEvent) => void;
}

const WORD_HIGHLIGHT_CONTEXT_KEY = Symbol('word-highlight');
const WORD_HOVER_DELAY_MS = 200;

export function setWordHighlightContext(value: WordHighlightContextValue): void {
  setContext(WORD_HIGHLIGHT_CONTEXT_KEY, value);
}

export function getWordHighlightContext(): WordHighlightContextValue | undefined {
  return getContext<WordHighlightContextValue | undefined>(WORD_HIGHLIGHT_CONTEXT_KEY);
}

export function wordsMatch(a: string | null | undefined, b: string | null | undefined): boolean {
  if (!a || !b) return false;
  return normalizeWord(a) === normalizeWord(b);
}

export function createWordHighlightHandlers(options: {
  getEffectiveWord: () => string | null;
  getPinnedWord: () => string | null;
  setHoveredWord: (word: string | null) => void;
  setPinnedWord: (word: string | null) => void;
}): WordHighlightContextValue & { dispose: () => void } {
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;

  const clearHoverTimer = () => {
    if (hoverTimer) {
      clearTimeout(hoverTimer);
      hoverTimer = null;
    }
  };

  return {
    isWordHighlighted: (word: string) => wordsMatch(options.getEffectiveWord(), word),
    onWordMouseOver: (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof HTMLElement) || !target.classList.contains('word-token')) return;
      const word = target.getAttribute('data-word');
      if (!word) return;
      clearHoverTimer();
      hoverTimer = setTimeout(() => {
        hoverTimer = null;
        options.setHoveredWord(word);
      }, WORD_HOVER_DELAY_MS);
    },
    onWordMouseOut: () => {
      clearHoverTimer();
      options.setHoveredWord(null);
    },
    onWordClick: (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof HTMLElement)) return;
      const tokenEl = target.closest('.word-token');
      if (!(tokenEl instanceof HTMLElement)) return;
      const word = tokenEl.getAttribute('data-word');
      if (!word) return;
      options.setPinnedWord(wordsMatch(options.getPinnedWord(), word) ? null : word);
    },
    dispose: clearHoverTimer,
  };
}
