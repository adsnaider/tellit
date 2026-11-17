import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import HelpModal from './HelpModal.svelte';

describe('HelpModal.svelte', () => {
  it('renders shortcut sections and closes on button click or Escape', async () => {
    const onClose = vi.fn();
    render(HelpModal, { isOpen: true, onClose });

    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('Keyboard Shortcuts')).toBeInTheDocument();
    expect(screen.getByText('Next line')).toBeInTheDocument();
    expect(screen.getByText('Next unresolved comment')).toBeInTheDocument();
    expect(screen.getByText('Switch to Unified diff mode')).toBeInTheDocument();
    expect(screen.getByText('Switch to Split (side-by-side) diff mode')).toBeInTheDocument();

    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);

    await fireEvent.click(screen.getByRole('button', { name: 'Close help modal' }));
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
