import { useState, useCallback, useMemo, FormEvent } from "react";
import { createPortal } from "react-dom";
import { FullIdentity, deriveFullIdentity } from "../services/KeyService";
import { transferService, InputNoteData, parseNoteValue } from "../services/TransferService";
import { PrivateNote } from "../types/note";

interface TransferModalProps {
    playerName: string;
    identity: FullIdentity | null;
    availableNotes: InputNoteData[];
    onClose: () => void;
}

type TransferStatus = "input" | "submitting" | "success" | "error";

export function TransferModal({ playerName, identity, availableNotes, onClose }: TransferModalProps) {
    const [recipientInput, setRecipientInput] = useState("");
    const [amount, setAmount] = useState("");
    const [status, setStatus] = useState<TransferStatus>("input");
    const [error, setError] = useState<string | null>(null);
    const [txHash, setTxHash] = useState<string | null>(null);
    const [transferNote, setTransferNote] = useState<PrivateNote | null>(null);
    const [noteCopied, setNoteCopied] = useState(false);
    // true when recipient was a direct address (no encryption possible)
    const [noteShareNeeded, setNoteShareNeeded] = useState(false);
    const [consolidationStep, setConsolidationStep] = useState(0);

    const totalBalance = useMemo(
        () => availableNotes.reduce((sum, n) => sum + parseNoteValue(n.note), 0),
        [availableNotes],
    );

    const handleSubmit = useCallback(
        async (event: FormEvent) => {
            event.preventDefault();

            if (!identity || !playerName) {
                setError("Identity not available");
                return;
            }

            const trimmedRecipient = recipientInput.trim();
            if (!trimmedRecipient) {
                setError("Recipient is required");
                return;
            }

            const amountNum = parseInt(amount, 10);
            if (isNaN(amountNum) || amountNum <= 0) {
                setError("Amount must be a positive number");
                return;
            }

            if (amountNum > totalBalance) {
                setError(`Insufficient balance. You have ${totalBalance} but need ${amountNum}`);
                return;
            }

            try {
                setStatus("submitting");
                setConsolidationStep(0);
                setError(null);

                // Resolve recipient: username → deriveFullIdentity, hex → treat as utxoAddress
                let recipientUtxoAddress: string;
                let recipientEncryptionPubkey: string | undefined;

                const normalized = trimmedRecipient.replace(/^0x/i, "");
                if (normalized.length === 64 && /^[0-9a-fA-F]+$/.test(normalized)) {
                    // Direct UTXO address
                    recipientUtxoAddress = normalized;
                    recipientEncryptionPubkey = undefined;
                } else {
                    // Username: derive full identity deterministically
                    const recipientIdentity = await deriveFullIdentity(trimmedRecipient);
                    recipientUtxoAddress = recipientIdentity.utxoAddress;
                    recipientEncryptionPubkey = recipientIdentity.publicKey;
                }

                // Check if sending to self
                if (recipientUtxoAddress.toLowerCase() === identity.utxoAddress.toLowerCase()) {
                    setError("You cannot send funds to yourself");
                    setStatus("input");
                    return;
                }

                const result = await transferService.executeTransferWithConsolidation(
                    recipientUtxoAddress,
                    amountNum,
                    availableNotes,
                    identity,
                    playerName,
                    recipientEncryptionPubkey,
                    (step) => setConsolidationStep(step),
                );

                setStatus("success");
                setTxHash(result.txHash);
                setTransferNote(result.transferNote);
                setNoteShareNeeded(!recipientEncryptionPubkey);
            } catch (err) {
                console.error("Transfer failed:", err);
                setStatus("error");
                setError(err instanceof Error ? err.message : "Transfer failed. Please try again.");
            }
        },
        [recipientInput, amount, identity, playerName, availableNotes, totalBalance],
    );

    const handleClose = useCallback(() => {
        setRecipientInput("");
        setAmount("");
        setStatus("input");
        setError(null);
        setTxHash(null);
        setTransferNote(null);
        setNoteCopied(false);
        setNoteShareNeeded(false);
        setConsolidationStep(0);
        onClose();
    }, [onClose]);

    const handleTryAgain = useCallback(() => {
        setStatus("input");
        setError(null);
        setTxHash(null);
        setTransferNote(null);
        setNoteCopied(false);
        setNoteShareNeeded(false);
        setConsolidationStep(0);
    }, []);

    const handleCopyNote = useCallback(() => {
        if (!transferNote || !txHash) return;
        const payload = JSON.stringify({ txHash, note: transferNote }, null, 2);
        navigator.clipboard.writeText(payload).then(() => {
            setNoteCopied(true);
            setTimeout(() => setNoteCopied(false), 2500);
        });
    }, [transferNote, txHash]);

    const modalContent = (
        <div className="manage-notes-modal__backdrop" role="presentation">
            <div className="manage-notes-modal" role="dialog" aria-modal="true" aria-labelledby="transfer-modal-title">
                <div className="manage-notes-modal__header">
                    <div className="manage-notes-modal__eyebrow">Send Money</div>
                    <h2 id="transfer-modal-title" className="manage-notes-modal__title">
                        {playerName || "---"}
                    </h2>
                </div>

                <div className="manage-notes-modal__body">
                    {status === "input" && (
                        <form onSubmit={handleSubmit}>
                            <div className="manage-notes-modal__description">
                                Transfer funds to another user securely.
                            </div>

                            <div style={{ marginTop: "1rem" }}>
                                <label htmlFor="recipient-input" style={{ display: "block", marginBottom: "0.5rem" }}>
                                    Recipient (Username or UTXO Address)
                                </label>
                                <input
                                    id="recipient-input"
                                    type="text"
                                    value={recipientInput}
                                    onChange={(e) => setRecipientInput(e.target.value)}
                                    placeholder="username or 0x..."
                                    style={{ width: "100%", padding: "0.5rem", fontSize: "0.9rem" }}
                                    required
                                />
                            </div>

                            <div style={{ marginTop: "1rem" }}>
                                <label htmlFor="amount" style={{ display: "block", marginBottom: "0.5rem" }}>
                                    Amount
                                </label>
                                <input
                                    id="amount"
                                    type="number"
                                    value={amount}
                                    onChange={(e) => setAmount(e.target.value)}
                                    placeholder="0"
                                    min="1"
                                    max={totalBalance}
                                    style={{ width: "100%", padding: "0.5rem" }}
                                    required
                                />
                            </div>

                            <div className="manage-notes-modal__count" style={{ marginTop: "1rem" }}>
                                Available: {totalBalance} ({availableNotes.length} note
                                {availableNotes.length !== 1 ? "s" : ""})
                            </div>
                            {totalBalance === 0 && (
                                <div style={{ marginTop: "0.5rem", fontSize: "0.85rem", color: "#666" }}>
                                    Tip: Request funds from the faucet by slicing pumpkins first, or receive a transfer
                                    from another user.
                                </div>
                            )}

                            {error && (
                                <div className="manage-notes-modal__message manage-notes-modal__message--error">
                                    {error}
                                </div>
                            )}

                            <div className="manage-notes-modal__actions" style={{ marginTop: "1.5rem" }}>
                                <button type="submit" className="pixel-button">
                                    Send Transfer
                                </button>
                                <button
                                    type="button"
                                    className="pixel-button pixel-button--ghost"
                                    onClick={handleClose}
                                >
                                    Cancel
                                </button>
                            </div>
                        </form>
                    )}

                    {status === "submitting" && (
                        <div style={{ textAlign: "center", padding: "2rem 0" }}>
                            {consolidationStep > 0 ? (
                                <>
                                    <div className="manage-notes-modal__description">
                                        Consolidating notes (step {consolidationStep})…
                                    </div>
                                    <div style={{ marginTop: "0.5rem", fontSize: "0.8rem", color: "#666" }}>
                                        Your balance is spread across too many notes. Merging them first.
                                    </div>
                                </>
                            ) : (
                                <>
                                    <div className="manage-notes-modal__description">Generating zero-knowledge proof…</div>
                                    <div style={{ marginTop: "0.5rem", fontSize: "0.8rem", color: "#666" }}>
                                        This may take 10–30 seconds.
                                    </div>
                                </>
                            )}
                            <div style={{ marginTop: "1rem" }}>
                                <span className="loading-spinner">⏳</span>
                            </div>
                        </div>
                    )}

                    {status === "success" && (
                        <div style={{ textAlign: "center", padding: "1rem 0" }}>
                            <div
                                className="manage-notes-modal__description"
                                style={{ fontSize: "1.2rem", marginBottom: "1rem" }}
                            >
                                ✅ Transfer Successful!
                            </div>
                            {txHash && (
                                <div
                                    style={{
                                        marginTop: "1rem",
                                        padding: "0.5rem",
                                        borderRadius: "4px",
                                        wordBreak: "break-all",
                                        fontFamily: "monospace",
                                        fontSize: "0.85rem",
                                    }}
                                >
                                    <strong>Transaction:</strong> {txHash}
                                </div>
                            )}
                            {noteShareNeeded && transferNote ? (
                                <div style={{ marginTop: "1rem", textAlign: "left" }}>
                                    <div
                                        className="manage-notes-modal__description"
                                        style={{ marginBottom: "0.5rem", fontWeight: "bold" }}
                                    >
                                        Share this note with the recipient
                                    </div>
                                    <div
                                        className="manage-notes-modal__description"
                                        style={{ fontSize: "0.8rem", marginBottom: "0.5rem", color: "#555" }}
                                    >
                                        No encryption key is known for this address. Send the note below via
                                        a messaging app so the recipient can import it.
                                    </div>
                                    <textarea
                                        readOnly
                                        rows={6}
                                        style={{
                                            width: "100%",
                                            fontFamily: "monospace",
                                            fontSize: "0.72rem",
                                            padding: "0.4rem",
                                            boxSizing: "border-box",
                                            resize: "none",
                                        }}
                                        value={JSON.stringify({ txHash, note: transferNote }, null, 2)}
                                    />
                                    <button
                                        type="button"
                                        className="pixel-button pixel-button--ghost pixel-button--compact"
                                        onClick={handleCopyNote}
                                        style={{ marginTop: "0.5rem", width: "100%" }}
                                    >
                                        {noteCopied ? "Copied!" : "Copy note JSON"}
                                    </button>
                                </div>
                            ) : (
                                <div className="manage-notes-modal__description" style={{ marginTop: "1rem" }}>
                                    Recipient will receive the note automatically.
                                </div>
                            )}
                            <div className="manage-notes-modal__actions" style={{ marginTop: "1.5rem" }}>
                                <button type="button" className="pixel-button" onClick={handleClose}>
                                    Close
                                </button>
                            </div>
                        </div>
                    )}

                    {status === "error" && (
                        <div style={{ textAlign: "center", padding: "1rem 0" }}>
                            <div
                                className="manage-notes-modal__description"
                                style={{ fontSize: "1.2rem", marginBottom: "1rem", color: "#d32f2f" }}
                            >
                                ❌ Transfer Failed
                            </div>
                            {error && (
                                <div className="manage-notes-modal__message manage-notes-modal__message--error">
                                    {error}
                                </div>
                            )}
                            <div className="manage-notes-modal__actions" style={{ marginTop: "1.5rem" }}>
                                <button type="button" className="pixel-button" onClick={handleTryAgain}>
                                    Try Again
                                </button>
                                <button
                                    type="button"
                                    className="pixel-button pixel-button--ghost"
                                    onClick={handleClose}
                                >
                                    Cancel
                                </button>
                            </div>
                        </div>
                    )}
                </div>

                <div className="manage-notes-modal__footer">
                    <div className="manage-notes-modal__description" style={{ fontSize: "0.85rem" }}>
                        Your secret keys never leave your browser.
                    </div>
                </div>
            </div>
        </div>
    );

    if (typeof document === "undefined") {
        return modalContent;
    }

    return createPortal(modalContent, document.body);
}
