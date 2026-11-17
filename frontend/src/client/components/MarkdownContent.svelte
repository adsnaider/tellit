<script lang="ts">
  import { getPrismLanguageFromFilename } from '../utils/languageDetection';
  import { loadPrismLanguage } from '../utils/languageLoader';
  import { type GfmInlinePart, parseGfmBlocks } from '../utils/markdownParser';
  import { isPlainLanguage, isPrismLanguageLoaded, tokenizeCode } from '../utils/prism';

  import HighlightedCode from './HighlightedCode.svelte';

  interface Props {
    content: string;
    syntaxTheme?: string;
  }

  let { content, syntaxTheme = 'vsDark' }: Props = $props();

  const blocks = $derived(parseGfmBlocks(content));
  let grammarVersion = $state(0);

  function normalizePrismLanguage(rawLang: string): string {
    const trimmed = rawLang.trim().toLowerCase();
    if (!trimmed || trimmed === 'plaintext' || trimmed === 'txt') return 'text';
    if (trimmed === 'shell') return 'bash';
    if (isPlainLanguage(trimmed) || isPrismLanguageLoaded(trimmed)) {
      return trimmed;
    }
    const mapped = getPrismLanguageFromFilename(`code.${trimmed}`);
    return mapped !== 'text' ? mapped : trimmed;
  }

  $effect(() => {
    const missingLanguages = new Set<string>();
    for (const block of blocks) {
      if (block.type === 'code' && block.language) {
        const lang = normalizePrismLanguage(block.language);
        if (!isPlainLanguage(lang) && !isPrismLanguageLoaded(lang)) {
          missingLanguages.add(lang);
        }
      }
    }
    if (missingLanguages.size === 0) return;

    let cancelled = false;
    void Promise.allSettled([...missingLanguages].map((lang) => loadPrismLanguage(lang))).then(() => {
      if (!cancelled) {
        grammarVersion += 1;
      }
    });

    return () => {
      cancelled = true;
    };
  });

  function getHighlightedCodeLines(code: string, language: string) {
    void grammarVersion;
    const lang = normalizePrismLanguage(language);
    return { lang, tokenLines: tokenizeCode(code, lang) };
  }
</script>

{#snippet renderInlines(inlines: GfmInlinePart[])}
  {#each inlines as part, idx (idx)}
    {#if part.type === 'code'}
      <code class="px-1 py-0.5 rounded bg-github-bg-tertiary border border-github-border text-sm font-mono">
        {part.text}
      </code>
    {:else if part.type === 'bold'}
      <strong class="font-semibold text-github-text-primary">{part.text}</strong>
    {:else if part.type === 'italic'}
      <em class="italic">{part.text}</em>
    {:else if part.type === 'strike'}
      <del class="line-through text-github-text-muted">{part.text}</del>
    {:else if part.type === 'image'}
      <img
        src={part.src}
        alt={part.alt}
        loading="lazy"
        class="max-w-full rounded border border-github-border inline-block"
      />
    {:else if part.type === 'link'}
      {@const isExternal = /^https?:/i.test(part.href)}
      <a
        href={part.href}
        target={isExternal ? '_blank' : undefined}
        rel={isExternal ? 'noopener noreferrer' : undefined}
        class="text-sky-400 hover:text-sky-300 underline underline-offset-4"
      >
        {part.text}
      </a>
    {:else}
      {part.text}
    {/if}
  {/each}
{/snippet}

<div class="space-y-3 text-github-text-primary">
  {#each blocks as block, blockIdx (blockIdx)}
    {#if block.type === 'heading'}
      {#if block.level === 1}
        <h1 class="text-[26px] font-semibold mt-6 mb-2 first:mt-0">
          {@render renderInlines(block.inlines)}
        </h1>
      {:else if block.level === 2}
        <h2 class="text-[22px] font-semibold mt-6 mb-2 first:mt-0">
          {@render renderInlines(block.inlines)}
        </h2>
      {:else if block.level === 3}
        <h3 class="text-xl font-semibold mt-6 mb-2 first:mt-0">
          {@render renderInlines(block.inlines)}
        </h3>
      {:else if block.level === 4}
        <h4 class="text-lg font-semibold mt-6 mb-2 first:mt-0">
          {@render renderInlines(block.inlines)}
        </h4>
      {:else if block.level === 5}
        <h5 class="text-base font-semibold uppercase tracking-wide mt-6 mb-2 first:mt-0">
          {@render renderInlines(block.inlines)}
        </h5>
      {:else}
        <h6 class="text-base font-semibold mt-6 mb-2 first:mt-0">
          {@render renderInlines(block.inlines)}
        </h6>
      {/if}
    {:else if block.type === 'hr'}
      <hr class="border-github-border" />
    {:else if block.type === 'blockquote'}
      <blockquote class="border-l-4 border-github-border pl-4 text-github-text-muted italic space-y-1">
        {#each block.lines as quoteLine, qIdx (qIdx)}
          <p>{@render renderInlines(quoteLine)}</p>
        {/each}
      </blockquote>
    {:else if block.type === 'table'}
      <div class="overflow-x-auto">
        <table class="w-full border-collapse text-sm">
          <thead class="bg-github-bg-secondary">
            <tr class="border-b border-github-border">
              {#each block.headers as headerCell, colIdx (colIdx)}
                <th
                  align={block.alignments[colIdx] ?? undefined}
                  class="px-3 py-2 text-left font-semibold border border-github-border"
                >
                  {@render renderInlines(headerCell)}
                </th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each block.rows as row, rowIdx (rowIdx)}
              <tr class="border-b border-github-border">
                {#each row as cell, colIdx (colIdx)}
                  <td
                    align={block.alignments[colIdx] ?? undefined}
                    class="px-3 py-2 border border-github-border"
                  >
                    {@render renderInlines(cell)}
                  </td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else if block.type === 'list'}
      {#if block.ordered}
        <ol class="list-decimal pl-6 text-base space-y-1">
          {#each block.items as item, itemIdx (itemIdx)}
            <li>
              {#if item.checked !== undefined}
                <input
                  type="checkbox"
                  checked={item.checked}
                  disabled
                  aria-label="Task list item"
                  class="mr-1.5 align-middle"
                />
              {/if}
              {@render renderInlines(item.inlines)}
            </li>
          {/each}
        </ol>
      {:else}
        <ul class="list-disc pl-6 text-base space-y-1">
          {#each block.items as item, itemIdx (itemIdx)}
            <li class={item.checked !== undefined ? 'list-none -ml-4' : ''}>
              {#if item.checked !== undefined}
                <input
                  type="checkbox"
                  checked={item.checked}
                  disabled
                  aria-label="Task list item"
                  class="mr-1.5 align-middle"
                />
              {/if}
              {@render renderInlines(item.inlines)}
            </li>
          {/each}
        </ul>
      {/if}
    {:else if block.type === 'code'}
      {@const highlighted = getHighlightedCodeLines(block.code, block.language)}
      {@const codeLines = block.code.split('\n')}
      <pre class="markdown-preview-code rounded border border-github-border bg-github-bg-secondary p-3 overflow-x-auto text-sm font-mono text-github-text-primary">{#each codeLines as codeLine, lineIdx (lineIdx)}<div class="leading-5 min-h-5"><HighlightedCode code={codeLine} language={highlighted.lang} {syntaxTheme} precomputedTokens={highlighted.tokenLines[lineIdx] ?? null} /></div>{/each}</pre>
    {:else}
      <p class="text-base leading-7">{@render renderInlines(block.inlines)}</p>
    {/if}
  {/each}
</div>
