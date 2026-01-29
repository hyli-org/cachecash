import { BarretenbergSync, Fr, UltraHonkBackend } from "@aztec/bb.js";
import { Noir } from "@noir-lang/noir_js";
import type { InputMap, InputValue, CompiledCircuit } from "@noir-lang/types";
import { Note, InputNoteData } from "./TransferService";

// Constants matching the Rust backend
const HYLI_BLOB_LENGTH_BYTES = 128;

/**
 * UTXO transaction types matching the Rust backend
 */
export enum UtxoKind {
  Null = 0,
  Send = 1,
  Mint = 2,
  Burn = 3,
}

/**
 * Blob data for proof generation
 */
export interface BlobData {
  /** 128-byte blob: [input_commit_0, input_commit_1, nullifier_0, nullifier_1] */
  blob: Uint8Array;
  /** Contract name for the blob */
  contractName: string;
  /** Transaction identity string */
  identity: string;
  /** Placeholder tx_hash (64 hex chars, will be finalized after blob submission) */
  txHash: string;
  /** Number of blobs in the transaction */
  blobCount: number;
  /** Index of this blob in the transaction */
  blobIndex: number;
}

/**
 * Prover input structure
 */
export interface ProverInput {
  inputNotes: [InputNoteData, InputNoteData];
  outputNotes: [Note, Note];
  blobData: BlobData;
  kind: UtxoKind;
}

/**
 * Generated proof result
 */
export interface ProofResult {
  /** Base64-encoded proof bytes (raw proof without public inputs) */
  proof: string;
  /** Public inputs as hex strings (733 field elements) */
  publicInputs: string[];
  /** 128-byte blob data */
  blobData: Uint8Array;
}

/**
 * Service for generating UTXO proofs in the browser
 */
class ProverService {
  private circuit: CompiledCircuit | null = null;
  private backend: UltraHonkBackend | null = null;
  private noir: Noir | null = null;
  private initPromise: Promise<void> | null = null;

  /**
   * Initialize the prover by loading circuit artifacts
   */
  async initialize(): Promise<void> {
    if (this.initPromise) {
      return this.initPromise;
    }

    this.initPromise = this.doInitialize();
    return this.initPromise;
  }

  private async doInitialize(): Promise<void> {
    const baseUrl = import.meta.env.VITE_SERVER_BASE_URL?.replace(/\/$/, "") ?? "";

    // Load circuit JSON
    const circuitResponse = await fetch(`${baseUrl}/circuit/hyli_utxo.json`);
    if (!circuitResponse.ok) {
      throw new Error(`Failed to load circuit: ${circuitResponse.status}`);
    }
    this.circuit = await circuitResponse.json() as CompiledCircuit;

    // Initialize backend (it will generate keys as needed)
    this.backend = new UltraHonkBackend(this.circuit.bytecode);

    // Initialize Noir instance for witness generation
    this.noir = new Noir(this.circuit);
  }

  /**
   * Check if the prover has been initialized
   */
  isInitialized(): boolean {
    return this.circuit !== null && this.backend !== null && this.noir !== null;
  }

  /**
   * Generate a proof for a UTXO transfer
   */
  async generateProof(input: ProverInput): Promise<ProofResult> {
    if (!this.isInitialized()) {
      await this.initialize();
    }

    if (!this.noir || !this.backend) {
      throw new Error("Prover not initialized");
    }

    // Build input map
    const inputMap = this.buildInputMap(input);

    // Execute circuit to get witness
    const { witness } = await this.noir.execute(inputMap);

    // Generate proof
    const proofData = await this.backend.generateProof(witness);

    // Extract public inputs and raw proof
    // The proof structure from backend_barretenberg varies by version
    // Typically: { proof: Uint8Array, publicInputs: string[] }
    const publicInputs = proofData.publicInputs || [];
    const proofBytes = proofData.proof;

    return {
      proof: this.uint8ArrayToBase64(proofBytes),
      publicInputs: publicInputs.map((pi: string) => pi.startsWith("0x") ? pi.slice(2) : pi),
      blobData: input.blobData.blob,
    };
  }

