import { describe, expect, it } from 'vitest';

import { loadPrismLanguage } from './languageLoader';
import { isPrismLanguageLoaded, tokenizeCode, tokenizeLine } from './prism';
import { getTokenStyleString } from './syntaxThemes';

describe('prism and syntaxThemes utilities', () => {
  it('splits multiline template tokens cleanly and resets styling inside interpolation', () => {
    const lines = tokenizeCode('const msg = `hello ${name}\nworld`;', 'javascript');
    expect(lines).toHaveLength(2);
    expect(lines[0]?.some((t) => t.types.includes('keyword') && t.content === 'const')).toBe(true);
    const interpolated = lines[0]?.find((t) => t.content === 'name');
    expect(interpolated?.types).toContain('interpolation');
    expect(interpolated?.types).not.toContain('template-string');
    expect(lines[1]?.some((t) => t.types.includes('string') && t.content === 'world')).toBe(true);
  });

  it('dynamically loads typescript, tsx, and svelte grammars', async () => {
    await loadPrismLanguage('tsx');
    await loadPrismLanguage('svelte');

    expect(isPrismLanguageLoaded('typescript')).toBe(true);
    expect(isPrismLanguageLoaded('tsx')).toBe(true);
    expect(isPrismLanguageLoaded('svelte')).toBe(true);

    const tsTokens = tokenizeLine('interface User { id: number }', 'typescript');
    expect(tsTokens.some((t) => t.types.includes('keyword') && t.content === 'interface')).toBe(
      true,
    );
  });

  it('maps token types to syntax theme styles without background colors', () => {
    const darkKeyword = getTokenStyleString(['keyword'], 'vsDark');
    const lightKeyword = getTokenStyleString(['keyword'], 'github');
    expect(darkKeyword).toContain('color:#569cd6');
    expect(lightKeyword).toContain('color:#d73a49');
    expect(darkKeyword).not.toContain('background');
  });
});
