import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { DEFAULT_APPEARANCE_SETTINGS, type AppearanceSettings } from '../utils/appearanceSettings';

import SettingsModal from './SettingsModal.svelte';

const baseSettings: AppearanceSettings = {
  ...DEFAULT_APPEARANCE_SETTINGS,
  theme: 'dark',
  syntaxTheme: 'vsDark',
  editor: { id: 'cursor', command: 'cursor', argsTemplate: '-g %file:%line' },
  autoViewedPatterns: [],
};

describe('SettingsModal.svelte', () => {
  it('switches between Appearance and System tabs and edits typography, theme, and color vision', async () => {
    const onSettingsChange = vi.fn();

    render(SettingsModal, {
      isOpen: true,
      settings: baseSettings,
      onClose: vi.fn(),
      onSettingsChange,
    });

    expect(screen.getByText('Font Size')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Appearance' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );

    // Theme switch from Dark -> Light auto-selects a valid light syntax theme
    await fireEvent.click(screen.getByRole('button', { name: 'Light' }));
    expect(onSettingsChange).toHaveBeenLastCalledWith(
      expect.objectContaining({
        theme: 'light',
        syntaxTheme: 'github',
      }),
    );

    // Deuteranopia hover shows tooltip cleanly
    const deutBtn = screen.getByRole('button', { name: 'Deuteranopia' });
    await fireEvent.mouseEnter(deutBtn);
    expect(screen.getByRole('tooltip')).toHaveTextContent(
      'Deuteranopia mode uses blue/orange instead of green/red for diffs.',
    );
    await fireEvent.mouseLeave(deutBtn);
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();

    // System tab shows Auto-Mark Viewed Patterns and Open In Editor
    await fireEvent.click(screen.getByRole('button', { name: 'System' }));
    expect(screen.getByLabelText('Auto-Mark Viewed Patterns')).toBeInTheDocument();
    expect(screen.getByLabelText('Open In Editor')).toBeInTheDocument();

    const patternsInput = screen.getByLabelText('Auto-Mark Viewed Patterns');
    await fireEvent.input(patternsInput, { target: { value: '*.lock\nsrc/gen/**' } });
    expect(onSettingsChange).toHaveBeenLastCalledWith(
      expect.objectContaining({
        autoViewedPatterns: ['*.lock', 'src/gen/**'],
      }),
    );
  });

  it('validates custom editor command/arguments and closes on Escape', async () => {
    const onClose = vi.fn();
    const onSettingsChange = vi.fn();

    const { rerender } = render(SettingsModal, {
      isOpen: true,
      settings: {
        ...baseSettings,
        editor: { id: 'custom', command: '', argsTemplate: '' },
      },
      onClose,
      onSettingsChange,
    });

    await fireEvent.click(screen.getByRole('button', { name: 'System' }));
    expect(screen.getByLabelText('Command')).toHaveAttribute('aria-invalid', 'true');
    expect(screen.getByLabelText('Arguments')).toHaveAttribute('aria-invalid', 'true');
    expect(screen.getByText('Command is required.')).toBeInTheDocument();
    expect(screen.getByText('Arguments are required.')).toBeInTheDocument();

    await rerender({
      settings: {
        ...baseSettings,
        editor: { id: 'none', command: '', argsTemplate: '' },
      },
    });
    expect(screen.queryByLabelText('Command')).not.toBeInTheDocument();

    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
