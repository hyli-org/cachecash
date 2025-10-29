import { ChangeEvent, DragEvent, useCallback, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { StoredNote } from "../types/note";
import { createNotesArchive, readNotesArchive } from "../services/noteArchive";
import { setStoredNotes } from "../services/noteStorage";

function createNoteKey(note: StoredNote): string {
    if (typeof note.txHash === "string" && note.txHash.length > 0) {
        return note.txHash;
    }
    return `${note.player ?? ""}-${note.storedAt}`;
}

function mergeNotes(existing: StoredNote[], incoming: StoredNote[]): StoredNote[] {
    const seen = new Set(existing.map(createNoteKey));
    const uniqueIncoming = incoming.filter((note) => {
        const key = createNoteKey(note);
        if (seen.has(key)) {
            return false;
        }
        seen.add(key);
        return true;
    });

    const merged = [...uniqueIncoming, ...existing];
    return merged.sort((a, b) => b.storedAt - a.storedAt);
}

interface ManageNotesModalProps {
    playerName: string;
    notes: StoredNote[];
    onClose: () => void;
}

export function ManageNotesModal({ playerName, notes, onClose }: ManageNotesModalProps) {
    const [error, setError] = useState<string | null>(null);
    const [status, setStatus] = useState<string | null>(null);
    const [isDownloading, setIsDownloading] = useState(false);
    const [isUploading, setIsUploading] = useState(false);
    const [isDragOver, setIsDragOver] = useState(false);
    const fileInputRef = useRef<HTMLInputElement | null>(null);

    const noteCountLabel = useMemo(() => {
        if (notes.length === 0) {
            return "No stored notes";
        }
        if (notes.length === 1) {
            return "1 stored note";
        }
        return `${notes.length} stored notes`;
    }, [notes.length]);

    const handleDownload = useCallback(async () => {
        if (notes.length === 0 || isDownloading) {
            return;
        }

        setError(null);
        setStatus(null);
        setIsDownloading(true);
        try {
            const archive = await createNotesArchive(playerName, notes);
            const url = URL.createObjectURL(archive.blob);
            const anchor = document.createElement("a");
            anchor.href = url;
            anchor.download = archive.filename;
            document.body.appendChild(anchor);
            anchor.click();
            document.body.removeChild(anchor);
            URL.revokeObjectURL(url);
        } catch (downloadError) {
            console.error("Failed to download notes archive", downloadError);
            setError("We could not prepare your archive. Please try again.");
        } finally {
            setIsDownloading(false);
        }
    }, [isDownloading, notes, playerName]);

    const handleUploadClick = useCallback(() => {
        fileInputRef.current?.click();
    }, []);

    const processArchive = useCallback(
        async (file: File) => {
            if (isUploading) {
                return;
            }

            const filename = file.name?.toLowerCase() ?? "";
            if (!filename.endsWith(".zip")) {
                setError("Upload a .zip archive containing notes.json");
                setStatus(null);
                return;
            }

            setError(null);
            setStatus(null);
            setIsUploading(true);
            try {
                const result = await readNotesArchive(file);
                if (!result.ok) {
                    setError(result.error);
                    return;
                }

                const { data } = result;
                const normalizedArchivePlayer = data.player?.trim().toLowerCase();
                const normalizedCurrentPlayer = playerName.trim().toLowerCase();
                if (normalizedArchivePlayer && normalizedArchivePlayer !== normalizedCurrentPlayer) {
                    setError(`Archive belongs to ${data.player}. Switch player to import or download new notes.`);
                    return;
                }

                const mergedNotes = mergeNotes(notes, data.notes);
                setStoredNotes(playerName, mergedNotes);
                setStatus(`_uploaded notes for ${playerName}_`);
            } catch (uploadError) {
                console.error("Failed to upload notes archive", uploadError);
                setError("We could not read that archive. Please try a different file.");
            } finally {
                setIsUploading(false);
            }
        },
        [isUploading, playerName],
    );

    const handleUpload = useCallback(
        async (event: ChangeEvent<HTMLInputElement>) => {
            const [file] = event.target.files ?? [];
            event.target.value = "";
            if (!file) {
                return;
            }

            await processArchive(file);
        },
        [processArchive],
    );

    const handleDragEnter = useCallback(
        (event: DragEvent<HTMLElement>) => {
            event.preventDefault();
            event.stopPropagation();
            if (event.dataTransfer) {
                event.dataTransfer.dropEffect = "copy";
            }
            if (!isUploading) {
                setIsDragOver(true);
            }
        },
        [isUploading],
    );

    const handleDragOver = useCallback((event: DragEvent<HTMLElement>) => {
        event.preventDefault();
        event.stopPropagation();
        if (event.dataTransfer) {
            event.dataTransfer.dropEffect = "copy";
        }
        if (!isUploading) {
            setIsDragOver(true);
        }
    }, [isUploading]);

    const handleDragLeave = useCallback((event: DragEvent<HTMLElement>) => {
        event.preventDefault();
        event.stopPropagation();
        const nextTarget = event.relatedTarget as Node | null;
        if (nextTarget && event.currentTarget.contains(nextTarget)) {
            return;
        }
        setIsDragOver(false);
    }, []);

    const handleDrop = useCallback(
        async (event: DragEvent<HTMLElement>) => {
            event.preventDefault();
            event.stopPropagation();
            setIsDragOver(false);
            if (isUploading) {
                return;
            }

            const files = event.dataTransfer?.files;
            if (!files || files.length === 0) {
                return;
            }

            const [file] = Array.from(files);
            if (!file) {
                return;
            }

            await processArchive(file);
        },
        [isUploading, processArchive],
    );

    const modalContent = (
        <div className="manage-notes-modal__backdrop" role="presentation">
            <div className="manage-notes-modal" role="dialog" aria-modal="true" aria-labelledby="manage-notes-title">
                <div className="manage-notes-modal__header">
                    <div className="manage-notes-modal__eyebrow">Manage Notes</div>
                    <h2 id="manage-notes-title" className="manage-notes-modal__title">
                        {playerName || "---"}
                    </h2>
                </div>
                <div className="manage-notes-modal__body">
                    <p className="manage-notes-modal__description">Download a backup or upload a saved archive of your notes.</p>
                    <div className="manage-notes-modal__count">{noteCountLabel}</div>
                    <div
                        className={`manage-notes-modal__actions manage-notes-modal__upload${
                            isDragOver ? " is-active" : ""
                        }`}
                        onDragEnter={handleDragEnter}
                        onDragOver={handleDragOver}
                        onDragLeave={handleDragLeave}
                        onDrop={handleDrop}
                    >
                        <button
                            type="button"
                            className="pixel-button pixel-button--ghost"
                            onClick={handleDownload}
                            disabled={notes.length === 0 || isDownloading}
                        >
                            {isDownloading ? "Preparing…" : "Download archive"}
                        </button>
                        <button
                            type="button"
                            className={`pixel-button pixel-button--ghost manage-notes-modal__upload-button${
                                isDragOver ? " is-drag-over" : ""
                            }`}
                            onClick={handleUploadClick}
                            disabled={isUploading}
                        >
                            {isUploading ? "Uploading…" : "Upload archive"}
                        </button>
                        <input
                            ref={fileInputRef}
                            className="manage-notes-modal__file-input"
                            type="file"
                            accept=".zip"
                            onChange={handleUpload}
                        />
                        <div className="manage-notes-modal__upload-hint">Drag a .zip onto Upload or choose a file</div>
                    </div>
                    {error && <div className="manage-notes-modal__message manage-notes-modal__message--error">{error}</div>}
                    {status && <div className="manage-notes-modal__message manage-notes-modal__message--status">{status}</div>}
                </div>
                <div className="manage-notes-modal__footer">
                    <button type="button" className="pixel-button pixel-button--ghost" onClick={onClose}>
                        Close
                    </button>
                </div>
            </div>
        </div>
    );

    if (typeof document === "undefined") {
        return modalContent;
    }

    return createPortal(modalContent, document.body);
}
