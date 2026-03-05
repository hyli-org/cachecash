import { ChangeEvent, DragEvent, useCallback, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { StoredNote, PrivateNote } from "../types/note";
import { createNotesArchive, readNotesArchive } from "../services/noteArchive";
import { setStoredNotes, addStoredNote } from "../services/noteStorage";
import { FullIdentity } from "../services/KeyService";
import { transferService } from "../services/TransferService";

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
    const [isConsolidating, setIsConsolidating]       = useState(false);
    const [consolidateStep, setConsolidateStep]       = useState(0);
    const [consolidateTotal, setConsolidateTotal]     = useState(0);
    const [consolidateError, setConsolidateError]     = useState<string | null>(null);
    const [consolidateStatus, setConsolidateStatus]   = useState<string | null>(null);
    const fileInputRef = useRef<HTMLInputElement | null>(null);

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

    const spendableCount = useMemo(() => {
        if (!identity) return 0;
        return transferService.getSpendableNotes(notes, identity.zkSecretKey, playerName).length;
    }, [notes, identity, playerName]);

    const handleConsolidate = useCallback(async () => {
        if (!identity || isConsolidating) return;
        const inputs = transferService.getSpendableNotes(notes, identity.zkSecretKey, playerName);
        if (inputs.length < 2) return;

        setConsolidateError(null);
        setConsolidateStatus(null);
        setConsolidateStep(0);
        setConsolidateTotal(Math.max(0, inputs.length - 1));
        setIsConsolidating(true);

        try {
            const rounds = await transferService.consolidateAll(
                inputs,
                identity,
                playerName,
                (step, total) => {
                    setConsolidateStep(step);
                    setConsolidateTotal(total);
                },
            );
            setConsolidateStatus(
                rounds === 0 ? "Already consolidated." : `Done — ${rounds} proof${rounds > 1 ? "s" : ""} generated.`,
            );
        } catch (err) {
            console.error("Consolidation failed:", err);
            setConsolidateError(err instanceof Error ? err.message : "Consolidation failed.");
        } finally {
            setIsConsolidating(false);
        }
    }, [identity, isConsolidating, notes, playerName]);

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
                setStatus(`Uploaded notes for ${playerName}.`);
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
        <div className="modal-backdrop" role="presentation">
            <div
                className="modal"
                role="dialog"
                aria-modal="true"
                aria-labelledby="manage-notes-title"
            >
                <div className="modal-header">
                    <div className="modal-eyebrow">Settings</div>
                    <h2 id="manage-notes-title" className="modal-title">
                        {playerName || "---"}
                    </h2>
                </div>

                <div className="modal-body">
                    {/* ── UTXO Address ── */}
                    {identity?.utxoAddress && (
                        <div className="modal-section">
                            <div className="modal-section-title">Your Address</div>
                            <p className="modal-section-desc">
                                Share this address so others can send you notes directly.
                            </p>
                            <div style={{ display: "flex", alignItems: "center", gap: "0.5rem" }}>
                                <code className="mono" style={{ flex: 1 }} title={identity.utxoAddress}>
                                    {identity.utxoAddress}
                                </code>
                                <button
                                    type="button"
                                    className="btn btn-ghost btn-sm"
                                    onClick={handleCopyAddress}
                                    style={{ flexShrink: 0 }}
                                >
                                    {addressCopied ? "Copied!" : "Copy"}
                                </button>
                            </div>
                        </div>
                    )}

                    {/* ── Notes list ── */}
                    <div className="modal-section">
                        <div className="modal-section-title">Notes ({displayNotes.length})</div>
                        {displayNotes.length === 0 ? (
                            <p className="modal-section-desc">No spendable notes yet.</p>
                        ) : (
                            <ul className="modal-notes-list">
                                {displayNotes.map(({ stored, note, value }) => (
                                    <li key={createNoteKey(stored)} className="modal-note-item">
                                        <span className="modal-note-value">{value.toLocaleString()}</span>
                                        <span className="modal-note-hash" title={note.psi}>
                                            psi:{shortHex(note.psi)}
                                        </span>
                                        <span className="modal-note-date">
                                            {new Date(stored.storedAt).toLocaleDateString()}
                                        </span>
                                    </li>
                                ))}
                            </ul>
                        )}
                        {identity && spendableCount >= 2 && (
                            <div style={{ display: "flex", flexDirection: "column", gap: "0.5rem" }}>
                                <p className="modal-section-desc">
                                    Merge all notes into one for faster future transfers.
                                </p>
                                <button
                                    type="button"
                                    className="btn btn-secondary"
                                    onClick={handleConsolidate}
                                    disabled={isConsolidating}
                                    style={{ alignSelf: "flex-start" }}
                                >
                                    {isConsolidating
                                        ? `Consolidating… (${consolidateStep}/${consolidateTotal})`
                                        : `Consolidate (${spendableCount} → 1)`}
                                </button>
                                {consolidateError && (
                                    <div className="status-error">{consolidateError}</div>
                                )}
                                {consolidateStatus && (
                                    <div className="status-success">{consolidateStatus}</div>
                                )}
                            </div>
                        )}
                    </div>

                    {/* ── Import note ── */}
                    <div className="modal-section">
                        <div className="modal-section-title">Import Note</div>
                        <p className="modal-section-desc">
                            Paste a note JSON received from another user. The sender must share this if
                            no encryption key is registered.
                        </p>
                        <textarea
                            className="form-input"
                            value={pasteJson}
                            onChange={(e) => {
                                setPasteJson(e.target.value);
                                setPasteError(null);
                                setPasteStatus(null);
                            }}
                            placeholder={'{\n  "txHash": "...",\n  "note": { ... }\n}'}
                            rows={5}
                        />
                        <button
                            type="button"
                            className="btn btn-secondary"
                            onClick={handlePasteImport}
                            style={{ alignSelf: "flex-start" }}
                        >
                            Import
                        </button>
                        {pasteError && <div className="status-error">{pasteError}</div>}
                        {pasteStatus && <div className="status-success">{pasteStatus}</div>}
                    </div>

                    {/* ── Backup ── */}
                    <div className="modal-section">
                        <div className="modal-section-title">Backup</div>
                        <p className="modal-section-desc">
                            Download a backup archive to keep your notes safe if you clear browser data.
                            You can also restore from a previously saved archive.
                        </p>
                        <p className="form-hint">{noteCountLabel}</p>

                        <div
                            className={`upload-area${isDragOver ? " is-drag-over" : ""}`}
                            onDragEnter={handleDragEnter}
                            onDragOver={handleDragOver}
                            onDragLeave={handleDragLeave}
                            onDrop={handleDrop}
                        >
                            <div className="upload-actions">
                                <button
                                    type="button"
                                    className="btn btn-secondary"
                                    onClick={handleDownload}
                                    disabled={notes.length === 0 || isDownloading}
                                >
                                    {isDownloading ? "Preparing…" : "Download archive"}
                                </button>
                                <button
                                    type="button"
                                    className="btn btn-secondary"
                                    onClick={handleUploadClick}
                                    disabled={isUploading}
                                >
                                    {isUploading ? "Uploading…" : "Upload archive"}
                                </button>
                            </div>
                            <div className="upload-area-hint">Drag a .zip onto this area or choose a file</div>
                            <input
                                ref={fileInputRef}
                                className="file-input-hidden"
                                type="file"
                                accept=".zip"
                                onChange={handleUpload}
                            />
                        </div>

                        {error && <div className="status-error">{error}</div>}
                        {status && <div className="status-success">{status}</div>}
                    </div>
                </div>

                <div className="modal-footer">
                    <span className="modal-footer-note">Your keys stay local — nothing is sent to a server.</span>
                    <button type="button" className="btn btn-ghost btn-sm" onClick={onClose}>
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
