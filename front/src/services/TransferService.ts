import { StoredNote } from "../types/note";
import { DerivedKeyPair } from "./KeyService";
import { encryptedNoteService } from "./EncryptedNoteService";
import { markNotesPending, clearPendingNotes, getPendingNoteHashes } from "./noteStorage";
import { proverService, UtxoKind, ProverInput } from "./ProverService";

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
 * Input note data for transfer (includes full note + secret key)
 */
export interface InputNoteData {
  note: Note;
  secret_key: string;
}

/**
 * Transfer request payload
 */
export interface TransferRequest {
  recipient_pubkey: string;
  amount: number;
  input_notes: [InputNoteData, InputNoteData];
  output_notes: [Note, Note];
}

/**
 * Transfer response from backend
 */
export interface TransferResponse {
  tx_hash: string;
  change_note: Note | null;
}

/**
 * Proved transfer request payload (client-side proof)
 */
export interface ProvedTransferRequest {
  /** Base64-encoded proof bytes */
  proof: string;
  /** Public inputs as hex strings (733 field elements) */
  public_inputs: string[];
  /** 128-byte blob data as array */
  blob_data: number[];
  /** Output notes [recipient_note, change_note] */
  output_notes: [Note, Note];
}

/**
 * Transfer progress callback for UI updates
 */
export type TransferProgressCallback = (stage: TransferStage) => void;

/**
 * Stages of transfer process
 */
export type TransferStage =
  | "selecting_notes"
  | "building_transaction"
  | "initializing_prover"
  | "generating_proof"
  | "submitting_transaction"
  | "notifying_recipient"
  | "complete";

/**
 * Convert a number to a 64-character hex string (32 bytes, big-endian)
 */
function toHex64(value: number): string {
  return value.toString(16).padStart(64, "0");
}

// Note: hashMerge and computeNullifier are now handled by ProverService
// using the same Poseidon2 hash function from @aztec/bb.js

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
    kind: contract, // Circuit expects kind == contract (the token identifier)
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

    // Build input notes with secret keys (server needs full note data for proof)
    const inputNotes: [InputNoteData, InputNoteData] = [
      {
        note: selection.selectedNotes[0].note,
        secret_key: selection.selectedNotes[0].secretKey,
      },
      {
        note: selection.selectedNotes[1].note,
        secret_key: selection.selectedNotes[1].secretKey,
      },
    ];

    return {
      recipient_pubkey: recipientPubkey,
      amount,
      input_notes: inputNotes,
      output_notes: [recipientNote, changeNote],
    };
  }

  /**
   * Execute complete transfer with client-side proof generation
   * Secret keys never leave the browser
   */
  async executeTransfer(
    recipientPubkey: string,
    amount: number,
    availableNotes: SpendableNote[],
    senderKeypair: DerivedKeyPair,
    playerName: string,
    onProgress?: TransferProgressCallback
  ): Promise<TransferResponse> {
    const reportProgress = (stage: TransferStage) => {
      if (onProgress) {
        onProgress(stage);
      }
    };

    // 1. Select notes
    reportProgress("selecting_notes");
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
      // 3. Build transaction data
      reportProgress("building_transaction");
      const transferRequest = this.buildSendTransaction(
        selection,
        recipientPubkey,
        amount,
        senderKeypair.publicKey
      );

      // 4. Initialize prover if needed
      reportProgress("initializing_prover");
      if (!proverService.isInitialized()) {
        await proverService.initialize();
      }

      // 5. Build prover inputs
      // Get contract name from note's contract field or use default
      const contractName = "hyli_utxo"; // Standard contract name

      // Build blob data
      const blobData = proverService.buildBlobData(
        transferRequest.input_notes,
        transferRequest.output_notes,
        contractName,
        1, // blob_index (hyli_utxo blob is at index 1, state blob is at 0)
        2  // blob_count (state_blob + hyli_utxo_blob)
      );

      const proverInput: ProverInput = {
        inputNotes: transferRequest.input_notes,
        outputNotes: transferRequest.output_notes,
        blobData,
        kind: UtxoKind.Send,
      };

      // 6. Generate proof (this is the slow part)
      reportProgress("generating_proof");
      const proofResult = await proverService.generateProof(proverInput);

      // 7. Build proved transfer request
      const provedRequest: ProvedTransferRequest = {
        proof: proofResult.proof,
        public_inputs: proofResult.publicInputs,
        blob_data: Array.from(proofResult.blobData),
        output_notes: transferRequest.output_notes,
      };

      // 8. Submit to backend
      reportProgress("submitting_transaction");
      const baseUrl = import.meta.env.VITE_SERVER_BASE_URL?.replace(/\/$/, "") ?? "";
      const response = await fetch(`${baseUrl}/api/transfer/prove`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify(provedRequest),
      });

      if (!response.ok) {
        const errorData = await response.json().catch(() => ({}));
        throw new Error(
          errorData.error || `Transfer failed with status ${response.status}`
        );
      }

      const result: TransferResponse = await response.json();

      // 9. Upload encrypted note for recipient
      reportProgress("notifying_recipient");
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

      // 10. Clear pending state (transfer successful)
      reportProgress("complete");
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
