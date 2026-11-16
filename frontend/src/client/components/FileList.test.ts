import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import type { DiffFile } from '../../types/diff';

import FileList from './FileList.svelte';

const sampleFiles: DiffFile[] = [
  {
    path: 'Commit Message',
    status: 'added',
    additions: 1,
    deletions: 0,
    chunks: [],
  },
  {
    path: 'src/client/App.svelte',
    status: 'modified',
    additions: 12,
    deletions: 4,
    chunks: [],
  },
  {
    path: 'src/client/gen/schema.ts',
    status: 'added',
    additions: 50,
    deletions: 0,
    isGenerated: true,
    chunks: [],
  },
];

describe('FileList.svelte', () => {
  it('renders collapsed single-child folders, badges, viewed checkboxes, and folder toggles', async () => {
    const onScrollToFile = vi.fn();
    const onToggleViewed = vi.fn();
    const onToggleFolderViewed = vi.fn();

    render(FileList, {
      files: sampleFiles,
      viewedFiles: new Set(['src/client/gen/schema.ts']),
      changedSinceViewedFiles: new Set(['src/client/App.svelte']),
      onScrollToFile,
      onToggleViewed,
      onToggleFolderViewed,
    });

    expect(screen.getByText('Files (1/3)')).toBeInTheDocument();
    expect(screen.getByText('src/client')).toBeInTheDocument();
    expect(
      screen.getByText('gen', { selector: 'span[title="Generated file"]' }),
    ).toBeInTheDocument();
    expect(screen.getByText('updated')).toBeInTheDocument();

    const appCheckbox = screen.getByRole('checkbox', {
      name: 'Mark src/client/App.svelte as viewed',
    });
    await fireEvent.click(appCheckbox);
    expect(onToggleViewed).toHaveBeenCalledWith(sampleFiles[1]);

    const folderCheckbox = screen.getByRole('checkbox', {
      name: 'Mark folder src/client as viewed',
    });
    await fireEvent.click(folderCheckbox);
    expect(onToggleFolderViewed).toHaveBeenCalledWith([sampleFiles[1], sampleFiles[2]], true);

    await fireEvent.click(screen.getByRole('button', { name: /App\.svelte/ }));
    expect(onScrollToFile).toHaveBeenCalledWith('src/client/App.svelte');
  });

  it('filters files by path substring', async () => {
    render(FileList, {
      files: sampleFiles,
      viewedFiles: new Set<string>(),
      changedSinceViewedFiles: new Set<string>(),
      onScrollToFile: vi.fn(),
      onToggleViewed: vi.fn(),
      onToggleFolderViewed: vi.fn(),
    });

    const filterInput = screen.getByLabelText('Filter changed files');
    await fireEvent.input(filterInput, { target: { value: 'schema' } });

    expect(screen.getByText('schema.ts')).toBeInTheDocument();
    expect(screen.queryByText('App.svelte')).not.toBeInTheDocument();
  });

  it('scopes Collapse/Expand dirs to visible directories and handles file+dir name collisions', async () => {
    const twoDirFiles: DiffFile[] = [
      { path: 'alpha/one.ts', status: 'modified', additions: 1, deletions: 0, chunks: [] },
      { path: 'beta/two.ts', status: 'modified', additions: 1, deletions: 0, chunks: [] },
    ];

    const { rerender } = render(FileList, {
      files: twoDirFiles,
      viewedFiles: new Set<string>(),
      changedSinceViewedFiles: new Set<string>(),
      onScrollToFile: vi.fn(),
      onToggleViewed: vi.fn(),
      onToggleFolderViewed: vi.fn(),
    });

    // Collapse only alpha, then filter to beta: button should show "Collapse dirs"
    await fireEvent.click(screen.getByRole('button', { name: /alpha$/ }));
    expect(screen.queryByText('one.ts')).not.toBeInTheDocument();

    const filterInput = screen.getByLabelText('Filter changed files');
    await fireEvent.input(filterInput, { target: { value: 'beta' } });
    expect(screen.getByRole('button', { name: 'Collapse dirs' })).toBeInTheDocument();
    expect(screen.getByText('two.ts')).toBeInTheDocument();

    // Update files with a path collision between file "foo" and directory "foo/bar.ts"
    await fireEvent.input(filterInput, { target: { value: '' } });
    await rerender({
      files: [
        { path: 'foo', status: 'deleted', additions: 0, deletions: 2, chunks: [] },
        { path: 'foo/bar.ts', status: 'added', additions: 3, deletions: 0, chunks: [] },
      ],
      viewedFiles: new Set<string>(),
      changedSinceViewedFiles: new Set<string>(),
      onScrollToFile: vi.fn(),
      onToggleViewed: vi.fn(),
      onToggleFolderViewed: vi.fn(),
    });

    expect(screen.getByRole('button', { name: 'Collapse dirs' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /▾\s+foo/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /D\s+foo/ })).toBeInTheDocument();
    expect(screen.getByText('bar.ts')).toBeInTheDocument();
  });
});
