import { StoredNote } from "../types/note";

export const STORED_NOTES_PREFIX = "storedNotes:" as const;
const LEGACY_STORAGE_KEY = "storedNotes";
const PENDING_TRANSFERS_PREFIX = "pendingTransfers:" as const;
const PENDING_TRANSFER_TIMEOUT_MS = 5 * 60 * 1000; // 5 minutes

export interface PendingTransfer {
  spentNoteHashes: string[];
  timestamp: number;
}

type Listener = (notes: StoredNote[]) => void;
const listeners = new Map<string, Set<Listener>>();

type ResolvedPlayerKey = {
    playerKey: string;
    storageKey: string;
};

function hasStorage(): boolean {
    return typeof window !== "undefined" && typeof window.localStorage !== "undefined";
}

function normalizePlayerKey(playerName: string | undefined | null): string | undefined {
    const trimmed = playerName?.trim();
    if (!trimmed) {
        return undefined;
    }
    return trimmed.toLowerCase();
}

function resolvePlayer(playerName: string | undefined | null): ResolvedPlayerKey | undefined {
    const playerKey = normalizePlayerKey(playerName);
    if (!playerKey) {
        return undefined;
    }

    return {
        playerKey,
        storageKey: `${STORED_NOTES_PREFIX}${playerKey}`,
    };
}

function parseStoredNotes(raw: string | null): StoredNote[] {
    if (!raw) {
        return [];
    }

    try {
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed)) {
            return parsed as StoredNote[];
        }
    } catch (error) {
        console.warn("Failed to parse stored notes", error);
    }

    return [];
}

function readFromStorageResolved(resolved: ResolvedPlayerKey): StoredNote[] {
    if (!hasStorage()) {
        return [];
    }

    const raw = window.localStorage.getItem(resolved.storageKey);
    if (raw) {
        return parseStoredNotes(raw);
    }

    if (resolved.playerKey === "") {
        return [];
    }

    // Legacy fallback: single bucket without per-player segmentation.
    const legacy = parseStoredNotes(window.localStorage.getItem(LEGACY_STORAGE_KEY));
    if (legacy.length === 0) {
        return [];
    }

    const migrated = legacy.filter((entry) => {
        if (!entry || typeof entry !== "object") {
            return false;
        }
        if (!("player" in entry) || typeof entry.player !== "string") {
            return false;
        }
        return normalizePlayerKey(entry.player) === resolved.playerKey;
    });

    if (migrated.length === 0) {
        return [];
    }

    writeToStorageResolved(resolved, migrated);
    return migrated;
}

function writeToStorageResolved(resolved: ResolvedPlayerKey, next: StoredNote[]): void {
    if (!hasStorage()) {
        return;
    }

    try {
        window.localStorage.setItem(resolved.storageKey, JSON.stringify(next));
    } catch (error) {
        console.warn("Failed to persist stored notes", error);
    }
}

function notify(playerKey: string, snapshot?: StoredNote[]): void {
    const playerListeners = listeners.get(playerKey);
    if (!playerListeners || playerListeners.size === 0) {
        return;
    }

    const notes = snapshot ?? readFromStorageByKey(playerKey);

    playerListeners.forEach((listener) => {
        try {
            listener(notes);
        } catch (error) {
            console.warn("Stored notes listener failed", error);
        }
    });
}

function readFromStorageByKey(playerKey: string): StoredNote[] {
    return readFromStorageResolved({ playerKey, storageKey: `${STORED_NOTES_PREFIX}${playerKey}` });
}

export function addStoredNote(playerName: string, entry: StoredNote): void {
    const resolved = resolvePlayer(playerName);
    if (!resolved) {
        return;
    }

    const existing = readFromStorageResolved(resolved);
    const next = [entry, ...existing];
    writeToStorageResolved(resolved, next);
    notify(resolved.playerKey, next);
}

export function replaceStoredNote(playerName: string, existingTxHash: string, entry: StoredNote): void {
    const resolved = resolvePlayer(playerName);
    if (!resolved) {
        return;
    }

    const existing = readFromStorageResolved(resolved);
    const index = existing.findIndex((note) => note.txHash === existingTxHash);
    if (index === -1) {
        return;
    }

    const next = [...existing];
    next[index] = entry;
    writeToStorageResolved(resolved, next);
    notify(resolved.playerKey, next);
}

