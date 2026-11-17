import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import { computeFrontmatterDiff } from '../utils/frontmatterDiff';

import FrontmatterTable from './FrontmatterTable.svelte';

describe('FrontmatterTable.svelte', () => {
  it('renders diff mode with added, removed, and modified cells highlighted', () => {
    const entries = computeFrontmatterDiff(
      { title: 'Old', deprecated: true, keep: 'same' },
      { title: 'New', keep: 'same', tag: 'release' },
    );

    const { container } = render(FrontmatterTable, {
      mode: 'diff',
      entries,
      label: 'Frontmatter',
    });

    expect(screen.getByText('Frontmatter')).toBeInTheDocument();
    expect(screen.getByText('Old')).toBeInTheDocument();
    expect(screen.getByText('New')).toBeInTheDocument();
    expect(screen.getByText('release')).toBeInTheDocument();
    expect(container.querySelector('td.bg-diff-deletion-bg')).not.toBeNull();
    expect(container.querySelector('td.bg-diff-addition-bg')).not.toBeNull();
  });

  it('renders snapshot mode including nested depth-0 tables and scalar edge cases', () => {
    render(FrontmatterTable, {
      mode: 'snapshot',
      label: 'Metadata',
      data: {
        empty: '',
        missing: null,
        tags: ['a', 'b'],
        meta: { author: 'Alice' },
      },
    });

    expect(screen.getByText('Metadata')).toBeInTheDocument();
    expect(screen.getByText('null')).toBeInTheDocument();
    expect(screen.getByText('""')).toBeInTheDocument();
    expect(screen.getByText('["a","b"]')).toBeInTheDocument();
    expect(screen.getByText('author')).toBeInTheDocument();
    expect(screen.getByText('Alice')).toBeInTheDocument();
  });
});
