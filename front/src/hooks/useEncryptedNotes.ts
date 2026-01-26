import { useCallback, useEffect, useRef, useState } from "react";
import { DerivedKeyPair } from "../services/KeyService";
import { encryptedNoteService, DecryptedNoteRecord } from "../services/EncryptedNoteService";
import { addStoredNote } from "../services/noteStorage";
import { StoredNote } from "../types/note";

const POLLING_INTERVAL_MS = 30000; // 30 seconds
const LAST_FETCH_KEY_PREFIX = "encryptedNotes:lastFetch:";

interface UseEncryptedNotesOptions {
  enabled?: boolean;
  pollingInterval?: number;
  onNotesReceived?: (notes: DecryptedNoteRecord[]) => void;
  onError?: (error: Error) => void;
}

interface UseEncryptedNotesResult {
  isPolling: boolean;
  lastFetch: number | null;
  error: Error | null;
  fetchNow: () => Promise<void>;
  receivedCount: number;
}

function getLastFetchKey(publicKey: string): string {
  return `${LAST_FETCH_KEY_PREFIX}${publicKey.toLowerCase()}`;
}

function loadLastFetch(publicKey: string): number | null {
  if (typeof window === "undefined" || !window.localStorage) {
    return null;
  }

  const key = getLastFetchKey(publicKey);
  const value = window.localStorage.getItem(key);
  if (!value) {
    return null;
  }

  const parsed = parseInt(value, 10);
  return isNaN(parsed) ? null : parsed;
}

function saveLastFetch(publicKey: string, timestamp: number): void {
  if (typeof window === "undefined" || !window.localStorage) {
    return;
  }

  const key = getLastFetchKey(publicKey);
  window.localStorage.setItem(key, timestamp.toString());
}

/**
 * Hook for polling encrypted notes from the server.
 *
 * Features:
 * - Polls every 30 seconds (configurable)
 * - Decrypts notes using the provided keypair
 * - Stores decrypted notes in localStorage via noteStorage service
 * - Tracks last fetch timestamp for incremental fetching
 * - Deletes notes from server after successful decryption and storage
 *
 * @param keyPair - The user's keypair for decryption
 * @param playerName - The player name for storing notes
 * @param options - Configuration options
 */
export function useEncryptedNotes(
  keyPair: DerivedKeyPair | null,
  playerName: string | null,
  options: UseEncryptedNotesOptions = {}
): UseEncryptedNotesResult {
  const {
    enabled = true,
    pollingInterval = POLLING_INTERVAL_MS,
    onNotesReceived,
    onError,
  } = options;

  const [isPolling, setIsPolling] = useState(false);
  const [lastFetch, setLastFetch] = useState<number | null>(() =>
    keyPair ? loadLastFetch(keyPair.publicKey) : null
  );
  const [error, setError] = useState<Error | null>(null);
  const [receivedCount, setReceivedCount] = useState(0);

  const isMounted = useRef(true);
  const pollingRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Update lastFetch when keyPair changes
  useEffect(() => {
    if (keyPair) {
      setLastFetch(loadLastFetch(keyPair.publicKey));
    } else {
      setLastFetch(null);
    }
  }, [keyPair?.publicKey]);

  const fetchNotes = useCallback(async () => {
    if (!keyPair || !playerName) {
      return;
    }

    setIsPolling(true);
    setError(null);

    try {
      // Fetch notes since last fetch timestamp
      const since = lastFetch ?? undefined;
      const { notes, failedCount } = await encryptedNoteService.processNotes(keyPair, since);

      if (!isMounted.current) {
        return;
      }

      if (notes.length > 0) {
        // Store each decrypted note in localStorage
        for (const note of notes) {
          const storedNote: StoredNote = {
            txHash: `encrypted:${note.id}`,
            note: note.noteData,
            storedAt: note.storedAt * 1000, // Convert to milliseconds
            player: playerName,
          };
          addStoredNote(playerName, storedNote);
        }

        setReceivedCount((prev) => prev + notes.length);

        if (onNotesReceived) {
          onNotesReceived(notes);
        }
      }

      // Update last fetch timestamp
      const newLastFetch = Math.floor(Date.now() / 1000);
      setLastFetch(newLastFetch);
      saveLastFetch(keyPair.publicKey, newLastFetch);

      if (failedCount > 0) {
        console.warn(`Failed to decrypt ${failedCount} notes`);
      }
    } catch (err) {
      if (!isMounted.current) {
        return;
      }

      const error = err instanceof Error ? err : new Error(String(err));
      setError(error);
      console.error("Failed to fetch encrypted notes:", error);

      if (onError) {
        onError(error);
      }
    } finally {
      if (isMounted.current) {
        setIsPolling(false);
      }
    }
  }, [keyPair, playerName, lastFetch, onNotesReceived, onError]);

  // Set up polling
  useEffect(() => {
    isMounted.current = true;

    if (!enabled || !keyPair || !playerName) {
      return;
    }

    // Initial fetch
    fetchNotes();

    // Set up interval for polling
    const scheduleNextPoll = () => {
      pollingRef.current = setTimeout(() => {
        fetchNotes().finally(() => {
          if (isMounted.current && enabled) {
            scheduleNextPoll();
          }
        });
      }, pollingInterval);
    };

    scheduleNextPoll();

    return () => {
      isMounted.current = false;
      if (pollingRef.current) {
        clearTimeout(pollingRef.current);
        pollingRef.current = null;
      }
    };
  }, [enabled, keyPair?.publicKey, playerName, pollingInterval]);

  const fetchNow = useCallback(async () => {
    if (!isPolling) {
      await fetchNotes();
    }
  }, [fetchNotes, isPolling]);

  return {
    isPolling,
    lastFetch,
    error,
    fetchNow,
    receivedCount,
  };
}
