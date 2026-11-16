import { type JjChange } from '../../types/diff';

export function commitIdsMatch(a?: string, b?: string): boolean {
  if (!a || !b) return false;
  if (a === b) return true;
  if (Math.min(a.length, b.length) >= 6) {
    return a.startsWith(b) || b.startsWith(a);
  }
  return false;
}

export function isEmptyUndescribedChange(change: JjChange): boolean {
  return Boolean(
    change.isEmpty && change.hasDescription === false && (change.commentCount ?? 0) === 0,
  );
}

export function pickInitialChange(
  changes: JjChange[],
  currentCommitId?: string,
): JjChange | undefined {
  if (changes.length === 0) {
    return undefined;
  }

  const walkToNonEmptyAncestor = (start: JjChange): JjChange | undefined => {
    const visited = new Set<string>();
    const queue: JjChange[] = [start];
    while (queue.length > 0) {
      const current = queue.shift();
      if (!current || visited.has(current.commitId)) continue;
      visited.add(current.commitId);
      if (!isEmptyUndescribedChange(current)) {
        return current;
      }
      for (const parentId of current.parentCommitIds ?? []) {
        const parent = changes.find((c) => commitIdsMatch(c.commitId, parentId));
        if (parent && !visited.has(parent.commitId)) {
          queue.push(parent);
        }
      }
    }
    return undefined;
  };

  if (currentCommitId) {
    const commitMatch = changes.find((c) => commitIdsMatch(c.commitId, currentCommitId));
    if (commitMatch) {
      if (!isEmptyUndescribedChange(commitMatch)) {
        return commitMatch;
      }
      return (
        walkToNonEmptyAncestor(commitMatch) ??
        changes.find((c) => !isEmptyUndescribedChange(c)) ??
        commitMatch
      );
    }
  }

  const workingCopy = changes.find((c) => c.isWorkingCopy);
  if (workingCopy) {
    return (
      walkToNonEmptyAncestor(workingCopy) ??
      changes.find((c) => !isEmptyUndescribedChange(c)) ??
      workingCopy
    );
  }

  return changes.find((c) => !isEmptyUndescribedChange(c)) ?? changes[0];
}
