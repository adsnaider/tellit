<script lang="ts">
  import { getPrismLanguageFromFilename } from '../utils/languageDetection';
  import { loadPrismLanguage } from '../utils/languageLoader';
  import {
    type HighlightToken,
    isPrismLanguageLoaded,
    tokenizeLine,
  } from '../utils/prism';
  import { getTokenStyleString } from '../utils/syntaxThemes';
  import { getWordHighlightContext, splitTextIntoWordSegments } from '../utils/wordHighlight';
  import { type DiffSegment } from '../utils/wordLevelDiff';

  interface Props {
    code: string;
    language?: string;
    filename?: string;
    syntaxTheme?: string;
    precomputedTokens?: HighlightToken[] | null;
    diffSegments?: DiffSegment[];
    class?: string;
  }

  let {
    code,
    language,
    filename = '',
    syntaxTheme = 'vsDark',
    precomputedTokens = null,
    diffSegments,
    class: className = '',
  }: Props = $props();

  const wordHighlight = getWordHighlightContext();
  let grammarVersion = $state(0);

  const detectedLanguage = $derived(
    language || (filename ? getPrismLanguageFromFilename(filename) : 'text'),
  );

  $effect(() => {
    if (precomputedTokens || diffSegments) return;
    const lang = detectedLanguage;
    if (isPrismLanguageLoaded(lang)) return;

    let cancelled = false;
    void loadPrismLanguage(lang)
      .then(() => {
        if (!cancelled) {
          grammarVersion += 1;
        }
      })
      .catch(() => {
        // Keep plain-text fallback when a grammar is unavailable.
      });

    return () => {
      cancelled = true;
    };
  });

  const tokens = $derived.by<HighlightToken[]>(() => {
    void grammarVersion;
    if (precomputedTokens) {
      return precomputedTokens;
    }
    if (diffSegments) {
      return [];
    }
    return tokenizeLine(code, detectedLanguage);
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<span
  class={className}
  onmouseover={(event) => wordHighlight?.onWordMouseOver(event)}
  onmouseout={() => wordHighlight?.onWordMouseOut()}
  onfocus={() => {}}
  onblur={() => {}}
  onclick={(event) => wordHighlight?.onWordClick(event)}
  onkeydown={() => {}}
>
  {#if diffSegments}
    {#each diffSegments as segment, segmentIndex (segmentIndex)}
      {@const diffClass =
        segment.type === 'added'
          ? 'word-diff-added'
          : segment.type === 'removed'
            ? 'word-diff-removed'
            : ''}
      <span class={diffClass}>
        {#each splitTextIntoWordSegments(segment.value) as wordPart, partIdx (partIdx)}
          {#if wordPart.word}
            <span
              class="word-token {wordHighlight?.isWordHighlighted(wordPart.word)
                ? 'word-highlight'
                : ''}"
              data-word={wordPart.word}
            >{wordPart.text}</span>
          {:else}
            <span>{wordPart.text}</span>
          {/if}
        {/each}
      </span>
    {/each}
  {:else}
    {#each tokens as token, tokenIndex (tokenIndex)}
      {@const style = getTokenStyleString(token.types, syntaxTheme)}
      {@const wordParts = splitTextIntoWordSegments(token.content)}
      <span {style} data-token-types={token.types.join(' ') || undefined}>
        {#each wordParts as wordPart, partIdx (partIdx)}
          {#if wordPart.word}
            <span
              class="word-token {wordHighlight?.isWordHighlighted(wordPart.word)
                ? 'word-highlight'
                : ''}"
              data-word={wordPart.word}
            >{wordPart.text}</span>
          {:else}
            <span>{wordPart.text}</span>
          {/if}
        {/each}
      </span>
    {/each}
  {/if}
</span>
