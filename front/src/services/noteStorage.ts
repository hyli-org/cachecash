import { StoredNote } from "../types/note";

export const STORED_NOTES_PREFIX = "storedNotes:" as const;
const LEGACY_STORAGE_KEY = "storedNotes";

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
