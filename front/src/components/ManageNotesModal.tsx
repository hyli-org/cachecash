import { ChangeEvent, DragEvent, useCallback, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { StoredNote, PrivateNote } from "../types/note";
import { createNotesArchive, readNotesArchive } from "../services/noteArchive";
import { setStoredNotes, addStoredNote } from "../services/noteStorage";
import { FullIdentity } from "../services/KeyService";

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
        if (seen.has(key)) return false;
        seen.add(key);
        return true;
    });
    return [...uniqueIncoming, ...existing].sort((a, b) => b.storedAt - a.storedAt);
}

function parseValue(note: PrivateNote): number {
    const hex = (note.value ?? "").replace(/^0x/i, "");
    if (!hex || hex === "0".repeat(64)) return 0;
    const n = parseInt(hex, 16);
    return isNaN(n) ? 0 : n;
}

function shortHex(hex: string, chars = 8): string {
    const h = hex.replace(/^0x/i, "");
    if (h.length <= chars * 2) return h;
    return `${h.slice(0, chars)}…${h.slice(-4)}`;
}

interface ManageNotesModalProps {
    playerName: string;
    notes: StoredNote[];
    identity?: FullIdentity | null;
    onClose: () => void;
}

export function ManageNotesModal({ playerName, notes, identity, onClose }: ManageNotesModalProps) {
    const [error, setError]           = useState<string | null>(null);
    const [status, setStatus]         = useState<string | null>(null);
    const [isDownloading, setIsDownloading] = useState(false);
    const [isUploading, setIsUploading]     = useState(false);
    const [isDragOver, setIsDragOver]       = useState(false);
    const [addressCopied, setAddressCopied] = useState(false);
    const [pasteJson, setPasteJson]         = useState("");
    const [pasteError, setPasteError]       = useState<string | null>(null);
    const [pasteStatus, setPasteStatus]     = useState<string | null>(null);
    const fileInputRef = useRef<HTMLInputElement | null>(null);

    // Parsed notes for display (exclude zero-value / optimistic)
    const displayNotes = useMemo(
        () =>
            notes
                .map((stored) => {
                    const note = stored.note as PrivateNote & { status?: string };
                    const value = parseValue(note);
                    return { stored, note, value };
                })
                .filter(({ value }) => value > 0)
                .sort((a, b) => b.stored.storedAt - a.stored.storedAt),
        [notes],
    );

    const noteCountLabel = useMemo(() => {
        if (notes.length === 0) return "No stored notes";
        if (notes.length === 1) return "1 stored note";
        return `${notes.length} stored notes`;
    }, [notes.length]);

    const handleCopyAddress = useCallback(() => {
        if (!identity?.utxoAddress) return;
        navigator.clipboard.writeText(identity.utxoAddress).then(() => {
            setAddressCopied(true);
            setTimeout(() => setAddressCopied(false), 2500);
        });
    }, [identity?.utxoAddress]);

    const handlePasteImport = useCallback(() => {
        setPasteError(null);
        setPasteStatus(null);
        const trimmed = pasteJson.trim();
        if (!trimmed) {
            setPasteError("Paste a JSON note first.");
            return;
        }
        let parsed: unknown;
        try {
            parsed = JSON.parse(trimmed);
        } catch {
            setPasteError("Invalid JSON – could not parse.");
            return;
        }
        if (typeof parsed !== "object" || parsed === null) {
            setPasteError("Expected a JSON object.");
            return;
        }
        const obj = parsed as Record<string, unknown>;
        const note = obj.note as PrivateNote | undefined;
        const txHash = typeof obj.txHash === "string" ? obj.txHash : "";
        if (!note || typeof note !== "object") {
            setPasteError('Missing "note" field.');
            return;
        }
        if (!note.psi || !note.value || !note.address || !note.contract) {
            setPasteError("Note is missing required fields (psi, value, address, contract).");
            return;
        }
        const entry: StoredNote = {
            txHash: txHash || `paste:${note.psi}`,
            note,
            storedAt: Date.now(),
            player: playerName,
        };
        addStoredNote(playerName, entry);
        setPasteJson("");
        setPasteStatus("Note imported successfully.");
    }, [pasteJson, playerName]);

    const handleDownload = useCallback(async () => {
        if (notes.length === 0 || isDownloading) return;
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
            if (isUploading) return;
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
                    setError(
                        `Archive belongs to ${data.player}. Switch player to import or download new notes.`,
                    );
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
        [isUploading, playerName, notes],
    );

    const handleUpload = useCallback(
        async (event: ChangeEvent<HTMLInputElement>) => {
            const [file] = event.target.files ?? [];
            event.target.value = "";
            if (!file) return;
            await processArchive(file);
        },
        [processArchive],
    );

    const handleDragEnter = useCallback(
        (event: DragEvent<HTMLElement>) => {
            event.preventDefault();
            event.stopPropagation();
            if (event.dataTransfer) event.dataTransfer.dropEffect = "copy";
            if (!isUploading) setIsDragOver(true);
        },
        [isUploading],
    );

    const handleDragOver = useCallback(
        (event: DragEvent<HTMLElement>) => {
            event.preventDefault();
            event.stopPropagation();
            if (event.dataTransfer) event.dataTransfer.dropEffect = "copy";
            if (!isUploading) setIsDragOver(true);
        },
        [isUploading],
    );

    const handleDragLeave = useCallback((event: DragEvent<HTMLElement>) => {
        event.preventDefault();
        event.stopPropagation();
        const nextTarget = event.relatedTarget as Node | null;
        if (nextTarget && event.currentTarget.contains(nextTarget)) return;
        setIsDragOver(false);
    }, []);

    const handleDrop = useCallback(
        async (event: DragEvent<HTMLElement>) => {
            event.preventDefault();
            event.stopPropagation();
            setIsDragOver(false);
            if (isUploading) return;
            const files = event.dataTransfer?.files;
            if (!files || files.length === 0) return;
            const [file] = Array.from(files);
            if (!file) return;
            await processArchive(file);
        },
        [isUploading, processArchive],
    );

    const modalContent = (
        <div className="manage-notes-modal__backdrop" role="presentation">
            <div
                className="manage-notes-modal"
                role="dialog"
                aria-modal="true"
                aria-labelledby="manage-notes-title"
            >
                <div className="manage-notes-modal__header">
                    <div className="manage-notes-modal__eyebrow">Settings</div>
                    <h2 id="manage-notes-title" className="manage-notes-modal__title">
                        {playerName || "---"}
                    </h2>
                </div>

                <div className="manage-notes-modal__body">

                    {/* ── UTXO Address ── */}
                    {identity?.utxoAddress && (
                        <section style={{ marginBottom: "1.25rem" }}>
                            <div
                                style={{
                                    fontWeight: "bold",
                                    fontSize: "0.8rem",
                                    textTransform: "uppercase",
                                    letterSpacing: "0.05em",
                                    marginBottom: "0.4rem",
                                }}
                            >
                                Your UTXO address
                            </div>
                            <div
                                style={{
                                    display: "flex",
                                    alignItems: "center",
                                    gap: "0.5rem",
                                }}
                            >
                                <code
                                    style={{
                                        fontFamily: "monospace",
                                        fontSize: "0.72rem",
                                        wordBreak: "break-all",
                                        flex: 1,
                                    }}
                                    title={identity.utxoAddress}
                                >
                                    {identity.utxoAddress}
                                </code>
                                <button
                                    type="button"
                                    className="pixel-button pixel-button--ghost pixel-button--compact"
                                    onClick={handleCopyAddress}
                                    style={{ flexShrink: 0 }}
                                >
                                    {addressCopied ? "Copied!" : "Copy"}
                                </button>
                            </div>
                            <div
                                style={{
                                    fontSize: "0.72rem",
                                    color: "#666",
                                    marginTop: "0.25rem",
                                }}
                            >
                                Share this address so others can send you notes directly.
                            </div>
                        </section>
                    )}

                    {/* ── Notes list ── */}
                    <section style={{ marginBottom: "1.25rem" }}>
                        <div
                            style={{
                                fontWeight: "bold",
                                fontSize: "0.8rem",
                                textTransform: "uppercase",
                                letterSpacing: "0.05em",
                                marginBottom: "0.4rem",
                            }}
                        >
                            Notes ({displayNotes.length})
                        </div>
                        {displayNotes.length === 0 ? (
                            <div style={{ fontSize: "0.85rem", color: "#666" }}>
                                No spendable notes yet. Slice some pumpkins!
                            </div>
                        ) : (
                            <ul
                                style={{
                                    listStyle: "none",
                                    margin: 0,
                                    padding: 0,
                                    maxHeight: "180px",
                                    overflowY: "auto",
                                    border: "1px solid #ccc",
                                }}
                            >
                                {displayNotes.map(({ stored, note, value }) => (
                                    <li
                                        key={createNoteKey(stored)}
                                        style={{
                                            display: "flex",
                                            justifyContent: "space-between",
                                            alignItems: "center",
                                            padding: "0.35rem 0.5rem",
                                            borderBottom: "1px solid #eee",
                                            fontSize: "0.8rem",
                                            gap: "0.5rem",
                                        }}
                                    >
                                        <span style={{ fontWeight: "bold", flexShrink: 0 }}>
                                            {value.toLocaleString()}
                                        </span>
                                        <span
                                            style={{
                                                fontFamily: "monospace",
                                                color: "#555",
                                                fontSize: "0.7rem",
                                                flex: 1,
                                                overflow: "hidden",
                                                textOverflow: "ellipsis",
                                                whiteSpace: "nowrap",
                                            }}
                                            title={note.psi}
                                        >
                                            psi:{shortHex(note.psi)}
                                        </span>
                                        <span style={{ color: "#888", flexShrink: 0, fontSize: "0.7rem" }}>
                                            {new Date(stored.storedAt).toLocaleDateString()}
                                        </span>
                                    </li>
                                ))}
                            </ul>
                        )}
                    </section>

                    {/* ── Paste note ── */}
                    <section style={{ marginBottom: "1.25rem" }}>
                        <div
                            style={{
                                fontWeight: "bold",
                                fontSize: "0.8rem",
                                textTransform: "uppercase",
                                letterSpacing: "0.05em",
                                marginBottom: "0.4rem",
                            }}
                        >
                            Import note
                        </div>
                        <p className="manage-notes-modal__description">
                            Paste a note JSON received from another player.
                        </p>
                        <textarea
                            value={pasteJson}
                            onChange={(e) => { setPasteJson(e.target.value); setPasteError(null); setPasteStatus(null); }}
                            placeholder={'{\n  "txHash": "...",\n  "note": { ... }\n}'}
                            rows={5}
                            style={{
                                width: "100%",
                                fontFamily: "monospace",
                                fontSize: "0.72rem",
                                padding: "0.4rem",
                                boxSizing: "border-box",
                                resize: "vertical",
                            }}
                        />
                        <button
                            type="button"
                            className="pixel-button pixel-button--ghost"
                            onClick={handlePasteImport}
                            style={{ marginTop: "0.4rem" }}
                        >
                            Import
                        </button>
                        {pasteError && (
                            <div className="manage-notes-modal__message manage-notes-modal__message--error">
                                {pasteError}
                            </div>
                        )}
                        {pasteStatus && (
                            <div className="manage-notes-modal__message manage-notes-modal__message--status">
                                {pasteStatus}
                            </div>
                        )}
                    </section>

                    {/* ── Archive ── */}
                    <section>
                        <div
                            style={{
                                fontWeight: "bold",
                                fontSize: "0.8rem",
                                textTransform: "uppercase",
                                letterSpacing: "0.05em",
                                marginBottom: "0.4rem",
                            }}
                        >
                            Backup
                        </div>
                        <p className="manage-notes-modal__description">
                            Download a backup or upload a saved archive of your notes.
                        </p>
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
                            <div className="manage-notes-modal__upload-hint">
                                Drag a .zip onto Upload or choose a file
                            </div>
                        </div>
                        {error && (
                            <div className="manage-notes-modal__message manage-notes-modal__message--error">
                                {error}
                            </div>
                        )}
                        {status && (
                            <div className="manage-notes-modal__message manage-notes-modal__message--status">
                                {status}
                            </div>
                        )}
                    </section>
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
