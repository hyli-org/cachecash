import SHA256 from "crypto-js/sha256";
import { enc as Enc } from "crypto-js";
import { StoredNote } from "../types/note";
import { DerivedKeyPair } from "./KeyService";
import { encryptedNoteService } from "./EncryptedNoteService";
import { markNotesPending, clearPendingNotes, getPendingNoteHashes } from "./noteStorage";

/**
 * Note structure matching backend zk-primitives Note
 */
export interface Note {
  kind: string;
  contract: string;
  address: string;
  psi: string;
  value: string;
}

/**
 * A spendable note with its secret key
 */
export interface SpendableNote {
  note: Note;
  secretKey: string;
  value: number;
  txHash: string;
}

/**
 * Result of note selection
 */
export interface NoteSelection {
  selectedNotes: [SpendableNote, SpendableNote];
  changeAmount: number;
  totalInput: number;
}

/**
 * Transfer request payload
 */
export interface TransferRequest {
  recipient_pubkey: string;
  amount: number;
  output_notes: [Note, Note];
  input_commitments: [string, string];
  nullifiers: [string, string];
}

/**
 * Transfer response from backend
 */
export interface TransferResponse {
  tx_hash: string;
  change_note: Note | null;
}

/**
 * Convert a number to a 64-character hex string (32 bytes, big-endian)
 */
function toHex64(value: number): string {
  return value.toString(16).padStart(64, "0");
}

/**
 * Hash merge function matching backend hash::hash_merge
 */
function hashMerge(elements: string[]): string {
  // Concatenate all hex strings and hash
  const concatenated = elements.join("");
  const hash = SHA256(Enc.Hex.parse(concatenated));
  return hash.toString(Enc.Hex);
}

/**
 * Compute nullifier for an input note
 * nullifier = hash_merge([note.psi, secret_key])
 */
function computeNullifier(notePsi: string, secretKey: string): string {
  return hashMerge([notePsi, secretKey]);
}

/**
 * Create a padding note (all zeros)
 */
function createPaddingNote(): Note {
  return {
    kind: "0".repeat(64),
    contract: "0".repeat(64),
    address: "0".repeat(64),
    psi: "0".repeat(64),
    value: "0".repeat(64),
  };
}

/**
 * Create an input note with padding (no secret key)
 */
function createPaddingSpendableNote(): SpendableNote {
  return {
    note: createPaddingNote(),
    secretKey: "0".repeat(64),
    value: 0,
    txHash: "",
  };
}

/**
 * Create a new output note
 */