  /**
   * Build the input map for the circuit
   * This matches the Rust build_hyli_input_map function
   */
  private buildInputMap(input: ProverInput): InputMap {
    const { inputNotes, outputNotes, blobData, kind } = input;

    // Build commitments and nullifiers
    const commitments = this.computeCommitments(inputNotes, outputNotes);
    const messages = this.computeMessages(kind);

    // Build padded strings
    const paddedIdentity = this.padString(blobData.identity, 256);
    const paddedContractName = this.padString(blobData.contractName, 256);
    const paddedTxHash = this.padString(blobData.txHash, 64);

    const inputMap: InputMap = {
      // Circuit metadata
      version: 1,
      initial_state_len: 4,
      initial_state: [0, 0, 0, 0],
      next_state_len: 4,
      next_state: [0, 0, 0, 0],

      // Identity
      identity_len: blobData.identity.length,
      identity: paddedIdentity,

      // Transaction
      tx_hash: paddedTxHash,
      index: blobData.blobIndex,
      blob_number: 1,
      blob_index: blobData.blobIndex,

      // Blob contract
      blob_contract_name_len: blobData.contractName.length,
      blob_contract_name: paddedContractName,

      // Blob data
      blob_capacity: HYLI_BLOB_LENGTH_BYTES,
      blob_len: HYLI_BLOB_LENGTH_BYTES,
      blob: Array.from(blobData.blob),

      // Transaction count
      tx_blob_count: blobData.blobCount,
      success: true,

      // Notes
      input_notes: [
        this.buildInputNoteStruct(inputNotes[0]),
        this.buildInputNoteStruct(inputNotes[1]),
      ],
      output_notes: [
        this.buildNoteStruct(outputNotes[0]),
        this.buildNoteStruct(outputNotes[1]),
      ],

      // Message hint (pmessage4 is messages[4])
      pmessage4: this.toFieldHex(messages[4]),

      // Commitments
      commitments: commitments.map(c => this.toFieldHex(c)),

      // Messages
      messages: messages.map(m => this.toFieldHex(m)),
    };

    return inputMap;
  }

  /**
   * Build an input note struct for the circuit
   */
  private buildInputNoteStruct(noteData: InputNoteData): InputValue {
    return {
      note: this.buildNoteStruct(noteData.note),
      secret_key: this.toFieldHex(noteData.secret_key),
    };
  }

  /**
   * Build a note struct for the circuit
   */
  private buildNoteStruct(note: Note): InputValue {
    return {
      kind: this.toFieldHex(note.kind),
      value: this.toFieldHex(note.value),
      address: this.toFieldHex(note.address),
      psi: this.toFieldHex(note.psi),
    };
  }

  /**
   * Convert hex string to field element format
   */
  private toFieldHex(hexStr: string): string {
    const normalized = hexStr.startsWith("0x") ? hexStr : "0x" + hexStr;
    return normalized;
  }

  /**
   * Compute commitments for all notes
   */
  private computeCommitments(
    inputNotes: [InputNoteData, InputNoteData],
    outputNotes: [Note, Note]
  ): string[] {
    return [
      this.computeNoteCommitment(inputNotes[0].note),
      this.computeNoteCommitment(inputNotes[1].note),
      this.computeNoteCommitment(outputNotes[0]),
      this.computeNoteCommitment(outputNotes[1]),
    ];
  }

  /**
   * Compute commitment for a single note
   * Matches circuit: poseidon2([0x2, kind, value, address, psi, 0, 0], 7)
   * Note: kind == contract (token identifier) in the frontend Note structure
   */
  private computeNoteCommitment(note: Note): string {
    // Padding notes have zero commitment
    if (note.kind === "0".repeat(64) || note.value === "0".repeat(64)) {
      return "0".repeat(64);
    }

    const bb = BarretenbergSync.getSingleton();
    const frs = [
      Fr.fromString("0x2"),                   // format indicator (always 2)
      Fr.fromString("0x" + note.kind),        // kind (token identifier)
      Fr.fromString("0x" + note.value),       // value
      Fr.fromString("0x" + note.address),     // address
      Fr.fromString("0x" + note.psi),         // psi
      Fr.ZERO,                                // padding
      Fr.ZERO,                                // padding
    ];
    const result = bb.poseidon2Hash(frs);
    return result.toString().slice(2).toLowerCase();
  }

