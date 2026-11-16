import {
  type BaseMode,
  type DiffFile,
  type ViewedFileRecord,
  type ViewedHashIndexEntry,
} from '../../types/diff';
import { storageService, VIEWED_HASH_VERSION } from '../services/StorageService';

import { matchesAutoViewedPatterns } from './autoViewedPatterns';
import { generateDiffHash, getDiffContentForHashing } from './diffUtils';

export interface ViewedContextParams {
  baseCommitish: string;
  targetCommitish: string;
  currentCommitHash?: string;
  repositoryId?: string;
  baseMode?: BaseMode;
}

export interface HydratedViewedState {
  records: ViewedFileRecord[];
  viewedPaths: Set<string>;
  changedSinceViewedPaths: Set<string>;
  hashesByPath: Map<string, string>;
}

export async function hydrateViewedState(
  context: ViewedContextParams,
  files: DiffFile[],
  autoViewedPatterns: string[] = [],
): Promise<HydratedViewedState> {
  const loadedRecords = storageService.getViewedFiles(
    context.baseCommitish,
    context.targetCommitish,
    context.currentCommitHash,
    undefined,
    context.repositoryId,
    context.baseMode,
  );

  const hashesByPath = new Map<string, string>();
  await Promise.all(
    files.map(async (file) => {
      const hash = await generateDiffHash(getDiffContentForHashing(file));
      hashesByPath.set(file.path, hash);
    }),
  );

  const knownPaths = new Set(loadedRecords.map((record) => record.filePath));
  const additions: ViewedFileRecord[] = [];
  const viewedAt = new Date().toISOString();

  for (const file of files) {
    if (knownPaths.has(file.path)) continue;
    const shouldAutoMark =
      Boolean(file.isGenerated) ||
      file.status === 'deleted' ||
      matchesAutoViewedPatterns(file.path, autoViewedPatterns);
    if (!shouldAutoMark) continue;

    const hash = hashesByPath.get(file.path);
    if (!hash) continue;

    additions.push({
      filePath: file.path,
      viewedAt,
      diffContentHash: hash,
    });
    knownPaths.add(file.path);
  }

  const index = storageService.getViewedHashIndex(context.repositoryId);
  const indexByPath = new Map<string, Set<string>>();
  for (const entry of index.entries) {
    let set = indexByPath.get(entry.filePath);
    if (!set) {
      set = new Set();
      indexByPath.set(entry.filePath, set);
    }
    set.add(entry.diffContentHash);
  }

  const changedSinceViewedPaths = new Set<string>();
  for (const file of files) {
    const hash = hashesByPath.get(file.path);
    if (!hash) continue;
    const priorHashes = indexByPath.get(file.path);
    if (!priorHashes || priorHashes.size === 0) continue;

    if (priorHashes.has(hash)) {
      if (!knownPaths.has(file.path)) {
        additions.push({
          filePath: file.path,
          viewedAt,
          diffContentHash: hash,
        });
        knownPaths.add(file.path);
      }
    } else {
      changedSinceViewedPaths.add(file.path);
    }
  }

  // Filter out stale stored records whose diff hash no longer matches the file in this comparison.
  const validLoadedRecords: ViewedFileRecord[] = [];
  for (const record of loadedRecords) {
    const currentHash = hashesByPath.get(record.filePath);
    if (currentHash && currentHash !== record.diffContentHash) {
      changedSinceViewedPaths.add(record.filePath);
      continue;
    }
    validLoadedRecords.push(record);
  }

  const nextRecords = [...validLoadedRecords, ...additions];
  if (additions.length > 0 || validLoadedRecords.length !== loadedRecords.length) {
    storageService.saveViewedFiles(
      context.baseCommitish,
      context.targetCommitish,
      nextRecords,
      context.currentCommitHash,
      undefined,
      context.repositoryId,
      context.baseMode,
    );
  }

  if (additions.length > 0) {
    const indexEntries: ViewedHashIndexEntry[] = additions.map((record) => ({
      filePath: record.filePath,
      diffContentHash: record.diffContentHash,
      hashVersion: VIEWED_HASH_VERSION,
      viewedAt: record.viewedAt,
    }));
    storageService.recordViewedHashes(context.repositoryId, indexEntries);
  }

  return {
    records: nextRecords,
    viewedPaths: new Set(nextRecords.map((record) => record.filePath)),
    changedSinceViewedPaths,
    hashesByPath,
  };
}