function createOutputNote(recipientPubkey: string, amount: number, contract: string): Note {
  // Generate random psi for the note
  const randomBytes = new Uint8Array(32);
  crypto.getRandomValues(randomBytes);
  const psi = Array.from(randomBytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");

  return {
    kind: contract,
    contract,
    address: recipientPubkey,
    psi,
    value: toHex64(amount),
  };
}

class TransferService {
  /**
   * Select 2 notes to spend for given amount.
   * Uses greedy algorithm: sort by value, take smallest sufficient combination.
   */
  selectNotesForTransfer(
    availableNotes: SpendableNote[],
    amount: number
  ): NoteSelection | null {
    const total = availableNotes.reduce((sum, n) => sum + n.value, 0);
    if (total < amount) {
      return null; // Insufficient balance
    }

    // Sort by value ascending
    const sorted = [...availableNotes].sort((a, b) => a.value - b.value);

    // Strategy: Try single note first, then two notes
    for (const note of sorted) {
      if (note.value >= amount) {
        return {
          selectedNotes: [note, createPaddingSpendableNote()],
          changeAmount: note.value - amount,
          totalInput: note.value,
        };
      }
    }

    // Try combinations of 2 notes
    for (let i = 0; i < sorted.length - 1; i++) {
      for (let j = i + 1; j < sorted.length; j++) {
        const sum = sorted[i].value + sorted[j].value;
        if (sum >= amount) {
          return {
            selectedNotes: [sorted[i], sorted[j]],
            changeAmount: sum - amount,
            totalInput: sum,
          };
        }
      }
    }

    return null;
  }

  /**
   * Build transfer transaction with input/output notes
   */
  buildSendTransaction(
    selection: NoteSelection,
    recipientPubkey: string,
    amount: number,
    senderPubkey: string
  ): TransferRequest {
    // Get contract from first input note (assuming all notes are same contract)
    const contract = selection.selectedNotes[0].note.contract;

    // Output 0: Recipient's note
    const recipientNote = createOutputNote(recipientPubkey, amount, contract);

    // Output 1: Change note or padding
    const changeNote =
      selection.changeAmount > 0
        ? createOutputNote(senderPubkey, selection.changeAmount, contract)
        : createPaddingNote();

    // Compute input commitments (these are the note commitments)
    const inputCommitments: [string, string] = [
      this.computeCommitment(selection.selectedNotes[0].note),
      this.computeCommitment(selection.selectedNotes[1].note),
    ];

    // Compute nullifiers for the inputs
    const nullifiers: [string, string] = [
      computeNullifier(
        selection.selectedNotes[0].note.psi,
        selection.selectedNotes[0].secretKey
      ),
      computeNullifier(
        selection.selectedNotes[1].note.psi,
        selection.selectedNotes[1].secretKey
      ),
    ];

    return {
      recipient_pubkey: recipientPubkey,
      amount,
      output_notes: [recipientNote, changeNote],
      input_commitments: inputCommitments,
      nullifiers,
    };
  }

  /**
   * Compute note commitment
   * commitment = hash_merge([kind, contract, address, psi, value])
   */
  private computeCommitment(note: Note): string {
    return hashMerge([note.kind, note.contract, note.address, note.psi, note.value]);
  }

  /**
   * Execute complete transfer: select notes, build tx, submit, notify recipient
   */
  async executeTransfer(
    recipientPubkey: string,
    amount: number,
    availableNotes: SpendableNote[],
    senderKeypair: DerivedKeyPair,
    playerName: string
  ): Promise<TransferResponse> {
    // 1. Select notes
    const selection = this.selectNotesForTransfer(availableNotes, amount);
    if (!selection) {
      throw new Error(
        `Insufficient balance. You have ${availableNotes.reduce(
          (sum, n) => sum + n.value,
          0
        )} but need ${amount}`
      );
    }

    // 2. Mark notes as pending to prevent double-spend
    const spentNoteHashes = selection.selectedNotes
      .filter((n) => n.value > 0) // Only non-padding notes
      .map((n) => n.txHash);
    markNotesPending(playerName, spentNoteHashes);

    try {
      // 3. Build transaction
      const transferRequest = this.buildSendTransaction(
        selection,
        recipientPubkey,
        amount,
        senderKeypair.publicKey
      );

      // 4. Submit to backend
      const baseUrl = import.meta.env.VITE_SERVER_BASE_URL?.replace(/\/$/, "") ?? "";
      const response = await fetch(`${baseUrl}/api/transfer`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify(transferRequest),
      });

      if (!response.ok) {
        const errorData = await response.json().catch(() => ({}));
        throw new Error(
          errorData.error || `Transfer failed with status ${response.status}`
        );
      }

      const result: TransferResponse = await response.json();

      // 5. Upload encrypted note for recipient
      try {
        await encryptedNoteService.uploadNote(
          recipientPubkey,
          {
            note: transferRequest.output_notes[0],
            txHash: result.tx_hash,
            amount,
            from: senderKeypair.publicKey,
            timestamp: Date.now(),
          },
          senderKeypair
        );
      } catch (error) {
        console.warn("Failed to upload encrypted note for recipient:", error);
        // Don't fail the transfer - the transaction already succeeded
      }

      // 6. Clear pending state (transfer successful)
      clearPendingNotes(playerName, spentNoteHashes);

      return result;
    } catch (error) {
      // Transfer failed - clear pending state so notes become spendable again
      clearPendingNotes(playerName, spentNoteHashes);
      throw error;
    }
  }

  /**
   * Convert StoredNote to SpendableNote
   */
  toSpendableNote(stored: StoredNote, secretKey: string): SpendableNote {
    const note = stored.note as any;

    // Handle different note formats (from faucet vs from transfer)
    let value = 0;
    if (typeof note.value === "string") {
      // Backend sends hex strings (Element serialization)
      // Try parsing as hex first, then fall back to decimal
      if (note.value.match(/^[0-9a-fA-F]+$/)) {
        value = parseInt(note.value, 16);
      } else {
        value = parseInt(note.value, 10);
      }
    } else if (typeof note.value === "number") {
      value = note.value;
    }

    // Ensure we have a proper Note structure
    const properNote: Note = {
      kind: note.kind || note.contract || "0".repeat(64),
      contract: note.contract || "0".repeat(64),
      address: note.address || "0".repeat(64),
      psi: note.psi || "0".repeat(64),
      value: toHex64(value),
    };

    return {
      note: properNote,
      secretKey,
      value,
      txHash: stored.txHash,
    };
  }

  /**
   * Get all spendable notes for a player (excluding pending notes)
   */
  getSpendableNotes(
    storedNotes: StoredNote[],
    secretKey: string,
    playerName: string
  ): SpendableNote[] {
    const pendingHashes = getPendingNoteHashes(playerName);

    return storedNotes
      .filter((stored) => {
        // Exclude pending notes
        if (pendingHashes.has(stored.txHash)) return false;

        // Exclude optimistic notes (not yet confirmed)
        const note = stored.note as any;
        if (note?.status === "optimistic") return false;

        return true;
      })
      .map((stored) => this.toSpendableNote(stored, secretKey))
      .filter((spendable) => spendable.value > 0); // Filter out padding/invalid notes
  }
}

export const transferService = new TransferService();
