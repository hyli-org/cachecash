import { useCallback, useEffect, useState } from "react";
import { clearStoredNotes, getStoredNotes, isStoredNotesStorageKey, subscribeToStoredNotes } from "../services/noteStorage";
import { StoredNote } from "../types/note";

interface UseStoredNotesResult {
    notes: StoredNote[];
    clearNotes: () => void;
}

export function useStoredNotes(playerName: string | null | undefined): UseStoredNotesResult {
    const [notes, setNotes] = useState<StoredNote[]>(() => (playerName ? getStoredNotes(playerName) : []));

    useEffect(() => {
        if (!playerName) {
            setNotes([]);
            return;
        }

        setNotes(getStoredNotes(playerName));
    }, [playerName]);

    useEffect(() => {
        if (!playerName) {
            return;
        }

        const unsubscribe = subscribeToStoredNotes(playerName, setNotes);
        const handleStorage = (event: StorageEvent) => {
            if (isStoredNotesStorageKey(event.key, playerName)) {
                setNotes(getStoredNotes(playerName));
            }
        };

        window.addEventListener("storage", handleStorage);

        return () => {
            unsubscribe();
            window.removeEventListener("storage", handleStorage);
        };
    }, [playerName]);

    const clearNotes = useCallback(() => {
        if (!playerName) {
            return;
        }

        clearStoredNotes(playerName);
    }, [playerName]);

    return { notes, clearNotes };
}