export async function toggleSingleFileViewed(
  context: ViewedContextParams,
  currentRecords: ViewedFileRecord[],
  file: DiffFile,
  cachedHash?: string,
): Promise<{ records: ViewedFileRecord[]; isViewed: boolean; hash: string }> {
  const existing = currentRecords.find((record) => record.filePath === file.path);
  const hash = cachedHash ?? (await generateDiffHash(getDiffContentForHashing(file)));

  if (existing) {
    const nextRecords = currentRecords.filter((record) => record.filePath !== file.path);
    storageService.saveViewedFiles(
      context.baseCommitish,
      context.targetCommitish,
      nextRecords,
      context.currentCommitHash,
      undefined,
      context.repositoryId,
      context.baseMode,
    );
    storageService.removeViewedHashes(context.repositoryId, [
      { filePath: file.path, diffContentHash: existing.diffContentHash },
    ]);
    return { records: nextRecords, isViewed: false, hash };
  }

  const viewedAt = new Date().toISOString();
  const addedRecord: ViewedFileRecord = {
    filePath: file.path,
    viewedAt,
    diffContentHash: hash,
  };
  const nextRecords = [...currentRecords, addedRecord];
  storageService.saveViewedFiles(
    context.baseCommitish,
    context.targetCommitish,
    nextRecords,
    context.currentCommitHash,
    undefined,
    context.repositoryId,
    context.baseMode,
  );
  storageService.recordViewedHashes(context.repositoryId, [
    {
      filePath: file.path,
      diffContentHash: hash,
      hashVersion: VIEWED_HASH_VERSION,
      viewedAt,
    },
  ]);
  return { records: nextRecords, isViewed: true, hash };
}

export async function setMultipleFilesViewed(
  context: ViewedContextParams,
  currentRecords: ViewedFileRecord[],
  targetFiles: DiffFile[],
  viewed: boolean,
  hashesByPath?: Map<string, string>,
): Promise<ViewedFileRecord[]> {
  if (viewed) {
    const existingPaths = new Set(currentRecords.map((record) => record.filePath));
    const filesToAdd = targetFiles.filter((file) => !existingPaths.has(file.path));
    if (filesToAdd.length === 0) return currentRecords;

    const viewedAt = new Date().toISOString();
    const addedRecords: ViewedFileRecord[] = await Promise.all(
      filesToAdd.map(async (file) => ({
        filePath: file.path,
        viewedAt,
        diffContentHash:
          hashesByPath?.get(file.path) ?? (await generateDiffHash(getDiffContentForHashing(file))),
      })),
    );
    const nextRecords = [...currentRecords, ...addedRecords];
    storageService.saveViewedFiles(
      context.baseCommitish,
      context.targetCommitish,
      nextRecords,
      context.currentCommitHash,
      undefined,
      context.repositoryId,
      context.baseMode,
    );
    storageService.recordViewedHashes(
      context.repositoryId,
      addedRecords.map((record) => ({
        filePath: record.filePath,
        diffContentHash: record.diffContentHash,
        hashVersion: VIEWED_HASH_VERSION,
        viewedAt: record.viewedAt,
      })),
    );
    return nextRecords;
  }

  const removePaths = new Set(targetFiles.map((file) => file.path));
  const removedRecords = currentRecords.filter((record) => removePaths.has(record.filePath));
  if (removedRecords.length === 0) return currentRecords;

  const nextRecords = currentRecords.filter((record) => !removePaths.has(record.filePath));
  storageService.saveViewedFiles(
    context.baseCommitish,
    context.targetCommitish,
    nextRecords,
    context.currentCommitHash,
    undefined,
    context.repositoryId,
    context.baseMode,
  );
  storageService.removeViewedHashes(
    context.repositoryId,
    removedRecords.map((record) => ({
      filePath: record.filePath,
      diffContentHash: record.diffContentHash,
    })),
  );
  return nextRecords;
}
