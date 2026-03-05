import { useState, useCallback, useMemo, FormEvent } from "react";
import { createPortal } from "react-dom";
import { FullIdentity } from "../services/KeyService";
import { addressService } from "../services/AddressService";
import { transferService, InputNoteData, parseNoteValue, TransferStep } from "../services/TransferService";
import { PrivateNote } from "../types/note";

interface TransferModalProps {
    playerName: string;
    identity: FullIdentity | null;
    availableNotes: InputNoteData[];
    onClose: () => void;
}

type TransferStatus = "input" | "submitting" | "success" | "error";

type ProgressStep = "resolving" | TransferStep;

const PROGRESS_STEPS: { key: ProgressStep; label: string; desc: string }[] = [
    {
        key: "resolving",
        label: "Resolving recipient",
        desc: "Looking up the recipient's UTXO address from their username.",
    },
    {
        key: "smt-witness",
        label: "Fetching inclusion witnesses",
        desc: "Retrieving cryptographic witnesses that prove your notes exist in the UTXO tree.",
    },
    {
        key: "creating-blob",
        label: "Preparing transaction",
        desc: "Building the transaction blobs with your note commitments and nullifiers.",
    },
    {
        key: "proving-utxo",
        label: "Generating UTXO proof",
        desc: "Proving the validity of your note and the new note for the recipient — zero-knowledge.",
    },
    {
        key: "proving-smt",
        label: "Generating SMT proof",
        desc: "Proving your note is included in the Sparse Merkle Tree. This ensures privacy.",
    },
    {
        key: "submitting-proofs",
        label: "Submitting transaction & proofs",
        desc: "Broadcasting the transaction and its proofs to the Hyli network.",
    },
];

function stepStatus(
    step: ProgressStep,
    current: ProgressStep | null,
): "done" | "active" | "pending" {
    const idx = PROGRESS_STEPS.findIndex((s) => s.key === step);
    const curIdx = current ? PROGRESS_STEPS.findIndex((s) => s.key === current) : -1;
    if (idx < curIdx) return "done";
    if (idx === curIdx) return "active";
    return "pending";
}

