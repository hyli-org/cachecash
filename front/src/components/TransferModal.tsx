import { useState, useCallback, useMemo, FormEvent } from "react";
import { createPortal } from "react-dom";
import { DerivedKeyPair, deriveKeyPairFromName, isBarretenbergInitialized } from "../services/KeyService";
import { transferService, SpendableNote, TransferStage } from "../services/TransferService";
import { setStoredNotes, getStoredNotes } from "../services/noteStorage";
import { StoredNote } from "../types/note";

interface TransferModalProps {
  playerName: string;
  keyPair: DerivedKeyPair | null;
  availableNotes: SpendableNote[];
  onClose: () => void;
}

type TransferStatus = "input" | "submitting" | "proving" | "success" | "error";

/**
 * Human-readable descriptions for transfer stages
 */
const stageDescriptions: Record<TransferStage, string> = {
  selecting_notes: "Selecting notes...",
  building_transaction: "Building transaction...",
  initializing_prover: "Loading prover (first time may take a moment)...",
  generating_proof: "Generating zero-knowledge proof...",
  submitting_transaction: "Submitting to network...",
  notifying_recipient: "Notifying recipient...",
  complete: "Transfer complete!",
};

export function TransferModal({
  playerName,
  keyPair,
  availableNotes,
  onClose,
}: TransferModalProps) {
  const [recipientInput, setRecipientInput] = useState("");
  const [amount, setAmount] = useState("");
  const [status, setStatus] = useState<TransferStatus>("input");
  const [error, setError] = useState<string | null>(null);
  const [txHash, setTxHash] = useState<string | null>(null);
  const [currentStage, setCurrentStage] = useState<TransferStage | null>(null);

  const totalBalance = useMemo(
    () => availableNotes.reduce((sum, n) => sum + n.value, 0),
    [availableNotes]
  );

  // Resolve recipient input to public key (supports username or direct pubkey)
  const resolveRecipientPubkey = useCallback((input: string): string | null => {
    const trimmed = input.trim();
    if (!trimmed) return null;

    // Remove 0x prefix if present
    const normalized = trimmed.replace(/^0x/i, "");

    // Check if it's a valid 64-char hex string (direct pubkey)
    if (normalized.length === 64 && /^[0-9a-fA-F]+$/.test(normalized)) {
      return normalized;
    }

    // Otherwise, treat as username and derive pubkey
    if (!isBarretenbergInitialized()) {
      return null; // Barretenberg not ready yet
    }

    try {
      const derived = deriveKeyPairFromName(trimmed);
      return derived.publicKey;
    } catch (err) {
      return null;
    }
  }, []);

  const handleSubmit = useCallback(
    async (event: FormEvent) => {
      event.preventDefault();

      if (!keyPair || !playerName) {
        setError("Key pair not available");
        return;
      }

      // Resolve recipient (username or pubkey)
      const resolvedPubkey = resolveRecipientPubkey(recipientInput);
      if (!resolvedPubkey) {
        setError("Invalid recipient. Enter a username or 64-character public key.");
        return;
      }

      // Check if sending to self
      if (resolvedPubkey.toLowerCase() === keyPair.publicKey.toLowerCase()) {
        setError("You cannot send funds to yourself");
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
        setStatus("proving");
        setError(null);
        setCurrentStage("selecting_notes");

        // Progress callback to update UI
        const onProgress = (stage: TransferStage) => {
          setCurrentStage(stage);
          // Switch to submitting status when we're done proving
          if (stage === "submitting_transaction") {
            setStatus("submitting");
          }
        };

        // Execute the transfer with client-side proving
        const result = await transferService.executeTransfer(
          resolvedPubkey,
          amountNum,
          availableNotes,
          keyPair,
          playerName,
          onProgress
        );

        // Update local storage: remove spent notes, add change note if any
        const currentNotes = getStoredNotes(playerName);
        const spentTxHashes = new Set(
          availableNotes.slice(0, 2).map((n) => n.txHash)
        );

        // Filter out spent notes
        let updatedNotes = currentNotes.filter(
          (note) => !spentTxHashes.has(note.txHash)
        );

        // Add change note if exists
        if (result.change_note && parseInt(result.change_note.value, 10) > 0) {
          const changeNote: StoredNote = {
            txHash: result.tx_hash,
            note: result.change_note,
            storedAt: Date.now(),
            player: playerName,
          };
          updatedNotes = [changeNote, ...updatedNotes];
        }

        setStoredNotes(playerName, updatedNotes);

        // Show success
        setStatus("success");
        setTxHash(result.tx_hash);
      } catch (err) {
        console.error("Transfer failed:", err);
        setStatus("error");
        setError(
          err instanceof Error ? err.message : "Transfer failed. Please try again."
        );
      }
    },
    [recipientInput, amount, keyPair, playerName, availableNotes, totalBalance, resolveRecipientPubkey]
  );

  const handleClose = useCallback(() => {
    // Reset state when closing
    setRecipientInput("");
    setAmount("");
    setStatus("input");
    setError(null);
    setTxHash(null);
    setCurrentStage(null);
    onClose();
  }, [onClose]);

  const handleTryAgain = useCallback(() => {
    setStatus("input");
    setError(null);
    setTxHash(null);
    setCurrentStage(null);
  }, []);

  // Show derived pubkey hint when username is entered
  const recipientPubkeyHint = useMemo(() => {
    if (!recipientInput.trim()) return null;
    const pubkey = resolveRecipientPubkey(recipientInput);
    if (!pubkey) return null;

    // If input is already a pubkey, don't show hint
    const normalized = recipientInput.trim().replace(/^0x/i, "");
    if (normalized === pubkey) return null;

    return `→ ${pubkey.slice(0, 8)}...${pubkey.slice(-8)}`;
  }, [recipientInput, resolveRecipientPubkey]);

  const modalContent = (
    <div className="manage-notes-modal__backdrop" role="presentation">
      <div
        className="manage-notes-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="transfer-modal-title"
      >
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
                <label
                  htmlFor="recipient-input"
                  style={{ display: "block", marginBottom: "0.5rem" }}
                >
                  Recipient (Username or Public Key)
                </label>
                <input
                  id="recipient-input"
                  type="text"
                  value={recipientInput}
                  onChange={(e) => setRecipientInput(e.target.value)}
                  placeholder="username or 0x..."
                  style={{
                    width: "100%",
                    padding: "0.5rem",
                    fontSize: "0.9rem",
                  }}
                  required
                />
                {recipientPubkeyHint && (
                  <div style={{
                    marginTop: "0.25rem",
                    fontSize: "0.75rem",
                    color: "#666",
                    fontFamily: "monospace"
                  }}>
                    {recipientPubkeyHint}
                  </div>
                )}
              </div>

              <div style={{ marginTop: "1rem" }}>
                <label
                  htmlFor="amount"
                  style={{ display: "block", marginBottom: "0.5rem" }}
                >
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
                  style={{
                    width: "100%",
                    padding: "0.5rem",
                  }}
                  required
                />
              </div>

              <div className="manage-notes-modal__count" style={{ marginTop: "1rem" }}>
                Available: {totalBalance} USDC ({availableNotes.length} note{availableNotes.length !== 1 ? 's' : ''})
              </div>
              {totalBalance === 0 && (
                <div style={{ marginTop: "0.5rem", fontSize: "0.85rem", color: "#666" }}>
                  Tip: Request funds from the faucet by slicing pumpkins first, or receive a transfer from another user.
                </div>
              )}

              {error && (
                <div className="manage-notes-modal__message manage-notes-modal__message--error">
                  {error}
                </div>
              )}

              <div
                className="manage-notes-modal__actions"
                style={{ marginTop: "1.5rem" }}
              >
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

          {status === "proving" && (
            <div style={{ textAlign: "center", padding: "2rem 0" }}>
              <div className="manage-notes-modal__description">
                {currentStage ? stageDescriptions[currentStage] : "Preparing..."}
              </div>
              <div style={{ marginTop: "1rem" }}>
                <span className="loading-spinner">⏳</span>
              </div>
              {currentStage === "generating_proof" && (
                <div style={{
                  marginTop: "1rem",
                  fontSize: "0.85rem",
                  color: "#666",
                }}>
                  This may take 10-30 seconds. Your secret keys stay in your browser.
                </div>
              )}
              {currentStage === "initializing_prover" && (
                <div style={{
                  marginTop: "1rem",
                  fontSize: "0.85rem",
                  color: "#666",
                }}>
                  Downloading circuit artifacts...
                </div>
              )}
            </div>
          )}

          {status === "submitting" && (
            <div style={{ textAlign: "center", padding: "2rem 0" }}>
              <div className="manage-notes-modal__description">
                {currentStage ? stageDescriptions[currentStage] : "Submitting transaction..."}
              </div>
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
                    background: "#f0f0f0",
                    borderRadius: "4px",
                    wordBreak: "break-all",
                    fontFamily: "monospace",
                    fontSize: "0.85rem",
                  }}
                >
                  <strong>Transaction:</strong> {txHash}
                </div>
              )}
              <div
                className="manage-notes-modal__description"
                style={{ marginTop: "1rem" }}
              >
                Recipient will receive the note automatically.
              </div>
              <div className="manage-notes-modal__actions" style={{ marginTop: "1.5rem" }}>
                <button
                  type="button"
                  className="pixel-button"
                  onClick={handleClose}
                >
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
                <button
                  type="button"
                  className="pixel-button"
                  onClick={handleTryAgain}
                >
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
