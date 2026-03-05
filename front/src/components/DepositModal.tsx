import { useState, useCallback, FormEvent } from "react";
import { createPortal } from "react-dom";
import { FullIdentity } from "../services/KeyService";
import { addressService } from "../services/AddressService";
import { transferService, InputNoteData, TransferStep } from "../services/TransferService";

interface DepositModalProps {
    playerName: string;
    identity: FullIdentity | null;
    availableNotes: InputNoteData[];
    onClose: () => void;
}

type DepositStatus = "input" | "submitting" | "success" | "error";
type ProgressStep = "resolving" | TransferStep;

const DEPOSIT_RECIPIENT = "bank@hyli-utxo-state";

const PROGRESS_STEPS: { key: ProgressStep; label: string }[] = [
    { key: "resolving",         label: "Resolving recipient" },
    { key: "smt-witness",       label: "Fetching inclusion witnesses" },
    { key: "creating-blob",     label: "Preparing transaction" },
    { key: "proving-utxo",      label: "Generating UTXO proof" },
    { key: "proving-smt",       label: "Generating SMT proof" },
    { key: "submitting-proofs", label: "Submitting transaction & proofs" },
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

export function DepositModal({ playerName, identity, availableNotes, onClose }: DepositModalProps) {
    const [amount, setAmount] = useState("");
    const [status, setStatus] = useState<DepositStatus>("input");
    const [error, setError] = useState<string | null>(null);
    const [txHash, setTxHash] = useState<string | null>(null);
    const [consolidationStep, setConsolidationStep] = useState(0);
    const [progressStep, setProgressStep] = useState<ProgressStep | null>(null);

    const handleSubmit = useCallback(
        async (event: FormEvent) => {
            event.preventDefault();

            if (!identity || !playerName) {
                setError("Identity not available");
                return;
            }

            const amountNum = parseInt(amount, 10);
            if (isNaN(amountNum) || amountNum <= 0) {
                setError("Amount must be a positive number");
                return;
            }

            try {
                setStatus("submitting");
                setConsolidationStep(0);
                setProgressStep("resolving");
                setError(null);

                const resolved = await addressService.resolve(DEPOSIT_RECIPIENT);
                const result = await transferService.executeTransferWithConsolidation(
                    resolved.utxoAddress,
                    amountNum,
                    availableNotes,
                    identity,
                    playerName,
                    undefined,
                    (step) => {
                        setConsolidationStep(step);
                        setProgressStep(null);
                    },
                    (step) => setProgressStep(step),
                    true,
                );

                setStatus("success");
                setTxHash(result.txHash);
            } catch (err) {
                console.error("Deposit failed:", err);
                setStatus("error");
                setError(err instanceof Error ? err.message : "Deposit failed. Please try again.");
            }
        },
        [amount, identity, playerName, availableNotes],
    );

    const handleClose = useCallback(() => {
        setAmount("");
        setStatus("input");
        setError(null);
        setTxHash(null);
        setConsolidationStep(0);
        setProgressStep(null);
        onClose();
    }, [onClose]);

    const handleTryAgain = useCallback(() => {
        setStatus("input");
        setError(null);
        setTxHash(null);
        setConsolidationStep(0);
        setProgressStep(null);
    }, []);

    const modalContent = (
        <div className="manage-notes-modal__backdrop" role="presentation">
            <div className="manage-notes-modal" role="dialog" aria-modal="true" aria-labelledby="deposit-modal-title">
                <div className="manage-notes-modal__header">
                    <div className="manage-notes-modal__eyebrow">Deposit</div>
                    <h2 id="deposit-modal-title" className="manage-notes-modal__title">
                        {playerName || "---"}
                    </h2>
                </div>

                <div className="manage-notes-modal__body">
                    {status === "input" && (
                        <form onSubmit={handleSubmit}>
                            <div className="manage-notes-modal__description">
                                Deposit funds to the bank contract.
                            </div>

                            <div style={{ marginTop: "1rem" }}>
                                <label htmlFor="deposit-amount" style={{ display: "block", marginBottom: "0.5rem" }}>
                                    Amount
                                </label>
                                <input
                                    id="deposit-amount"
                                    type="number"
                                    value={amount}
                                    onChange={(e) => setAmount(e.target.value)}
                                    placeholder="0"
                                    min="1"
                                    style={{ width: "100%", padding: "0.5rem" }}
                                    required
                                />
                            </div>

                            {error && (
                                <div className="manage-notes-modal__message manage-notes-modal__message--error">
                                    {error}
                                </div>
                            )}

                            <div className="manage-notes-modal__actions" style={{ marginTop: "1.5rem" }}>
                                <button type="submit" className="pixel-button">
                                    Deposit
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
                        <div style={{ padding: "1.5rem 0" }}>
                            {consolidationStep > 0 && (
                                <div style={{ marginBottom: "1rem", textAlign: "center" }}>
                                    <div className="manage-notes-modal__description">
                                        Consolidating notes (round {consolidationStep})…
                                    </div>
                                    <div style={{ marginTop: "0.25rem", fontSize: "0.8rem", color: "#666" }}>
                                        Your balance is spread across too many notes. Merging them first.
                                    </div>
                                </div>
                            )}

                            <div style={{ display: "flex", flexDirection: "column", gap: "0.6rem" }}>
                                {PROGRESS_STEPS.map(({ key, label }) => {
                                    const state = stepStatus(key, progressStep);
                                    return (
                                        <div
                                            key={key}
                                            style={{
                                                display: "flex",
                                                alignItems: "center",
                                                gap: "0.6rem",
                                                opacity: state === "pending" ? 0.4 : 1,
                                            }}
                                        >
                                            <span
                                                style={{
                                                    width: "1.2rem",
                                                    textAlign: "center",
                                                    fontSize: "0.9rem",
                                                    flexShrink: 0,
                                                }}
                                            >
                                                {state === "done"
                                                    ? "✓"
                                                    : state === "active"
                                                      ? "⏳"
                                                      : "·"}
                                            </span>
                                            <span
                                                style={{
                                                    fontSize: "0.9rem",
                                                    fontWeight: state === "active" ? "bold" : "normal",
                                                    color: state === "done" ? "#2e7d32" : "inherit",
                                                }}
                                            >
                                                {label}
                                            </span>
                                        </div>
                                    );
                                })}
                            </div>

                            <div style={{ marginTop: "1.25rem", fontSize: "0.78rem", color: "#888", textAlign: "center" }}>
                                Proof generation may take 10–60 seconds.
                            </div>
                        </div>
                    )}

                    {status === "success" && (
                        <div style={{ textAlign: "center", padding: "1rem 0" }}>
                            <div
                                className="manage-notes-modal__description"
                                style={{ fontSize: "1.2rem", marginBottom: "1rem" }}
                            >
                                ✅ Deposit Successful!
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
                                ❌ Deposit Failed
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