export function TransferModal({
    playerName,
    identity,
    availableNotes,
    onClose,
}: TransferModalProps) {
    const [recipientInput, setRecipientInput] = useState("");
    const [amount, setAmount] = useState("");
    const [status, setStatus] = useState<TransferStatus>("input");
    const [error, setError] = useState<string | null>(null);
    const [txHash, setTxHash] = useState<string | null>(null);
    const [transferNote, setTransferNote] = useState<PrivateNote | null>(null);
    const [noteCopied, setNoteCopied] = useState(false);
    const [noteShareNeeded, setNoteShareNeeded] = useState(false);
    const [consolidationStep, setConsolidationStep] = useState(0);
    const [progressStep, setProgressStep] = useState<ProgressStep | null>(null);

    const totalBalance = useMemo(
        () => availableNotes.reduce((sum, n) => sum + parseNoteValue(n.note), 0),
        [availableNotes],
    );

    const executeTransfer = useCallback(
        async () => {
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
                setProgressStep("resolving");
                setError(null);

                let recipientUtxoAddress: string;
                let recipientEncryptionPubkey: string | undefined;

                const normalized = trimmedRecipient.replace(/^0x/i, "");
                if (normalized.length === 64 && /^[0-9a-fA-F]+$/.test(normalized)) {
                    recipientUtxoAddress = normalized;
                    recipientEncryptionPubkey = undefined;
                } else {
                    const resolved = await addressService.resolve(trimmedRecipient);
                    recipientUtxoAddress = resolved.utxoAddress;
                    recipientEncryptionPubkey = resolved.encryptionPubkey;
                }

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
                    (step) => {
                        setConsolidationStep(step);
                        setProgressStep(null);
                    },
                    (step) => setProgressStep(step),
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

    const handleSubmit = useCallback(
        async (event: FormEvent) => {
            event.preventDefault();
            await executeTransfer();
        },
        [executeTransfer],
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
        setProgressStep(null);
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
        setProgressStep(null);
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
        <div className="modal-backdrop" role="presentation">
            <div className="modal" role="dialog" aria-modal="true" aria-labelledby="transfer-modal-title">
                <div className="modal-header">
                    <div className="modal-eyebrow">Send Money</div>
                    <h2 id="transfer-modal-title" className="modal-title">
                        {playerName || "---"}
                    </h2>
                </div>

                <div className="modal-body">
                    {status === "input" && (
                        <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: "1rem" }}>
                            <p className="modal-section-desc">
                                Transfer funds to another user. Enter their username or UTXO address.
                            </p>

                            <div className="form-group">
                                <label htmlFor="recipient-input" className="form-label">
                                    Recipient
                                </label>
                                <input
                                    id="recipient-input"
                                    type="text"
                                    className="form-input"
                                    value={recipientInput}
                                    onChange={(e) => setRecipientInput(e.target.value)}
                                    placeholder="username or 0x…"
                                    required
                                />
                                <span className="form-hint">Username or 64-character hex UTXO address</span>
                            </div>

                            <div className="form-group">
                                <label htmlFor="amount" className="form-label">
                                    Amount
                                </label>
                                <input
                                    id="amount"
                                    type="number"
                                    className="form-input"
                                    value={amount}
                                    onChange={(e) => setAmount(e.target.value)}
                                    placeholder="0"
                                    min="1"
                                    max={totalBalance}
                                    required
                                />
                                <span className="form-hint">
                                    Available: {totalBalance.toLocaleString()} ({availableNotes.length} note
                                    {availableNotes.length !== 1 ? "s" : ""})
                                </span>
                            </div>

                            {error && <div className="status-error">{error}</div>}

                            <div style={{ display: "flex", gap: "0.5rem" }}>
                                <button type="submit" className="btn btn-primary">
                                    Send
                                </button>
                                <button type="button" className="btn btn-ghost" onClick={handleClose}>
                                    Cancel
                                </button>
                            </div>
                        </form>
                    )}

                    {status === "submitting" && (
                        <div style={{ display: "flex", flexDirection: "column", gap: "1rem" }}>
                            {consolidationStep > 0 && (
                                <div className="status-info">
                                    <strong>Consolidating notes (round {consolidationStep})…</strong>
                                    <br />
                                    Your balance is split across multiple notes. We need to merge them into one before
                                    sending — this takes a moment.
                                </div>
                            )}

                            <div className="progress-steps">
                                {PROGRESS_STEPS.map(({ key, label, desc }) => {
                                    const state = stepStatus(key, progressStep);
                                    return (
                                        <div key={key} className={`progress-step progress-step--${state}`}>
                                            <div className="progress-step-indicator">
                                                {state === "done" ? "✓" : state === "active" ? "●" : "·"}
                                            </div>
                                            <div className="progress-step-content">
                                                <div className="progress-step-label">{label}</div>
                                                {state === "active" && (
                                                    <div className="progress-step-desc">{desc}</div>
                                                )}
                                            </div>
                                        </div>
                                    );
                                })}
                            </div>

                            <p className="form-hint" style={{ textAlign: "center" }}>
                                Proof generation may take 10–60 seconds.
                            </p>
                        </div>
                    )}

                    {status === "success" && (
                        <div style={{ display: "flex", flexDirection: "column", gap: "1rem" }}>
                            <div className="success-title">Transfer Successful</div>

                            {txHash && (
                                <div className="modal-section">
                                    <div className="modal-section-title">Transaction</div>
                                    <div className="tx-hash-box">{txHash}</div>
                                </div>
                            )}

                            {noteShareNeeded && transferNote ? (
                                <div className="note-share-area">
                                    <div className="note-share-label">Share this note with the recipient</div>
                                    <p className="note-share-desc">
                                        No encryption key is known for this address. Send the note below via a
                                        messaging app so the recipient can import it.
                                    </p>
                                    <textarea
                                        readOnly
                                        rows={6}
                                        className="form-input"
                                        value={JSON.stringify({ txHash, note: transferNote }, null, 2)}
                                    />
                                    <button type="button" className="btn btn-secondary" onClick={handleCopyNote}>
                                        {noteCopied ? "Copied!" : "Copy note JSON"}
                                    </button>
                                </div>
                            ) : (
                                <p className="modal-section-desc">
                                    The recipient will receive the note automatically.
                                </p>
                            )}

                            <button type="button" className="btn btn-primary" onClick={handleClose}>
                                Close
                            </button>
                        </div>
                    )}

                    {status === "error" && (
                        <div style={{ display: "flex", flexDirection: "column", gap: "1rem" }}>
                            <div style={{ fontSize: "1.1rem", fontWeight: 600, color: "var(--error)" }}>
                                Transfer Failed
                            </div>
                            {error && <div className="status-error">{error}</div>}
                            <div style={{ display: "flex", gap: "0.5rem" }}>
                                <button type="button" className="btn btn-primary" onClick={handleTryAgain}>
                                    Try Again
                                </button>
                                <button type="button" className="btn btn-ghost" onClick={handleClose}>
                                    Cancel
                                </button>
                            </div>
                        </div>
                    )}
                </div>

                <div className="modal-footer">
                    <span className="modal-footer-note">Your secret keys never leave your browser.</span>
                </div>
            </div>
        </div>
    );

    if (typeof document === "undefined") {
        return modalContent;
    }

    return createPortal(modalContent, document.body);
}