  /**
   * Compute nullifier for an input note
   * nullifier = poseidon2([psi, secret_key])
   */
  private computeNullifier(notePsi: string, secretKey: string): string {
    const bb = BarretenbergSync.getSingleton();
    const frs = [
      Fr.fromString("0x" + notePsi),
      Fr.fromString("0x" + secretKey),
    ];
    const result = bb.poseidon2Hash(frs);
    return result.toString().slice(2).toLowerCase();
  }

  /**
   * Compute messages based on UTXO kind
   * For Send: [1, 0, 0, 0, 0]
   */
  private computeMessages(kind: UtxoKind): string[] {
    switch (kind) {
      case UtxoKind.Send:
        return [
          "0".repeat(63) + "1", // Element::new(1)
          "0".repeat(64),       // Element::ZERO
          "0".repeat(64),       // Element::ZERO
          "0".repeat(64),       // Element::ZERO
          "0".repeat(64),       // Element::ZERO
        ];
      default:
        // For now only Send is supported in transfers
        throw new Error(`Unsupported UTXO kind: ${kind}`);
    }
  }

  /**
   * Build the 128-byte blob from input notes
   * Format: [input_commit_0, input_commit_1, nullifier_0, nullifier_1]
   */
  buildBlobData(
    inputNotes: [InputNoteData, InputNoteData],
    _outputNotes: [Note, Note],  // Not used in blob, but kept for API consistency
    contractName: string,
    blobIndex: number,
    blobCount: number
  ): BlobData {
    const blob = new Uint8Array(HYLI_BLOB_LENGTH_BYTES);
    let offset = 0;

    // Input commitments (first 64 bytes)
    for (const inputNote of inputNotes) {
      const commitment = this.computeNoteCommitment(inputNote.note);
      const commitmentBytes = this.hexToBytes(commitment);
      blob.set(commitmentBytes, offset);
      offset += 32;
    }

    // Nullifiers (next 64 bytes)
    for (const inputNote of inputNotes) {
      const nullifier = this.computeNullifier(inputNote.note.psi, inputNote.secret_key);
      const nullifierBytes = this.hexToBytes(nullifier);
      blob.set(nullifierBytes, offset);
      offset += 32;
    }

    // Build identity string (e.g., "transfer@hyli_utxo")
    const identity = `transfer@${contractName}`;

    // Placeholder tx_hash (will be replaced after blob submission)
    const txHash = "0".repeat(64);

    return {
      blob,
      contractName,
      identity,
      txHash,
      blobCount,
      blobIndex,
    };
  }

  /**
   * Pad a string to target length with null characters
   */
  private padString(value: string, targetLen: number): string {
    if (value.length > targetLen) {
      throw new Error(`String '${value}' exceeds maximum length ${targetLen}`);
    }
    return value.padEnd(targetLen, "\0");
  }

  /**
   * Convert hex string to bytes
   */
  private hexToBytes(hex: string): Uint8Array {
    const normalized = hex.startsWith("0x") ? hex.slice(2) : hex;
    const bytes = new Uint8Array(normalized.length / 2);
    for (let i = 0; i < bytes.length; i++) {
      bytes[i] = parseInt(normalized.substr(i * 2, 2), 16);
    }
    return bytes;
  }

  /**
   * Convert Uint8Array to base64 string
   */
  private uint8ArrayToBase64(bytes: Uint8Array): string {
    let binary = "";
    for (let i = 0; i < bytes.length; i++) {
      binary += String.fromCharCode(bytes[i]);
    }
    return btoa(binary);
  }
}

export const proverService = new ProverService();
