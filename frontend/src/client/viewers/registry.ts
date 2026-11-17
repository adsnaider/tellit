import type { Component } from 'svelte';

import type { DiffFile } from '../../types/diff';
import ImageDiffViewer from '../components/ImageDiffViewer.svelte';
import MarkdownDiffViewer from '../components/MarkdownDiffViewer.svelte';
import TextDiffViewer from '../components/TextDiffViewer.svelte';
import { isImageFile } from '../utils/imageUtils';
import { isMarkdownFile } from '../utils/markdownUtils';

import type { DiffViewerBodyProps } from './types';

export type ViewerId = 'image' | 'markdown' | 'text';

export interface DiffViewerRegistration {
  id: ViewerId;
  match: (file: DiffFile) => boolean;
  Component: Component<DiffViewerBodyProps>;
  canExpandHiddenLines: (file: DiffFile) => boolean;
}

const viewers: DiffViewerRegistration[] = [
  {
    id: 'image',
    match: (file) => isImageFile(file.path),
    Component: ImageDiffViewer,
    canExpandHiddenLines: () => false,
  },
  {
    id: 'markdown',
    match: (file) => isMarkdownFile(file.path),
    Component: MarkdownDiffViewer,
    canExpandHiddenLines: (file) => file.status !== 'added' && file.status !== 'deleted',
  },
  {
    id: 'text',
    match: () => true,
    Component: TextDiffViewer,
    canExpandHiddenLines: (file) => file.status !== 'added' && file.status !== 'deleted',
  },
];

export function getViewerForFile(file: DiffFile): DiffViewerRegistration {
  const fallback = viewers[viewers.length - 1];
  if (!fallback) {
    throw new Error('No diff viewers registered');
  }
  return viewers.find((viewer) => viewer.match(file)) ?? fallback;
}
