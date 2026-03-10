import { useState, useCallback, useEffect, FormEvent } from "react";
import { createPortal } from "react-dom";
import { FullIdentity } from "../services/KeyService";
import { nodeService } from "../services/NodeService";
import { addStoredNote } from "../services/noteStorage";
import { getOranjIndexerUrl } from "../services/ConfigService";
import { PrivateNote, StoredNote } from "../types/note";
import { useWallet, WalletOperations } from "hyli-wallet";

interface DepositModalProps {
    playerName: string;
    walletAddress: string;
    identity: FullIdentity | null;
    onClose: () => void;
}

type DepositStatus = "input" | "submitting" | "success" | "error";

export function DepositModal({ playerName, walletAddress, identity, onClose }: DepositModalProps) {
    const { wallet } = useWallet();
    const [amount, setAmount] = useState("");
    const [status, setStatus] = useState<DepositStatus>("input");
    const [error, setError] = useState<string | null>(null);
    const [oranjBalance, setOranjBalance] = useState<number | null>(null);
    const [balanceError, setBalanceError] = useState<string | null>(null);

    useEffect(() => {
        if (!walletAddress) return;
        const url = `${getOranjIndexerUrl()}/v1/indexer/contract/oranj/balance/${walletAddress}`;
        fetch(url)
            .then((r) => {
                if (r.status === 404) return { balance: 0 };
                if (!r.ok) throw new Error(`Status ${r.status}`);
                return r.json();
            })
            .then((data: { balance: number }) => setOranjBalance(data.balance))
            .catch(() => setBalanceError("Could not fetch Oranj balance."));
    }, [walletAddress]);

    const handleSubmit = useCallback(
        async (event: FormEvent) => {
            event.preventDefault();

            if (!identity?.utxoAddress || !playerName) {
                setError("Identity not available");
                return;
            }

            const amountNum = parseInt(amount, 10);
            if (isNaN(amountNum) || amountNum <= 0) {
                setError("Amount must be a positive number");
                return;
            }

            if (oranjBalance !== null && amountNum > oranjBalance) {
                setError(`Insufficient Oranj balance. You have ${oranjBalance.toLocaleString()} but entered ${amountNum.toLocaleString()}.`);
                return;
            }

            try {
                setStatus("submitting");
                setError(null);

                let secp256k1Blob: number[] | undefined;
                let walletBlob: number[] | undefined;
                if (wallet?.sessionKey) {
                    const [secp256k1, walletIdentity] = WalletOperations.createIdentityBlobs(wallet);
                    secp256k1Blob = Array.from(secp256k1.data);
                    walletBlob = Array.from(walletIdentity.data);
                }

                const response = await nodeService.requestDeposit(identity.utxoAddress, amountNum, "oranj", playerName, secp256k1Blob, walletBlob);
                const { tx_hash, note } = response;
                const reference = tx_hash ?? (note as any)?.psi;
                const entry: StoredNote = {
                    txHash: reference ?? `deposit-${Date.now()}`,
                    note: (note ?? response) as unknown as PrivateNote,
                    storedAt: Date.now(),
                    player: playerName,
                };
                addStoredNote(playerName, entry);
                setStatus("success");
            } catch (err) {
                console.error("Deposit failed:", err);
                setStatus("error");
                setError(err instanceof Error ? err.message : "Deposit request failed.");
            }
        },
        [amount, identity, playerName, oranjBalance, wallet],
    );

    const handleClose = useCallback(() => {
        setAmount("");
        setStatus("input");
        setError(null);
        onClose();
    }, [onClose]);

    const handleTryAgain = useCallback(() => {
        setStatus("input");
        setError(null);
    }, []);

    const modalContent = (
        <div className="modal-backdrop" role="presentation">
            <div className="modal" role="dialog" aria-modal="true" aria-labelledby="deposit-modal-title">
                <div className="modal-header">
                    <div className="modal-eyebrow">Deposit</div>
                    <h2 id="deposit-modal-title" className="modal-title">
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
                                <label htmlFor="deposit-amount" className="form-label">
                                    Amount
                                </label>
                                <input
                                    id="deposit-amount"
                                    type="number"
                                    className="form-input"
                                    value={amount}
                                    onChange={(e) => setAmount(e.target.value)}
                                    placeholder="0"
                                    min="1"
                                    required
                                    autoFocus
                                />
                                <span className="form-hint">
                                    {balanceError
                                        ? balanceError
                                        : oranjBalance === null
                                          ? "Fetching balance…"
                                          : `Oranj balance: ${oranjBalance.toLocaleString()}`}
                                </span>
                            </div>

                            {error && <div className="status-error">{error}</div>}

                            <div style={{ display: "flex", gap: "0.5rem" }}>
                                <button type="submit" className="btn btn-primary">
                                    Deposit
                                </button>
                                <button type="button" className="btn btn-ghost" onClick={handleClose}>
                                    Cancel
                                </button>
                            </div>
                        </form>
                    )}

                    {status === "submitting" && (
                        <div style={{ display: "flex", flexDirection: "column", gap: "1rem", alignItems: "center" }}>
                            <p className="modal-section-desc">Processing deposit…</p>
                        </div>
                    )}

                    {status === "success" && (
                        <div style={{ display: "flex", flexDirection: "column", gap: "1rem" }}>
                            <div className="status-success">Deposit has been sent.</div>

                            <button type="button" className="btn btn-primary" onClick={handleClose}>
                                Close
                            </button>
                        </div>
                    )}

                    {status === "error" && (
                        <div style={{ display: "flex", flexDirection: "column", gap: "1rem" }}>
                            <div style={{ fontSize: "1.1rem", fontWeight: 600, color: "var(--error)" }}>
                                Deposit Failed
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
