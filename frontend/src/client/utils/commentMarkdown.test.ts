import { describe, expect, it } from 'vitest';

import {
  parseInlineMarkdown,
  parseMarkdownBlocks,
  splitCommentBodySegments,
} from './commentMarkdown';

describe('commentMarkdown utilities', () => {
  it('splits suggestion blocks from surrounding markdown prose', () => {
    const body = 'Please update:\n```suggestion\nconst x = 2;\n```\nThanks!';
    expect(splitCommentBodySegments(body)).toEqual([
      { type: 'markdown', content: 'Please update:\n' },
      { type: 'suggestion', code: 'const x = 2;' },
      { type: 'markdown', content: '\nThanks!' },
    ]);
  });

  it('parses headings, code fences, lists, and blockquotes', () => {
    const blocks = parseMarkdownBlocks(
      '## Notes\n\n> quoted line\n\n- item one\n- item two\n\n```ts\nconst a = 1;\n```',
    );
    expect(blocks.map((b) => b.type)).toEqual(['heading', 'blockquote', 'list', 'code']);
  });

  it('neutralizes unsafe link protocols while preserving safe links', () => {
    expect(parseInlineMarkdown('[bad](javascript:alert(1)) [ok](https://example.com)')).toEqual([
      { type: 'text', text: 'bad' },
      { type: 'text', text: ' ' },
      { type: 'link', text: 'ok', href: 'https://example.com' },
    ]);
  });

  it('handles unclosed links linearly and preserves uppercase HTTPS links', () => {
    const unclosed = `[label](${'a'.repeat(5000)}`;
    expect(parseInlineMarkdown(unclosed)).toEqual([{ type: 'text', text: unclosed }]);

    expect(parseInlineMarkdown('[spec](HTTPS://example.com/Docs)')).toEqual([
      { type: 'link', text: 'spec', href: 'HTTPS://example.com/Docs' },
    ]);
  });
});
