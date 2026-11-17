<script lang="ts">
  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  let { isOpen, onClose }: Props = $props();

  function handleKeydown(event: KeyboardEvent) {
    if (isOpen && event.key === 'Escape') {
      event.preventDefault();
      onClose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isOpen}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center"
    role="dialog"
    aria-modal="true"
    aria-labelledby="help-modal-title"
  >
    <button
      type="button"
      tabindex="-1"
      aria-label="Close help modal backdrop"
      class="absolute inset-0 bg-black/50 w-full h-full border-0 cursor-default"
      onclick={onClose}
    ></button>

    <div class="relative bg-github-bg-primary border border-github-border rounded-lg shadow-lg max-w-4xl w-full mx-4 max-h-[80vh] overflow-y-auto">
      <div class="sticky top-0 bg-github-bg-primary border-b border-github-border px-6 py-4 flex items-center justify-between">
        <h2 id="help-modal-title" class="text-lg font-semibold text-github-text-primary">
          Keyboard Shortcuts
        </h2>
        <button
          type="button"
          onclick={onClose}
          class="text-github-text-secondary hover:text-github-text-primary transition-colors px-2 py-1 text-sm cursor-pointer"
          aria-label="Close help modal"
        >
          ✕
        </button>
      </div>

      <div class="px-6 py-4 grid grid-cols-1 lg:grid-cols-2 gap-6">
        <section>
          <h3 class="text-sm font-semibold text-github-text-primary mb-2">Line Navigation</h3>
          <div class="space-y-1.5 text-sm">
            <div class="flex justify-between">
              <div class="flex gap-2">
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">j</kbd>
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">↓</kbd>
              </div>
              <span class="text-github-text-secondary">Next line</span>
            </div>
            <div class="flex justify-between">
              <div class="flex gap-2">
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">k</kbd>
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">↑</kbd>
              </div>
              <span class="text-github-text-secondary">Previous line</span>
            </div>
          </div>
        </section>

        <section>
          <h3 class="text-sm font-semibold text-github-text-primary mb-2">File Navigation</h3>
          <div class="space-y-1.5 text-sm">
            <div class="flex justify-between">
              <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">]</kbd>
              <span class="text-github-text-secondary">Next file</span>
            </div>
            <div class="flex justify-between">
              <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">[</kbd>
              <span class="text-github-text-secondary">Previous file</span>
            </div>
            <div class="flex justify-between">
              <div class="flex items-center gap-1">
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">Shift</kbd>
                <span class="text-github-text-muted">+</span>
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">[</kbd>
              </div>
              <span class="text-github-text-secondary">Jump to first file</span>
            </div>
            <div class="flex justify-between">
              <div class="flex items-center gap-1">
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">Shift</kbd>
                <span class="text-github-text-muted">+</span>
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">]</kbd>
              </div>
              <span class="text-github-text-secondary">Jump to last file</span>
            </div>
          </div>
        </section>

        <section>
          <h3 class="text-sm font-semibold text-github-text-primary mb-2">Chunk Navigation</h3>
          <div class="space-y-1.5 text-sm">
            <div class="flex justify-between">
              <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">n</kbd>
              <span class="text-github-text-secondary">Next change chunk</span>
            </div>
            <div class="flex justify-between">
              <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">p</kbd>
              <span class="text-github-text-secondary">Previous change chunk</span>
            </div>
          </div>
        </section>

        <section>
          <h3 class="text-sm font-semibold text-github-text-primary mb-2">Comment Navigation</h3>
          <div class="space-y-1.5 text-sm">
            <div class="flex justify-between">
              <div class="flex items-center gap-1">
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">Shift</kbd>
                <span class="text-github-text-muted">+</span>
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">N</kbd>
              </div>
              <span class="text-github-text-secondary">Next unresolved comment</span>
            </div>
            <div class="flex justify-between">
              <div class="flex items-center gap-1">
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">Shift</kbd>
                <span class="text-github-text-muted">+</span>
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">P</kbd>
              </div>
              <span class="text-github-text-secondary">Previous unresolved comment</span>
            </div>
          </div>
        </section>

        <section>
          <h3 class="text-sm font-semibold text-github-text-primary mb-2">View & Side Modes</h3>
          <div class="space-y-1.5 text-sm">
            <div class="flex justify-between">
              <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">u</kbd>
              <span class="text-github-text-secondary">Switch to Unified diff mode</span>
            </div>
            <div class="flex justify-between">
              <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">s</kbd>
              <span class="text-github-text-secondary">Switch to Split (side-by-side) diff mode</span>
            </div>
            <div class="flex justify-between">
              <div class="flex gap-2">
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">h</kbd>
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">←</kbd>
              </div>
              <span class="text-github-text-secondary">Focus left side (Split mode)</span>
            </div>
            <div class="flex justify-between">
              <div class="flex gap-2">
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">l</kbd>
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">→</kbd>
              </div>
              <span class="text-github-text-secondary">Focus right side (Split mode)</span>
            </div>
          </div>
        </section>

        <section>
          <h3 class="text-sm font-semibold text-github-text-primary mb-2">Actions</h3>
          <div class="space-y-1.5 text-sm">
            <div class="flex justify-between">
              <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">v</kbd>
              <span class="text-github-text-secondary">Toggle viewed state of current file</span>
            </div>
            <div class="flex justify-between gap-4">
              <div class="flex items-center gap-1">
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">Shift</kbd>
                <span class="text-github-text-muted">+</span>
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">V</kbd>
              </div>
              <span class="text-github-text-secondary text-right">Mark file viewed and go to next unviewed</span>
            </div>
            <div class="flex justify-between">
              <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">c</kbd>
              <span class="text-github-text-secondary">Add comment at focused line</span>
            </div>
            <div class="flex justify-between">
              <div class="flex items-center gap-1">
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">Shift</kbd>
                <span class="text-github-text-muted">+</span>
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">L</kbd>
              </div>
              <span class="text-github-text-secondary">Toggle comments drawer</span>
            </div>
            <div class="flex justify-between">
              <div class="flex items-center gap-1">
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">Shift</kbd>
                <span class="text-github-text-muted">+</span>
                <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">R</kbd>
              </div>
              <span class="text-github-text-secondary">Reload diff</span>
            </div>
            <div class="flex justify-between">
              <kbd class="px-2 py-0.5 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary font-mono">?</kbd>
              <span class="text-github-text-secondary">Show/hide this help</span>
            </div>
          </div>
        </section>

        <div class="pt-4 border-t border-github-border lg:col-span-2">
          <p class="text-xs text-github-text-secondary">
            Shortcuts are disabled when typing in input fields or while a modal dialog is open.
          </p>
        </div>
      </div>
    </div>
  </div>
{/if}