export function setStoredNotes(playerName: string | undefined | null, entries: StoredNote[]): void {
    const resolved = resolvePlayer(playerName);
    if (!resolved) {
        return;
    }

    writeToStorageResolved(resolved, entries);
    notify(resolved.playerKey, entries);
}

export function getStoredNotes(playerName: string | undefined | null): StoredNote[] {
    const resolved = resolvePlayer(playerName);
    if (!resolved) {
        return [];
    }

    return readFromStorageResolved(resolved);
}

export function clearStoredNotes(playerName: string | undefined | null): void {
    const resolved = resolvePlayer(playerName);
    if (!resolved) {
        return;
    }

    writeToStorageResolved(resolved, []);
    notify(resolved.playerKey, []);
}

export function subscribeToStoredNotes(
    playerName: string | undefined | null,
    listener: Listener,
): () => void {
    const resolved = resolvePlayer(playerName);
    if (!resolved) {
        listener([]);
        return () => undefined;
    }

    let playerListeners = listeners.get(resolved.playerKey);
    if (!playerListeners) {
        playerListeners = new Set<Listener>();
        listeners.set(resolved.playerKey, playerListeners);
    }

    playerListeners.add(listener);

    return () => {
        const set = listeners.get(resolved.playerKey);
        if (!set) {
            return;
        }
        set.delete(listener);
        if (set.size === 0) {
            listeners.delete(resolved.playerKey);
        }
    };
}

export function isStoredNotesStorageKey(key: string | null | undefined, playerName: string | undefined | null): boolean {
    if (!key) {
        return false;
    }

    const resolved = resolvePlayer(playerName);
    if (!resolved) {
        return false;
    }

    return key === resolved.storageKey;
}

// ---- Pending Transfer Management ----

function getPendingTransfersKey(playerName: string | undefined | null): string | null {
    const playerKey = normalizePlayerKey(playerName);
    if (!playerKey) {
        return null;
    }
    return `${PENDING_TRANSFERS_PREFIX}${playerKey}`;
}

function readPendingTransfers(playerName: string | undefined | null): PendingTransfer[] {
    const key = getPendingTransfersKey(playerName);
    if (!key || !hasStorage()) {
        return [];
    }

    try {
        const raw = window.localStorage.getItem(key);
        if (!raw) {
            return [];
        }
        const parsed = JSON.parse(raw);
        if (Array.isArray(parsed)) {
            // Filter out expired pending transfers
            const now = Date.now();
            const active = parsed.filter(
                (p: PendingTransfer) =>
                    typeof p.timestamp === "number" && now - p.timestamp < PENDING_TRANSFER_TIMEOUT_MS
            );
            return active;
        }
    } catch (error) {
        console.warn("Failed to parse pending transfers", error);
    }

    return [];
}

function writePendingTransfers(playerName: string | undefined | null, pending: PendingTransfer[]): void {
    const key = getPendingTransfersKey(playerName);
    if (!key || !hasStorage()) {
        return;
    }

    try {
        window.localStorage.setItem(key, JSON.stringify(pending));
    } catch (error) {
        console.warn("Failed to persist pending transfers", error);
    }
}

/**
 * Mark notes as pending to prevent double-spend during transfer
 */
export function markNotesPending(playerName: string | undefined | null, noteHashes: string[]): void {
    const pending = readPendingTransfers(playerName);
    pending.push({
        spentNoteHashes: noteHashes,
        timestamp: Date.now(),
    });
    writePendingTransfers(playerName, pending);
}

/**
 * Clear pending state for specific notes (after successful transfer)
 */
export function clearPendingNotes(playerName: string | undefined | null, noteHashes: string[]): void {
    const hashSet = new Set(noteHashes);
    const pending = readPendingTransfers(playerName);
    const updated = pending.filter(
        (p) => !p.spentNoteHashes.some((hash) => hashSet.has(hash))
    );
    writePendingTransfers(playerName, updated);
}

/**
 * Get all pending note hashes (to exclude from spendable notes)
 */
export function getPendingNoteHashes(playerName: string | undefined | null): Set<string> {
    const pending = readPendingTransfers(playerName);
    const hashes = new Set<string>();
    pending.forEach((p) => {
        p.spentNoteHashes.forEach((hash) => hashes.add(hash));
    });
    return hashes;
}

/**
 * Clean up expired pending transfers
 */
export function cleanupExpiredPending(playerName: string | undefined | null): void {
    // This happens automatically in readPendingTransfers
    const pending = readPendingTransfers(playerName);
    writePendingTransfers(playerName, pending);
}
