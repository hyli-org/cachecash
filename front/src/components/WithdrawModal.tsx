import { useState, useCallback, useMemo, FormEvent } from "react";
import { createPortal } from "react-dom";
import { FullIdentity } from "../services/KeyService";
import { InputNoteData, parseNoteValue, transferService, TransferStep } from "../services/TransferService";

interface WithdrawModalProps {
    playerName: string;
    walletAddress: string;
    identity: FullIdentity | null;
    availableNotes: InputNoteData[];
    onClose: () => void;
}

type WithdrawStatus = "input" | "submitting" | "success" | "error";

type ProgressStep = TransferStep;

const PROGRESS_STEPS: { key: ProgressStep; label: string; desc: string }[] = [
    {
        key: "smt-witness",
        label: "Fetching inclusion witnesses",
        desc: "Retrieving cryptographic witnesses for the notes you are withdrawing.",
    },
    {
        key: "creating-blob",
        label: "Preparing transaction",
        desc: "Building the burn transaction and the bank-to-wallet token transfer blob.",
    },
    {
        key: "proving-utxo",
        label: "Generating UTXO proof",
        desc: "Proving that your notes are valid and that the withdrawal amount is burned correctly.",
    },
    {
        key: "proving-smt",
        label: "Generating SMT proof",
        desc: "Proving that the spent notes are included in the Sparse Merkle Tree.",
    },
    {
        key: "submitting-proofs",
        label: "Submitting transaction & proofs",
        desc: "Broadcasting the withdrawal transaction and proofs to the Hyli network.",
    },
];

function stepStatus(step: ProgressStep, current: ProgressStep | null): "done" | "active" | "pending" {
    const idx = PROGRESS_STEPS.findIndex((s) => s.key === step);
    const curIdx = current ? PROGRESS_STEPS.findIndex((s) => s.key === current) : -1;
    if (idx < curIdx) return "done";
    if (idx === curIdx) return "active";
    return "pending";
}

export function WithdrawModal({
    playerName,
    walletAddress,
    identity,
    availableNotes,
    onClose,
}: WithdrawModalProps) {
    const [amount, setAmount] = useState("");
    const [status, setStatus] = useState<WithdrawStatus>("input");
    const [error, setError] = useState<string | null>(null);
    const [progressStep, setProgressStep] = useState<ProgressStep | null>(null);

    const totalBalance = useMemo(
        () => availableNotes.reduce((sum, note) => sum + parseNoteValue(note.note), 0),
        [availableNotes],
    );

    const handleSubmit = useCallback(
        async (event: FormEvent) => {
            event.preventDefault();

            if (!identity || !playerName) {
                setError("Identity not available");
                return;
            }

            if (!walletAddress) {
                setError("Wallet address not available");
                return;
            }

            const amountNum = parseInt(amount, 10);
            if (isNaN(amountNum) || amountNum <= 0) {
                setError("Amount must be a positive number");
                return;
            }

            if (amountNum > totalBalance) {
                setError(`Insufficient note balance. You have ${totalBalance.toLocaleString()} but entered ${amountNum.toLocaleString()}.`);
                return;
            }

            try {
                setStatus("submitting");
                setError(null);
                setProgressStep("smt-witness");

                await transferService.executeWithdraw(
                    walletAddress,
                    amountNum,
                    availableNotes,
                    identity,
                    playerName,
                    (step) => setProgressStep(step),
                );

                setStatus("success");
            } catch (err) {
                console.error("Withdraw failed:", err);
                setStatus("error");
                setError(err instanceof Error ? err.message : "Withdraw request failed.");
            }
        },
        [amount, availableNotes, identity, playerName, totalBalance, walletAddress],
    );

    const handleClose = useCallback(() => {
        setAmount("");
        setStatus("input");
        setError(null);
        setProgressStep(null);
        onClose();
    }, [onClose]);

    const handleTryAgain = useCallback(() => {
        setStatus("input");
        setError(null);
        setProgressStep(null);
    }, []);

    const modalContent = (
        <div className="modal-backdrop" role="presentation">
            <div className="modal" role="dialog" aria-modal="true" aria-labelledby="withdraw-modal-title">
                <div className="modal-header">
                    <div className="modal-eyebrow">Withdraw</div>
                    <h2 id="withdraw-modal-title" className="modal-title">
                        {playerName || "---"}
                    </h2>
                </div>

                <div className="modal-body">
                    {status === "input" && (
                        <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: "1rem" }}>
                            <div className="form-group">
                                <label className="form-label">Token</label>
                                <select className="form-input" disabled value="oranj">
                                    <option value="oranj">Oranj</option>
                                </select>
                                <span className="form-hint">More tokens coming soon.</span>
                            </div>

                            <div className="form-group">
                                <label htmlFor="withdraw-amount" className="form-label">
                                    Amount
                                </label>
                                <input
                                    id="withdraw-amount"
                                    type="number"
                                    className="form-input"
                                    value={amount}
                                    onChange={(e) => setAmount(e.target.value)}
                                    placeholder="0"
                                    min="1"
                                    max={totalBalance}
                                    required
                                    autoFocus
                                />
                                <span className="form-hint">
                                    Available in notes: {totalBalance.toLocaleString()}
                                </span>
                            </div>

                            {error && <div className="status-error">{error}</div>}

                            <div style={{ display: "flex", gap: "0.5rem" }}>
                                <button type="submit" className="btn btn-primary">
                                    Withdraw
                                </button>
                                <button type="button" className="btn btn-ghost" onClick={handleClose}>
                                    Cancel
                                </button>
                            </div>
                        </form>
                    )}

                    {status === "submitting" && (
                        <div style={{ display: "flex", flexDirection: "column", gap: "1rem" }}>
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
                            <div className="status-success">Withdraw has been sent.</div>

                            <button type="button" className="btn btn-primary" onClick={handleClose}>
                                Close
                            </button>
                        </div>
                    )}

                    {status === "error" && (
                        <div style={{ display: "flex", flexDirection: "column", gap: "1rem" }}>
                            <div style={{ fontSize: "1.1rem", fontWeight: 600, color: "var(--error)" }}>
                                Withdraw Failed
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
