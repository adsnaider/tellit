<script lang="ts">
  import {
    type InlinePart,
    parseMarkdownBlocks,
    splitCommentBodySegments,
  } from '../utils/commentMarkdown';

  import HighlightedCode from './HighlightedCode.svelte';

  interface Props {
    body: string;
    originalCode?: string;
    filename?: string;
    syntaxTheme?: string;
  }

  let { body, originalCode, filename, syntaxTheme = 'vsDark' }: Props = $props();

  const segments = $derived(splitCommentBodySegments(body));

  function getDiffCodeLineClass(line: string): string {
    if (line.startsWith('+')) return 'bg-diff-addition-bg/70';
    if (line.startsWith('-')) return 'bg-diff-deletion-bg/70';
    return '';
  }
</script>

{#snippet renderInlines(inlines: InlinePart[])}
  {#each inlines as part, idx (idx)}
    {#if part.type === 'code'}
      <code class="px-1.5 py-0.5 rounded bg-github-bg-tertiary text-github-text-primary font-mono text-xs">
        {part.text}
      </code>
    {:else if part.type === 'bold'}
      <strong class="font-semibold text-github-text-primary">{part.text}</strong>
    {:else if part.type === 'link'}
      {@const isExternalHttp = /^https?:/i.test(part.href)}
      <a
        href={part.href}
        target={isExternalHttp ? '_blank' : undefined}
        rel={isExternalHttp ? 'noopener noreferrer' : undefined}
        class="text-blue-400 underline hover:text-blue-300"
      >
        {part.text}
      </a>
    {:else}
      {part.text}
    {/if}
  {/each}
{/snippet}

<div class="text-sm text-github-text-primary leading-relaxed break-words space-y-2">
  {#each segments as segment, segIdx (segIdx)}
    {#if segment.type === 'suggestion'}
      <div class="rounded-md border border-github-border bg-github-bg-primary overflow-hidden font-mono text-xs">
        <div class="px-3 py-1 bg-github-bg-tertiary border-b border-github-border text-[11px] font-sans font-medium text-github-text-secondary">
          Suggested change
        </div>
        <div class="overflow-x-auto py-1">
          {#if originalCode !== undefined}
            {#each originalCode.split('\n') as oldLine, idx (idx)}
              <div class="flex items-start bg-diff-deletion-bg/60">
                <span class="w-5 text-center shrink-0 text-github-danger select-none">-</span>
                <HighlightedCode
                  code={oldLine}
                  {filename}
                  {syntaxTheme}
                  class="flex-1 px-2 whitespace-pre-wrap break-all"
                />
              </div>
            {/each}
          {/if}
          {#each segment.code.split('\n') as newLine, idx (idx)}
            <div class="flex items-start bg-diff-addition-bg/60">
              <span class="w-5 text-center shrink-0 text-github-accent select-none">+</span>
              <HighlightedCode
                code={newLine}
                {filename}
                {syntaxTheme}
                class="flex-1 px-2 whitespace-pre-wrap break-all"
              />
            </div>
          {/each}
        </div>
      </div>
    {:else}
      {@const blocks = parseMarkdownBlocks(segment.content)}
      {#each blocks as block, blockIdx (blockIdx)}
        {#if block.type === 'heading'}
          {#if block.level === 1}
            <h1 class="text-base font-semibold mt-2 first:mt-0">{@render renderInlines(block.inlines)}</h1>
          {:else if block.level === 2}
            <h2 class="text-sm font-semibold mt-2 first:mt-0">{@render renderInlines(block.inlines)}</h2>
          {:else}
            <h3 class="text-sm font-semibold mt-2 first:mt-0">{@render renderInlines(block.inlines)}</h3>
          {/if}
        {:else if block.type === 'hr'}
          <hr class="my-2 border-github-border" />
        {:else if block.type === 'blockquote'}
          <blockquote class="border-l-4 border-github-border pl-3 my-1.5 text-github-text-muted space-y-1">
            {#each block.lines as quoteLine, qIdx (qIdx)}
              <p class="whitespace-pre-wrap">{@render renderInlines(quoteLine)}</p>
            {/each}
          </blockquote>
        {:else if block.type === 'list'}
          {#if block.ordered}
            <ol class="list-decimal pl-5 my-1.5 space-y-0.5">
              {#each block.items as item, itemIdx (itemIdx)}
                <li class="whitespace-pre-wrap">{@render renderInlines(item)}</li>
              {/each}
            </ol>
          {:else}
            <ul class="list-disc pl-5 my-1.5 space-y-0.5">
              {#each block.items as item, itemIdx (itemIdx)}
                <li class="whitespace-pre-wrap">{@render renderInlines(item)}</li>
              {/each}
            </ul>
          {/if}
        {:else if block.type === 'code'}
          <div class="my-1.5 rounded-md border border-github-border bg-github-bg-secondary overflow-x-auto font-mono text-xs leading-5">
            {#if block.language === 'diff'}
              {#each block.code.split('\n') as diffLine, lIdx (lIdx)}
                <div class="px-3 whitespace-pre {getDiffCodeLineClass(diffLine)}">
                  {diffLine || ' '}
                </div>
              {/each}
            {:else}
              <pre class="p-2.5 m-0"><code>{#each block.code.split('\n') as codeLine, lIdx (lIdx)}{#if lIdx > 0}{'\n'}{/if}<HighlightedCode code={codeLine} language={block.language} {syntaxTheme} />{/each}</code></pre>
            {/if}
          </div>
        {:else}
          <p class="my-1.5 first:mt-0 last:mb-0 whitespace-pre-wrap">
            {#each block.lines as pLine, pIdx (pIdx)}
              {#if pIdx > 0}{'\n'}{/if}{@render renderInlines(pLine)}
            {/each}
          </p>
        {/if}
      {/each}
    {/if}
  {/each}
</div>
