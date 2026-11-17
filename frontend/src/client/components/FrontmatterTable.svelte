<script lang="ts">
  import type { FrontmatterDiffEntry } from '../utils/frontmatterDiff';

  type Props =
    | {
        mode: 'snapshot';
        data: Record<string, unknown> | null;
        label?: string;
      }
    | {
        mode: 'diff';
        entries: FrontmatterDiffEntry[];
        label?: string;
      };

  let props: Props = $props();

  function formatScalar(value: unknown): string {
    if (value instanceof Date) return value.toISOString();
    return String(value);
  }

  function isPlainObject(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value) && !(value instanceof Date);
  }
</script>

{#snippet renderValue(value: unknown, depth: number)}
  {#if value === null}
    <span class="text-github-text-muted italic">null</span>
  {:else if value === undefined}
    <span class="text-github-text-muted italic">undefined</span>
  {:else if typeof value === 'string'}
    {#if value === ''}
      <span class="text-github-text-muted italic">&quot;&quot;</span>
    {:else}
      <span>{value}</span>
    {/if}
  {:else if typeof value === 'boolean' || typeof value === 'number' || value instanceof Date}
    <span>{formatScalar(value)}</span>
  {:else if Array.isArray(value)}
    <pre class="whitespace-pre-wrap text-xs font-mono">{JSON.stringify(value)}</pre>
  {:else if isPlainObject(value)}
    {#if depth === 0}
      <table class="w-full border-collapse text-xs border border-github-border">
        <tbody>
          {#each Object.entries(value) as [subKey, subVal] (subKey)}
            <tr class="border-b border-github-border">
              <td class="px-2 py-1 border border-github-border font-mono w-1/3">{subKey}</td>
              <td class="px-2 py-1 border border-github-border">
                {@render renderValue(subVal, depth + 1)}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {:else}
      <pre class="whitespace-pre-wrap text-xs font-mono">{JSON.stringify(value, null, 2)}</pre>
    {/if}
  {:else}
    <span>{formatScalar(value)}</span>
  {/if}
{/snippet}

{#if props.mode === 'snapshot'}
  {#if props.data !== null}
    {@const entries = Object.entries(props.data)}
    {#if entries.length > 0}
      <div class="overflow-x-auto mb-4">
        {#if props.label}
          <div class="text-sm text-github-text-muted mb-1">{props.label}</div>
        {/if}
        <table class="w-full border-collapse text-sm border border-github-border">
          <thead class="bg-github-bg-secondary">
            <tr class="border-b border-github-border">
              <th class="px-3 py-2 text-left font-semibold border border-github-border w-1/3">Key</th>
              <th class="px-3 py-2 text-left font-semibold border border-github-border">Value</th>
            </tr>
          </thead>
          <tbody>
            {#each entries as [keyName, value] (keyName)}
              <tr class="border-b border-github-border">
                <td class="px-3 py-2 border border-github-border font-mono align-top">{keyName}</td>
                <td class="px-3 py-2 border border-github-border">
                  {@render renderValue(value, 0)}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {/if}
{:else if props.entries.length > 0}
  <div class="overflow-x-auto mb-4">
    {#if props.label}
      <div class="text-sm text-github-text-muted mb-1">{props.label}</div>
    {/if}
    <table class="w-full border-collapse text-sm border border-github-border">
      <thead class="bg-github-bg-secondary">
        <tr class="border-b border-github-border">
          <th class="px-3 py-2 text-left font-semibold border border-github-border w-1/5">Key</th>
          <th class="px-3 py-2 text-left font-semibold border border-github-border">Before</th>
          <th class="px-3 py-2 text-left font-semibold border border-github-border">After</th>
        </tr>
      </thead>
      <tbody>
        {#each props.entries as entry (entry.key)}
          {@const beforeClass =
            entry.status === 'removed' || entry.status === 'modified' ? 'bg-diff-deletion-bg' : ''}
          {@const afterClass =
            entry.status === 'added' || entry.status === 'modified' ? 'bg-diff-addition-bg' : ''}
          <tr class="border-b border-github-border">
            <td class="px-3 py-2 border border-github-border font-mono align-top">{entry.key}</td>
            <td class="px-3 py-2 border border-github-border align-top {beforeClass}">
              {#if entry.status !== 'added'}
                {@render renderValue(entry.oldValue, 0)}
              {/if}
            </td>
            <td class="px-3 py-2 border border-github-border align-top {afterClass}">
              {#if entry.status !== 'removed'}
                {@render renderValue(entry.newValue, 0)}
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}
