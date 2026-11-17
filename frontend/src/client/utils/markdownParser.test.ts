import { describe, expect, it } from 'vitest';

import type { MergedChunk } from '../../types/diff';

import {
  buildMarkdownPreviewBlocks,
  buildMarkdownRenderBlocks,
  parseGfmBlocks,
  parseGfmInline,
  stripFrontmatterBlocks,
} from './markdownParser';

describe('markdownParser', () => {
  it('parses GFM headings, tables, task lists, code blocks, strikethrough, and safe links', () => {
    const md = [
      '# Release Notes',
      '',
      '| Item | Status | Notes |',
      '| :--- | :---: | ---: |',
      '| Diff | ~~old~~ **new** | `ok` |',
      '',
      '- [x] Ready [docs](HTTPS://example.com/guide_(v2))',
      '- [ ] Blocked [xss](javascript:alert(1)) ![bad](data:text/html,xss)',
      '',
      '```ts',
      'const value = 42;',
      '```',
    ].join('\n');

    const blocks = parseGfmBlocks(md);
    expect(blocks.map((b) => b.type)).toEqual(['heading', 'table', 'list', 'code']);

    const table = blocks[1];
    expect(table?.type).toBe('table');
    if (table?.type === 'table') {
      expect(table.alignments).toEqual(['left', 'center', 'right']);
      expect(table.rows[0]?.[1]).toContainEqual({ type: 'strike', text: 'old' });
      expect(table.rows[0]?.[1]).toContainEqual({ type: 'bold', text: 'new' });
    }

    const list = blocks[2];
    expect(list?.type).toBe('list');
    if (list?.type === 'list') {
      expect(list.items[0]?.checked).toBe(true);
      expect(list.items[0]?.inlines).toContainEqual({
        type: 'link',
        text: 'docs',
        href: 'HTTPS://example.com/guide_(v2)',
      });
      expect(list.items[1]?.checked).toBe(false);
      // Unsafe links downgrade to plain text and unsafe images are stripped
      expect(list.items[1]?.inlines.some((p) => p.type === 'image')).toBe(false);
      expect(list.items[1]?.inlines).toContainEqual({ type: 'text', text: 'xss' });
    }
  });

  it('parses unclosed markdown link labels linearly without backtracking', () => {
    const malformed = `[broken](${'x'.repeat(5000)}`;
    expect(parseGfmInline(malformed)).toEqual([{ type: 'text', text: malformed }]);
  });

  it('builds diff preview blocks, strips leading frontmatter, and splits fenced diff blocks', () => {
    const chunks: MergedChunk[] = [
      {
        header: '@@ -1,5 +1,6 @@',
        oldStart: 1,
        oldLines: 5,
        newStart: 1,
        newLines: 6,
        originalIndices: [0],
        hiddenLinesBefore: 0,
        hiddenLinesAfter: 0,
        lines: [
          { type: 'normal', content: '---', oldLineNumber: 1, newLineNumber: 1 },
          { type: 'delete', content: 'title: Old', oldLineNumber: 2 },
          { type: 'add', content: 'title: New', newLineNumber: 2 },
          { type: 'normal', content: '---', oldLineNumber: 3, newLineNumber: 3 },
          { type: 'add', content: '# Updated Heading', newLineNumber: 4 },
          { type: 'normal', content: '```ts', oldLineNumber: 4, newLineNumber: 5 },
          { type: 'add', content: 'export const ok = true;', newLineNumber: 6 },
          { type: 'normal', content: '```', oldLineNumber: 5, newLineNumber: 7 },
        ],
      },
    ];

    const previewBlocks = buildMarkdownPreviewBlocks(chunks);
    const stripped = stripFrontmatterBlocks(
      previewBlocks,
      ['---', 'title: Old', '---'],
      ['---', 'title: New', '---'],
    );
    const rendered = buildMarkdownRenderBlocks(stripped);

    expect(rendered).toEqual([
      { kind: 'markdown', type: 'add', lines: ['# Updated Heading'] },
      { kind: 'fenced-code', lines: [{ type: 'add', content: 'export const ok = true;' }] },
    ]);
  });
});
