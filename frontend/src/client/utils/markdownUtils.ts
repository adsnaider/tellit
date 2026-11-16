import { getFileExtension } from '../../utils/fileUtils';

const MARKDOWN_EXTENSIONS = ['md', 'markdown'];

export function isMarkdownFile(filename: string): boolean {
  if (!filename) return false;

  const extension = getFileExtension(filename);
  return extension ? MARKDOWN_EXTENSIONS.includes(extension) : false;
}

export const isSafeUrl = (url: string) => /^(https?:|mailto:|#|\.{0,2}\/|\/)/i.test(url.trim());
