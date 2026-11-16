<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity';

  import { type DiffFile } from '../../types/diff';
  import { formatStatusBadge } from '../utils/diffUtils';

  interface Props {
    files: DiffFile[];
    viewedFiles: Set<string>;
    changedSinceViewedFiles: Set<string>;
    onScrollToFile: (path: string) => void;
    onToggleViewed: (file: DiffFile) => void;
    onToggleFolderViewed: (folderFiles: DiffFile[], nextViewed: boolean) => void;
  }

  interface TreeNode {
    name: string;
    path: string;
    isDirectory: boolean;
    children?: TreeNode[];
    file?: DiffFile;
  }

  let {
    files,
    viewedFiles,
    changedSinceViewedFiles,
    onScrollToFile,
    onToggleViewed,
    onToggleFolderViewed,
  }: Props = $props();

  let isSectionOpen = $state(true);
  let filterQuery = $state('');
  let collapsedDirs = new SvelteSet<string>();

  function buildFileTree(list: DiffFile[]): TreeNode {
    const root: TreeNode = {
      name: '',
      path: '',
      isDirectory: true,
      children: [],
    };

    for (const file of list) {
      const parts = file.path.split('/');
      let current = root;

      for (let i = 0; i < parts.length; i++) {
        const part = parts[i];
        if (!part) continue;

        const isLast = i === parts.length - 1;
        const pathSoFar = parts.slice(0, i + 1).join('/');

        if (!current.children) {
          current.children = [];
        }

        let child = current.children.find((c) => c.name === part && c.isDirectory === !isLast);
        if (!child) {
          child = {
            name: part,
            path: pathSoFar,
            isDirectory: !isLast,
            children: isLast ? undefined : [],
            file: isLast ? file : undefined,
          };
          current.children.push(child);
        }
        current = child;
      }
    }

    const collapseSingleChildDirs = (node: TreeNode): TreeNode => {
      if (!node.isDirectory || !node.children) return node;
      node.children = node.children.map(collapseSingleChildDirs);

      if (
        node.name &&
        node.children.length === 1 &&
        node.children[0]?.isDirectory &&
        node.children[0]?.children
      ) {
        const onlyChild = node.children[0];
        return {
          ...node,
          name: `${node.name}/${onlyChild.name}`,
          path: onlyChild.path,
          children: onlyChild.children,
        };
      }
      return node;
    };

    return collapseSingleChildDirs(root);
  }

  function collectFiles(node: TreeNode): DiffFile[] {
    if (node.file) return [node.file];
    if (!node.children) return [];
    return node.children.flatMap(collectFiles);
  }

  function collectDirPaths(node: TreeNode): string[] {
    if (!node.isDirectory || !node.children) return [];
    const paths: string[] = [];
    if (node.path) paths.push(node.path);
    for (const child of node.children) {
      paths.push(...collectDirPaths(child));
    }
    return paths;
  }

  const filteredFiles = $derived.by(() => {
    const q = filterQuery.trim().toLowerCase();
    if (!q) return files;
    return files.filter(
      (file) =>
        file.path.toLowerCase().includes(q) || Boolean(file.oldPath?.toLowerCase().includes(q)),
    );
  });

  const treeRoot = $derived(buildFileTree(filteredFiles));
  const allDirPaths = $derived(collectDirPaths(treeRoot));
  const hasCollapsedDirs = $derived(allDirPaths.some((dir) => collapsedDirs.has(dir)));
  const viewedCount = $derived(files.filter((file) => viewedFiles.has(file.path)).length);

  function toggleDir(path: string) {
    if (collapsedDirs.has(path)) {
      collapsedDirs.delete(path);
    } else {
      collapsedDirs.add(path);
    }
  }

  function toggleAllDirs() {
    if (hasCollapsedDirs) {
      collapsedDirs.clear();
    } else {
      for (const dir of allDirPaths) {
        collapsedDirs.add(dir);
      }
    }
  }
</script>

