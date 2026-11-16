import { type DiffFile } from '../../types/diff';
export { getLanguageFromPath } from './languageDetection';

export function formatStatusBadge(status: DiffFile['status']): {
  label: string;
  classes: string;
} {
  switch (status) {
    case 'added':
      return {
        label: 'A',
        classes: 'bg-green-500/15 text-green-400 border-green-500/30',
      };
    case 'deleted':
      return {
        label: 'D',
        classes: 'bg-red-500/15 text-red-400 border-red-500/30',
      };
    case 'renamed':
      return {
        label: 'R',
        classes: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30',
      };
    case 'modified':
      return {
        label: 'M',
        classes: 'bg-blue-500/15 text-blue-400 border-blue-500/30',
      };
  }
}

/**
 * Generate SHA-256 hash of diff content
 */
export async function generateDiffHash(diffContent: string): Promise<string> {
  try {
    const encoder = new TextEncoder();
    const data = encoder.encode(diffContent);
    const hashBuffer = await crypto.subtle.digest('SHA-256', data);
    const hashArray = Array.from(new Uint8Array(hashBuffer));
    return hashArray.map((b) => b.toString(16).padStart(2, '0')).join('');
  } catch {
    // Fallback to simple hash if crypto API is not available
    console.warn('Crypto API not available, using fallback hash');
    return simpleHash(diffContent);
  }
}

/**
 * Simple hash function as fallback
 */
function simpleHash(str: string): string {
  let hash = 0;
  for (let i = 0; i < str.length; i++) {
    const char = str.charCodeAt(i);
    hash = (hash << 5) - hash + char;
    hash = hash & hash; // Convert to 32-bit integer
  }
  return Math.abs(hash).toString(16);
}

/**
 * Generate diff content string for hashing
 */
export function getDiffContentForHashing(file: DiffFile): string {
  // Create a stable string representation of the diff
  const chunks = file.chunks
    .map((chunk) => {
      const lines = chunk.lines
        .filter((line) => line.type !== 'hunk' && line.type !== 'header')
        .map((line) => `${line.type}:${line.content}`)
        .join('\n');
      return `${chunk.header}\n${lines}`;
    })
    .join('\n\n');

  return `${file.path}\n${file.status}\n${chunks}`;
}
