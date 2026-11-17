import {
  DEFAULT_EDITOR_OPTION,
  type EditorOptionId,
  resolveEditorOption,
} from '../../utils/editorOptions';

import type { ColorVisionMode, ThemePreference } from './appearanceTheme';
import { normalizeAutoViewedPatterns } from './autoViewedPatterns';

export interface EditorSettings {
  id: EditorOptionId;
  command: string;
  argsTemplate: string;
}

export interface AppearanceSettings {
  fontSize: number;
  fontFamily: string;
  theme: ThemePreference;
  syntaxTheme: string;
  editor: EditorSettings;
  colorVision: ColorVisionMode;
  autoViewedPatterns: string[];
}

export const FONT_FAMILIES = [
  {
    name: 'System Font',
    value:
      '-apple-system, BlinkMacSystemFont, "Segoe UI", "Noto Sans", Helvetica, Arial, sans-serif',
  },
  { name: 'Menlo', value: 'Menlo, Monaco, "Courier New", monospace' },
  { name: 'SF Mono', value: 'SF Mono, Consolas, "Liberation Mono", monospace' },
  { name: 'Fira Code', value: '"Fira Code", "Courier New", monospace' },
  { name: 'JetBrains Mono', value: '"JetBrains Mono", "Courier New", monospace' },
] as const;

export const DEFAULT_APPEARANCE_SETTINGS: AppearanceSettings = {
  fontSize: 14,
  fontFamily: FONT_FAMILIES[0].value,
  theme: 'auto',
  syntaxTheme: 'vsDark',
  editor: {
    id: DEFAULT_EDITOR_OPTION.id,
    command: DEFAULT_EDITOR_OPTION.command,
    argsTemplate: DEFAULT_EDITOR_OPTION.argsTemplate,
  },
  colorVision: 'normal',
  autoViewedPatterns: [],
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function normalizeEditorSettings(raw: unknown): EditorSettings {
  if (!isRecord(raw) || typeof raw.id !== 'string') {
    return { ...DEFAULT_APPEARANCE_SETTINGS.editor };
  }
  const preset = resolveEditorOption(raw.id);
  const id = (raw.id === 'custom' || raw.id === 'none' ? raw.id : preset.id) as EditorOptionId;
  return {
    id,
    command: typeof raw.command === 'string' ? raw.command : preset.command,
    argsTemplate: typeof raw.argsTemplate === 'string' ? raw.argsTemplate : preset.argsTemplate,
  };
}

export function normalizeAppearanceSettings(
  raw: unknown,
  fallback: AppearanceSettings = DEFAULT_APPEARANCE_SETTINGS,
): AppearanceSettings | null {
  if (!isRecord(raw)) return null;

  const fontSize =
    typeof raw.fontSize === 'number' && Number.isFinite(raw.fontSize)
      ? Math.min(20, Math.max(10, Math.round(raw.fontSize)))
      : fallback.fontSize;

  const fontFamily =
    typeof raw.fontFamily === 'string' && raw.fontFamily.trim()
      ? raw.fontFamily
      : fallback.fontFamily;

  const theme: ThemePreference =
    raw.theme === 'light' || raw.theme === 'dark' || raw.theme === 'auto'
      ? raw.theme
      : fallback.theme;

  const syntaxTheme =
    typeof raw.syntaxTheme === 'string' && raw.syntaxTheme.trim()
      ? raw.syntaxTheme
      : fallback.syntaxTheme;

  const colorVision: ColorVisionMode =
    raw.colorVision === 'deuteranopia' || raw.colorVision === 'normal'
      ? raw.colorVision
      : fallback.colorVision;

  const editor =
    isRecord(raw.editor) && typeof raw.editor.id === 'string'
      ? normalizeEditorSettings(raw.editor)
      : { ...fallback.editor };

  const autoViewedPatterns =
    'autoViewedPatterns' in raw && raw.autoViewedPatterns !== undefined
      ? normalizeAutoViewedPatterns(raw.autoViewedPatterns)
      : [...fallback.autoViewedPatterns];

  return {
    fontSize,
    fontFamily,
    theme,
    syntaxTheme,
    colorVision,
    editor,
    autoViewedPatterns,
  };
}
