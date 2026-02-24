import { useCallback, useEffect, useRef, useState } from "react";
import { FullIdentity } from "../services/KeyService";
import { encryptedNoteService, DecryptedNoteRecord, DecryptedNotePayload } from "../services/EncryptedNoteService";
import { addStoredNote } from "../services/noteStorage";
import { StoredNote, PrivateNote } from "../types/note";

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

function getLastFetchKey(utxoAddress: string): string {
    return `${LAST_FETCH_KEY_PREFIX}${utxoAddress.toLowerCase()}`;
}

function loadLastFetch(utxoAddress: string): number | null {
    if (typeof window === "undefined" || !window.localStorage) return null;
    const value = window.localStorage.getItem(getLastFetchKey(utxoAddress));
    if (!value) return null;
    const parsed = parseInt(value, 10);
    return isNaN(parsed) ? null : parsed;
}

function saveLastFetch(utxoAddress: string, timestamp: number): void {
    if (typeof window === "undefined" || !window.localStorage) return;
    window.localStorage.setItem(getLastFetchKey(utxoAddress), timestamp.toString());
}

/**
 * Hook for polling encrypted notes from the server.
 * Uses the identity's UTXO address for tag derivation and private key for decryption.
 */
export function useEncryptedNotes(
    identity: FullIdentity | null,
    playerName: string | null,
    options: UseEncryptedNotesOptions = {}
): UseEncryptedNotesResult {
    const {
        enabled = true,
        pollingInterval = POLLING_INTERVAL_MS,
        onNotesReceived,
        onError,
    } = options;

    const [isPolling, setIsPolling]     = useState(false);
    const [lastFetch, setLastFetch]     = useState<number | null>(() =>
        identity ? loadLastFetch(identity.utxoAddress) : null
    );
    const [error, setError]             = useState<Error | null>(null);
    const [receivedCount, setReceivedCount] = useState(0);

    const isMounted  = useRef(true);
    const pollingRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    // Update lastFetch when identity changes
    useEffect(() => {
        if (identity) {
            setLastFetch(loadLastFetch(identity.utxoAddress));
        } else {
            setLastFetch(null);
        }
    }, [identity?.utxoAddress]);

    const fetchNotes = useCallback(async () => {
        if (!identity || !playerName) return;

        setIsPolling(true);
        setError(null);

        try {
            const since = lastFetch ?? undefined;
            const { notes, failedCount } = await encryptedNoteService.processNotes(
                identity, since
            );

            if (!isMounted.current) return;

            if (notes.length > 0) {
                for (const note of notes) {
                    // Extract the PrivateNote from the decrypted payload
                    const payload = note.noteData as DecryptedNotePayload;
                    const privateNote: PrivateNote = payload?.note ?? (note.noteData as PrivateNote);

                    const storedNote: StoredNote = {
                        txHash:   payload?.txHash ?? `encrypted:${note.id}`,
                        note:     privateNote,
                        storedAt: note.storedAt * 1000,
                        player:   playerName,
                    };
                    addStoredNote(playerName, storedNote);
                }

                setReceivedCount((prev) => prev + notes.length);

                if (onNotesReceived) {
                    onNotesReceived(notes);
                }
            }

            const newLastFetch = Math.floor(Date.now() / 1000);
            setLastFetch(newLastFetch);
            saveLastFetch(identity.utxoAddress, newLastFetch);

            if (failedCount > 0) {
                console.warn(`Failed to decrypt ${failedCount} notes`);
            }
        } catch (err) {
            if (!isMounted.current) return;

            const fetchError = err instanceof Error ? err : new Error(String(err));
            setError(fetchError);
            console.error("Failed to fetch encrypted notes:", fetchError);

            if (onError) {
                onError(fetchError);
            }
        } finally {
            if (isMounted.current) {
                setIsPolling(false);
            }
        }
    }, [identity, playerName, lastFetch, onNotesReceived, onError]);

    // Set up polling
    useEffect(() => {
        isMounted.current = true;

        if (!enabled || !identity || !playerName) return;

        // Initial fetch
        fetchNotes();

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
    }, [enabled, identity?.utxoAddress, playerName, pollingInterval]);

    const fetchNow = useCallback(async () => {
        if (!isPolling) {
            await fetchNotes();
        }
    }, [fetchNotes, isPolling]);

    return { isPolling, lastFetch, error, fetchNow, receivedCount };
}
