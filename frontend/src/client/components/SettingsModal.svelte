<script lang="ts">
  import {
    CUSTOM_EDITOR_ID,
    EDITOR_OPTIONS,
    type EditorOptionId,
    NONE_EDITOR_ID,
  } from '../../utils/editorOptions';
  import {
    type AppearanceSettings,
    DEFAULT_APPEARANCE_SETTINGS,
    FONT_FAMILIES,
  } from '../utils/appearanceSettings';
  import { resolveThemePreference, type ThemePreference } from '../utils/appearanceTheme';
  import { formatAutoViewedPatterns, parseAutoViewedPatterns } from '../utils/autoViewedPatterns';
  import {
    getFallbackSyntaxTheme,
    getThemesForResolvedTheme,
    isSyntaxThemeForResolvedTheme,
  } from '../utils/themeLoader';

  interface Props {
    isOpen: boolean;
    settings: AppearanceSettings;
    onClose: () => void;
    onSettingsChange: (next: AppearanceSettings) => void;
  }

  let { isOpen, settings, onClose, onSettingsChange }: Props = $props();

  type SettingsSection = 'appearance' | 'system';

  let activeSection = $state<SettingsSection>('appearance');
  let autoViewedPatternsInput = $state('');
  let showDeuteranopiaTooltip = $state(false);
  let wasOpen = false;

  $effect(() => {
    if (isOpen && !wasOpen) {
      autoViewedPatternsInput = formatAutoViewedPatterns(settings.autoViewedPatterns);
      showDeuteranopiaTooltip = false;
    }
    wasOpen = isOpen;
  });

  const resolvedTheme = $derived(resolveThemePreference(settings.theme));
  const availableSyntaxThemes = $derived(getThemesForResolvedTheme(resolvedTheme));
  const isCustomEditor = $derived(settings.editor.id === CUSTOM_EDITOR_ID);
  const commandInvalid = $derived(isCustomEditor && settings.editor.command.trim() === '');
  const argsInvalid = $derived(isCustomEditor && settings.editor.argsTemplate.trim() === '');

  function handleThemeChange(nextTheme: ThemePreference) {
    const nextResolved = resolveThemePreference(nextTheme);
    let nextSyntaxTheme = settings.syntaxTheme;
    if (!isSyntaxThemeForResolvedTheme(nextSyntaxTheme, nextResolved)) {
      nextSyntaxTheme = getFallbackSyntaxTheme(nextResolved)?.id ?? nextSyntaxTheme;
    }
    onSettingsChange({
      ...settings,
      theme: nextTheme,
      syntaxTheme: nextSyntaxTheme,
    });
  }

  function handleReset() {
    if (activeSection === 'appearance') {
      const nextResolved = resolveThemePreference(DEFAULT_APPEARANCE_SETTINGS.theme);
      const nextSyntaxTheme = isSyntaxThemeForResolvedTheme(
        DEFAULT_APPEARANCE_SETTINGS.syntaxTheme,
        nextResolved,
      )
        ? DEFAULT_APPEARANCE_SETTINGS.syntaxTheme
        : (getFallbackSyntaxTheme(nextResolved)?.id ?? DEFAULT_APPEARANCE_SETTINGS.syntaxTheme);
      onSettingsChange({
        ...settings,
        fontSize: DEFAULT_APPEARANCE_SETTINGS.fontSize,
        fontFamily: DEFAULT_APPEARANCE_SETTINGS.fontFamily,
        theme: DEFAULT_APPEARANCE_SETTINGS.theme,
        syntaxTheme: nextSyntaxTheme,
        colorVision: DEFAULT_APPEARANCE_SETTINGS.colorVision,
      });
      return;
    }

    autoViewedPatternsInput = formatAutoViewedPatterns(
      DEFAULT_APPEARANCE_SETTINGS.autoViewedPatterns,
    );
    onSettingsChange({
      ...settings,
      editor: { ...DEFAULT_APPEARANCE_SETTINGS.editor },
      autoViewedPatterns: DEFAULT_APPEARANCE_SETTINGS.autoViewedPatterns,
    });
  }

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
    class="fixed inset-0 flex items-center justify-center z-50"
    role="dialog"
    aria-modal="true"
    aria-labelledby="settings-modal-title"
  >
    <button
      type="button"
      tabindex="-1"
      aria-label="Close settings modal backdrop"
      class="absolute inset-0 bg-black/50 w-full h-full border-0 cursor-default"
      onclick={onClose}
    ></button>

    <div class="relative bg-github-bg-secondary border border-github-border rounded-lg w-full max-w-3xl mx-4 overflow-hidden shadow-xl">
      <div class="flex items-center justify-between p-4 border-b border-github-border">
        <h2 id="settings-modal-title" class="text-lg font-semibold text-github-text-primary">
          Settings
        </h2>
        <button
          type="button"
          onclick={onClose}
          aria-label="Close settings modal"
          class="text-github-text-secondary hover:text-github-text-primary px-2 py-1 text-sm cursor-pointer"
        >
          ✕
        </button>
      </div>

      <div class="flex flex-col sm:flex-row min-h-[420px]">
        <nav
          aria-label="Settings sections"
          class="sm:w-40 border-b sm:border-b-0 sm:border-r border-github-border px-3 py-2"
        >
          <div class="flex sm:flex-col gap-1">
            {#each [
              { id: 'appearance', label: 'Appearance' },
              { id: 'system', label: 'System' },
            ] as section (section.id)}
              {@const isActive = section.id === activeSection}
              <button
                type="button"
                onclick={() => {
                  activeSection = section.id as SettingsSection;
                }}
                aria-pressed={isActive}
                class="flex-1 sm:flex-none text-left px-3 py-2 text-sm font-medium transition-colors border-b-2 sm:border-b-0 sm:border-l-2 cursor-pointer {isActive
                  ? 'text-github-text-primary border-github-accent'
                  : 'text-github-text-secondary border-transparent hover:text-github-text-primary'}"
              >
                {section.label}
              </button>
            {/each}
          </div>
        </nav>

        <div class="flex-1 p-4 sm:p-6 overflow-y-auto max-h-[65vh]">
          {#if activeSection === 'appearance'}
            <div class="space-y-6">
              <div>
                <label
                  for="settings-font-size"
                  class="block text-sm font-medium text-github-text-primary mb-2"
                >
                  Font Size
                </label>
                <div class="flex items-center gap-3">
                  <input
                    id="settings-font-size"
                    type="range"
                    min="10"
                    max="20"
                    step="1"
                    value={settings.fontSize}
                    oninput={(e) => {
                      const nextSize = Number.parseInt(e.currentTarget.value, 10);
                      onSettingsChange({ ...settings, fontSize: nextSize });
                    }}
                    class="flex-1 accent-github-accent"
                  />
                  <span class="text-sm text-github-text-secondary w-10 text-right">
                    {settings.fontSize}px
                  </span>
                </div>
              </div>

              <div>
                <label
                  for="settings-font-family"
                  class="block text-sm font-medium text-github-text-primary mb-2"
                >
                  Font Family
                </label>
                <select
                  id="settings-font-family"
                  value={settings.fontFamily}
                  onchange={(e) =>
                    onSettingsChange({ ...settings, fontFamily: e.currentTarget.value })}
                  class="w-full p-2 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary text-sm"
                >
                  {#each FONT_FAMILIES as font (font.value)}
                    <option value={font.value}>{font.name}</option>
                  {/each}
                </select>
              </div>

              <div>
                <span class="block text-sm font-medium text-github-text-primary mb-2">Theme</span>
                <div class="flex gap-2">
                  {#each ['light', 'dark', 'auto'] as themeOpt (themeOpt)}
                    <button
                      type="button"
                      onclick={() => handleThemeChange(themeOpt as ThemePreference)}
                      aria-pressed={settings.theme === themeOpt}
                      class="px-3 py-2 text-sm rounded border transition-colors cursor-pointer {settings.theme ===
                      themeOpt
                        ? 'bg-github-accent text-white border-github-accent'
                        : 'bg-github-bg-tertiary text-github-text-secondary border-github-border hover:text-github-text-primary'}"
                    >
                      {themeOpt.charAt(0).toUpperCase() + themeOpt.slice(1)}
                    </button>
                  {/each}
                </div>
              </div>

              <div>
                <span class="block text-sm font-medium text-github-text-primary mb-2">
                  Color Vision
                </span>
                <div class="flex items-center gap-2 relative">
                  <button
                    type="button"
                    onclick={() => onSettingsChange({ ...settings, colorVision: 'normal' })}
                    aria-pressed={settings.colorVision === 'normal'}
                    class="px-3 py-2 text-sm rounded border transition-colors cursor-pointer {settings.colorVision ===
                    'normal'
                      ? 'bg-github-accent text-white border-github-accent'
                      : 'bg-github-bg-tertiary text-github-text-secondary border-github-border hover:text-github-text-primary'}"
                  >
                    Normal
                  </button>
                  <div class="relative inline-block">
                    <button
                      type="button"
                      onclick={() =>
                        onSettingsChange({ ...settings, colorVision: 'deuteranopia' })}
                      onmouseenter={() => {
                        showDeuteranopiaTooltip = true;
                      }}
                      onmouseleave={() => {
                        showDeuteranopiaTooltip = false;
                      }}
                      onfocus={() => {
                        showDeuteranopiaTooltip = true;
                      }}
                      onblur={() => {
                        showDeuteranopiaTooltip = false;
                      }}
                      aria-pressed={settings.colorVision === 'deuteranopia'}
                      aria-describedby={showDeuteranopiaTooltip
                        ? 'deuteranopia-tooltip'
                        : undefined}
                      class="px-3 py-2 text-sm rounded border transition-colors cursor-pointer {settings.colorVision ===
                      'deuteranopia'
                        ? 'bg-github-accent text-white border-github-accent'
                        : 'bg-github-bg-tertiary text-github-text-secondary border-github-border hover:text-github-text-primary'}"
                    >
                      Deuteranopia
                    </button>
                    {#if showDeuteranopiaTooltip}
                      <div
                        id="deuteranopia-tooltip"
                        role="tooltip"
                        class="absolute left-0 top-full mt-1.5 z-20 w-64 rounded border border-github-border bg-github-bg-primary px-2.5 py-1.5 text-xs text-github-text-secondary shadow-lg"
                      >
                        Deuteranopia mode uses blue/orange instead of green/red for diffs.
                      </div>
                    {/if}
                  </div>
                </div>
              </div>

              <div>
                <label
                  for="settings-syntax-theme"
                  class="block text-sm font-medium text-github-text-primary mb-2"
                >
                  Syntax Highlighting Theme
                </label>
                <select
                  id="settings-syntax-theme"
                  value={settings.syntaxTheme}
                  onchange={(e) =>
                    onSettingsChange({ ...settings, syntaxTheme: e.currentTarget.value })}
                  class="w-full p-2 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary text-sm"
                >
                  {#each availableSyntaxThemes as option (option.id)}
                    <option value={option.id}>{option.label}</option>
                  {/each}
                </select>
              </div>
            </div>
          {:else}
            <div class="space-y-6">
              <div>
                <label
                  for="auto-viewed-patterns"
                  class="block text-sm font-medium text-github-text-primary mb-2"
                >
                  Auto-Mark Viewed Patterns
                </label>
                <p class="text-sm text-github-text-secondary mb-2">
                  Files matching these glob patterns are marked as Viewed automatically. Enter one
                  pattern per line.
                </p>
                <textarea
                  id="auto-viewed-patterns"
                  value={autoViewedPatternsInput}
                  oninput={(e) => {
                    autoViewedPatternsInput = e.currentTarget.value;
                    onSettingsChange({
                      ...settings,
                      autoViewedPatterns: parseAutoViewedPatterns(e.currentTarget.value),
                    });
                  }}
                  rows={6}
                  spellcheck={false}
                  placeholder={'*.test.ts\n*.stories.tsx\nsrc/generated/**'}
                  class="w-full p-3 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary text-sm font-mono"
                ></textarea>
              </div>

              <div>
                <label
                  for="settings-open-editor"
                  class="block text-sm font-medium text-github-text-primary mb-2"
                >
                  Open In Editor
                </label>
                <select
                  id="settings-open-editor"
                  value={settings.editor.id}
                  onchange={(e) => {
                    const nextId = e.currentTarget.value as EditorOptionId;
                    const preset = EDITOR_OPTIONS.find((option) => option.id === nextId);
                    onSettingsChange({
                      ...settings,
                      editor: {
                        id: nextId,
                        command: preset?.command ?? '',
                        argsTemplate: preset?.argsTemplate ?? '',
                      },
                    });
                  }}
                  class="w-full p-2 bg-github-bg-tertiary border border-github-border rounded text-github-text-primary text-sm"
                >
                  {#each EDITOR_OPTIONS as option (option.id)}
                    <option value={option.id}>{option.label}</option>
                  {/each}
                </select>

                {#if settings.editor.id !== NONE_EDITOR_ID}
                  <div
                    aria-label="Editor command configuration"
                    class="mt-3 p-3 bg-github-bg-tertiary border border-github-border rounded space-y-3"
                  >
                    <div>
                      <label
                        for="editor-command"
                        class="block text-sm font-medium text-github-text-primary mb-1"
                      >
                        Command
                      </label>
                      <input
                        id="editor-command"
                        type="text"
                        value={settings.editor.command}
                        disabled={!isCustomEditor}
                        required={isCustomEditor}
                        aria-invalid={commandInvalid || undefined}
                        oninput={(e) =>
                          onSettingsChange({
                            ...settings,
                            editor: { ...settings.editor, command: e.currentTarget.value },
                          })}
                        placeholder="e.g. mate"
                        spellcheck={false}
                        autocomplete="off"
                        class="w-full p-2 bg-github-bg-secondary border rounded text-github-text-primary text-sm font-mono {commandInvalid
                          ? 'border-red-500'
                          : 'border-github-border'} {isCustomEditor
                          ? ''
                          : 'opacity-60 cursor-not-allowed'}"
                      />
                      {#if commandInvalid}
                        <p class="mt-1 text-xs text-red-400">Command is required.</p>
                      {/if}
                    </div>
                    <div>
                      <label
                        for="editor-args"
                        class="block text-sm font-medium text-github-text-primary mb-1"
                      >
                        Arguments
                      </label>
                      <input
                        id="editor-args"
                        type="text"
                        value={settings.editor.argsTemplate}
                        disabled={!isCustomEditor}
                        required={isCustomEditor}
                        aria-invalid={argsInvalid || undefined}
                        oninput={(e) =>
                          onSettingsChange({
                            ...settings,
                            editor: { ...settings.editor, argsTemplate: e.currentTarget.value },
                          })}
                        placeholder="-l %line %file"
                        spellcheck={false}
                        autocomplete="off"
                        class="w-full p-2 bg-github-bg-secondary border rounded text-github-text-primary text-sm font-mono {argsInvalid
                          ? 'border-red-500'
                          : 'border-github-border'} {isCustomEditor
                          ? ''
                          : 'opacity-60 cursor-not-allowed'}"
                      />
                      {#if argsInvalid}
                        <p class="mt-1 text-xs text-red-400">Arguments are required.</p>
                      {/if}
                      <p class="mt-2 text-xs text-github-text-secondary">
                        {isCustomEditor
                          ? 'Use %file and %line as placeholders. Wrap arguments containing spaces in single or double quotes.'
                          : 'Select Custom… above to edit these fields.'}
                      </p>
                    </div>
                  </div>
                {/if}
              </div>
            </div>
          {/if}
        </div>
      </div>

      <div class="flex items-center justify-between p-4 border-t border-github-border">
        <button
          type="button"
          onclick={handleReset}
          class="px-3 py-2 text-sm text-github-text-secondary hover:text-github-text-primary cursor-pointer"
        >
          {activeSection === 'appearance' ? 'Reset Appearance Defaults' : 'Reset System Defaults'}
        </button>
        <button
          type="button"
          onclick={onClose}
          class="px-4 py-2 text-sm bg-github-accent text-white rounded hover:bg-green-600 cursor-pointer"
        >
          Close
        </button>
      </div>
    </div>
  </div>
{/if}