{#snippet renderNode(node: TreeNode, depth: number)}
  {#if node.isDirectory}
    {@const isDirCollapsed = collapsedDirs.has(node.path)}
    {@const descendantFiles = collectFiles(node)}
    {@const allDescendantViewed =
      descendantFiles.length > 0 && descendantFiles.every((f) => viewedFiles.has(f.path))}
    <li>
      <div
        class="flex items-center justify-between gap-1.5 py-1 pr-2.5 text-xs text-github-text-secondary hover:bg-github-bg-tertiary/60"
        style:padding-left="{depth * 14 + 10}px"
      >
        <button
          type="button"
          onclick={() => toggleDir(node.path)}
          aria-expanded={!isDirCollapsed}
          class="flex items-center gap-1.5 min-w-0 flex-1 text-left font-mono text-github-text-primary cursor-pointer truncate"
        >
          <span class="text-github-text-muted w-3 text-center shrink-0">
            {isDirCollapsed ? '▸' : '▾'}
          </span>
          <span class="truncate">{node.name}</span>
        </button>

        <input
          type="checkbox"
          checked={allDescendantViewed}
          aria-label="Mark folder {node.path} as viewed"
          onchange={(event) => onToggleFolderViewed(descendantFiles, event.currentTarget.checked)}
          class="rounded border-github-border cursor-pointer shrink-0"
          title={allDescendantViewed ? 'Unmark folder viewed' : 'Mark folder viewed'}
        />
      </div>

      {#if !isDirCollapsed && node.children}
        <ul>
          {#each node.children as child (`${child.isDirectory ? 'd' : 'f'}:${child.path}`)}
            {@render renderNode(child, depth + 1)}
          {/each}
        </ul>
      {/if}
    </li>
  {:else if node.file}
    {@const file = node.file}
    {@const badge = formatStatusBadge(file.status)}
    {@const isViewed = viewedFiles.has(file.path)}
    {@const isChangedSinceViewed = changedSinceViewedFiles.has(file.path)}
    <li>
      <div
        class="flex items-center gap-1.5 py-1 pr-2.5 text-xs transition-colors hover:bg-github-bg-tertiary {isViewed
          ? 'opacity-65'
          : ''}"
        style:padding-left="{depth * 14 + 10}px"
      >
        <input
          type="checkbox"
          checked={isViewed}
          aria-label="Mark {file.path} as viewed"
          onchange={() => onToggleViewed(file)}
          class="rounded border-github-border cursor-pointer shrink-0"
          title={isViewed ? 'Viewed (click to unmark)' : 'Mark file as viewed'}
        />

        <button
          type="button"
          onclick={() => onScrollToFile(file.path)}
          class="flex items-center justify-between gap-2 min-w-0 flex-1 text-left text-github-text-secondary hover:text-github-text-primary cursor-pointer"
        >
          <span class="flex items-center gap-1.5 min-w-0">
            <span
              class="inline-flex items-center justify-center w-4 h-4 text-[10px] font-mono font-semibold rounded border shrink-0 {badge.classes}"
            >
              {badge.label}
            </span>
            <span class="font-mono truncate text-github-text-primary">{node.name}</span>
            {#if file.isGenerated}
              <span
                class="px-1 py-0 text-[10px] rounded bg-github-bg-tertiary border border-github-border text-github-text-muted shrink-0"
                title="Generated file"
              >
                gen
              </span>
            {/if}
            {#if isChangedSinceViewed}
              <span
                class="px-1 py-0 text-[10px] rounded bg-yellow-500/15 border border-yellow-500/30 text-yellow-400 shrink-0"
                title="Changed since you last viewed this file"
              >
                updated
              </span>
            {/if}
          </span>

          <span class="font-mono text-[11px] shrink-0 flex items-center gap-1">
            {#if file.additions > 0}
              <span class="text-green-400">+{file.additions}</span>
            {/if}
            {#if file.deletions > 0}
              <span class="text-red-400">-{file.deletions}</span>
            {/if}
          </span>
        </button>
      </div>
    </li>
  {/if}
{/snippet}

<section class="flex-1 min-h-0 flex flex-col" aria-label="Changed files">
  <div class="px-2.5 py-2 border-b border-github-border/60 flex items-center justify-between gap-2">
    <button
      type="button"
      onclick={() => {
        isSectionOpen = !isSectionOpen;
      }}
      aria-expanded={isSectionOpen}
      class="flex items-center gap-1.5 text-left text-xs font-semibold text-github-text-primary hover:text-github-accent cursor-pointer"
    >
      <span class="text-github-text-muted w-3 text-center">
        {isSectionOpen ? '▾' : '▸'}
      </span>
      <span>Files ({viewedCount}/{files.length})</span>
    </button>

    {#if isSectionOpen && allDirPaths.length > 0}
      <button
        type="button"
        onclick={toggleAllDirs}
        class="text-[11px] text-github-text-muted hover:text-github-text-primary px-1.5 py-0.5 rounded hover:bg-github-bg-tertiary cursor-pointer"
      >
        {hasCollapsedDirs ? 'Expand dirs' : 'Collapse dirs'}
      </button>
    {/if}
  </div>

  {#if isSectionOpen}
    {#if files.length > 0}
      <div class="px-2.5 py-1.5 border-b border-github-border/50">
        <input
          type="text"
          bind:value={filterQuery}
          placeholder="Filter files..."
          aria-label="Filter changed files"
          class="w-full bg-github-bg-primary border border-github-border rounded px-2 py-1 text-xs font-mono text-github-text-primary outline-none focus:border-github-accent"
        />
      </div>
    {/if}

    <div class="flex-1 overflow-y-auto py-1">
      {#if filteredFiles.length === 0}
        <div class="px-3 py-3 text-xs text-github-text-muted">
          {files.length === 0 ? 'No modified files' : 'No matching files'}
        </div>
      {:else if treeRoot.children}
        <ul>
          {#each treeRoot.children as child (`${child.isDirectory ? 'd' : 'f'}:${child.path}`)}
            {@render renderNode(child, 0)}
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</section>
