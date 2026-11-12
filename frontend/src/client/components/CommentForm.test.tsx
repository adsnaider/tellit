import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { CommentForm } from './CommentForm';

describe('CommentForm', () => {
  it('marks the cancel action independently from other form buttons', () => {
    const onCancel = vi.fn();
    const { container } = render(
      <CommentForm onSubmit={vi.fn()} onCancel={onCancel} selectedCode="const value = 1;" />,
    );

    const cancelButton = container.querySelector<HTMLButtonElement>('[data-comment-cancel="true"]');
    expect(cancelButton).toBe(screen.getByRole('button', { name: 'Cancel' }));

    fireEvent.click(cancelButton!);
    expect(onCancel).toHaveBeenCalledOnce();
    expect(screen.getByRole('textbox')).toHaveValue('');
  });

  it('renders an inline error and preserves the draft when submission fails, then clears on edit and retry', async () => {
    const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
    const onSubmit = vi
      .fn<(body: string) => Promise<void>>()
      .mockRejectedValueOnce(new Error('jj error: The working copy is stale'))
      .mockResolvedValueOnce(undefined);

    render(<CommentForm onSubmit={onSubmit} onCancel={vi.fn()} />);

    const textarea = screen.getByRole('textbox');
    fireEvent.change(textarea, { target: { value: 'Keep my draft intact' } });
    fireEvent.click(screen.getByRole('button', { name: 'Submit' }));

    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('jj error: The working copy is stale');
    expect(textarea).toHaveValue('Keep my draft intact');
    expect(textarea).not.toBeDisabled();

    // Editing the draft clears the inline error banner.
    fireEvent.change(textarea, { target: { value: 'Keep my draft intact (revised)' } });
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();

    // Retrying clears the draft when submission succeeds.
    fireEvent.click(screen.getByRole('button', { name: 'Submit' }));
    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledTimes(2);
      expect(textarea).toHaveValue('');
    });
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();

    consoleErrorSpy.mockRestore();
  });
});
