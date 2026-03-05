import { PrivateNote, StoredNote } from "../types/note";
import { FullIdentity } from "./KeyService";
import { poseidon2Service } from "./Poseidon2Service";
import { encryptedNoteService } from "./EncryptedNoteService";
import { nodeService } from "./NodeService";
import { proofService } from "./ProofService";
import { smtProofService } from "./SmtProofService";
import { markNotesPending, clearPendingNotes, getPendingNotePsis, setStoredNotes, getStoredNotes, addStoredNote } from "./noteStorage";
import { fetchContractName, fetchUtxoStateContractName, fetchSmtContractName } from "./ConfigService";

/** An input note ready for proving */
export interface InputNoteData {
    note: PrivateNote;
    secretKey: string; // ZK secret key (64-char hex)
}

/** A spendable note for UI display (no secretKey) */
export interface SpendableNote {
    note: PrivateNote;
    value: number;
    txHash: string;
}

/** Result of note selection */
export interface NoteSelection {
    selectedInputs: [InputNoteData, InputNoteData];
    changeAmount: number;
    totalInput: number;
}

/** Transfer step reported via onProgress callback */
export type TransferStep =
    | "smt-witness"
    | "creating-blob"
    | "proving-utxo"
    | "proving-smt"
    | "submitting-proofs";

/** Blob data for proof generation */
export interface BlobData {
    blob: Uint8Array; // 128 bytes
    contractName: string; // "hyli_utxo"
    identity: string; // "transfer@hyli_utxo"
    txHash: string; // 64-char hex (filled in after /api/blob/create)
    blobCount: number; // 3
    blobIndex: number; // 1
}

// Padding address = poseidon2([0, 0], 2)
export const PADDING_ADDRESS = "0b63a53787021a4a962a452c2921b3663aff1ffd8d5510540f8e659e782956f1";

function createPaddingNote(): PrivateNote {
    return {
        kind: "0".repeat(64),
        contract: "0".repeat(64),
        address: "0".repeat(64),
        psi: "0".repeat(64),
        value: "0".repeat(64),
    };
}

function createPaddingInputNote(): InputNoteData {
    return { note: createPaddingNote(), secretKey: "0".repeat(64) };
}

function parseNoteValue(note: PrivateNote): number {
    const hex = note.value.replace(/^0x/i, "");
    if (!hex || hex === "0".repeat(64)) return 0;
    try {
        const n = parseInt(hex, 16);
        return isNaN(n) ? 0 : n;
    } catch {
        return 0;
    }
}

function toHex64(value: number): string {
    return value.toString(16).padStart(64, "0");
}

function hexToBytes32(hexStr: string): Uint8Array {
    const normalized = hexStr.replace(/^0x/i, "").padStart(64, "0");
    const bytes = new Uint8Array(32);
    for (let i = 0; i < 32; i++) {
        bytes[i] = parseInt(normalized.slice(i * 2, i * 2 + 2), 16);
    }
    return bytes;
}

/**
 * Note commitment: poseidon2([0x2, kind, value, address, psi, 0, 0], 7)
 * Returns zero field element when kind == 0 (padding note)
 */
async function computeCommitment(note: PrivateNote): Promise<string> {
    if (note.contract === "0".repeat(64)) {
        return "0".repeat(64);
    }
    const TWO = "0".repeat(63) + "2";
    const ZERO = "0".repeat(64);
    return poseidon2Service.hash([
        TWO,
        note.contract, // kind in circuit
        note.value,
        note.address,
        note.psi,
        ZERO,
        ZERO,
    ]);
}

/** Nullifier: poseidon2([psi, secretKey], 2) */
async function computeNullifier(psi: string, secretKey: string): Promise<string> {
    return poseidon2Service.hash([psi, secretKey]);
}

/**
 * Generate a random 32-byte value that is guaranteed to be a valid BN254
 * scalar field element (i.e. < p = 0x30644e72…).
 *
 * BN254 modulus starts with 0x30, so zeroing the top 4 bits gives a value
 * with its leading byte in [0x00, 0x0f] which is always < 0x30…, hence < p.
 * This wastes 4 bits of entropy but avoids any rejection-sampling loop.
 */
function randomFieldElement(): string {
    const bytes = new Uint8Array(32);
    crypto.getRandomValues(bytes);
    bytes[0] &= 0x0f; // ensure value < 2^252 < BN254 field modulus
    return Array.from(bytes)
        .map((b) => b.toString(16).padStart(2, "0"))
        .join("");
}

class TransferService {
    private createOutputNote(recipientAddress: string, amount: number, contract: string): PrivateNote {
        const psi = randomFieldElement();

        return {
            kind: contract,
            contract,
            address: recipientAddress,
            psi,
            value: toHex64(amount),
        };
    }

    selectNotesForTransfer(availableInputs: InputNoteData[], amount: number): NoteSelection | null {
        const withValues = availableInputs
            .map((input) => ({ input, value: parseNoteValue(input.note) }))
            .filter((n) => n.value > 0);

        const total = withValues.reduce((sum, n) => sum + n.value, 0);
        if (total < amount) return null;

        // Sort by value ascending
        const sorted = [...withValues].sort((a, b) => a.value - b.value);

        // Try single note first
        for (const item of sorted) {
            if (item.value >= amount) {
                return {
                    selectedInputs: [item.input, createPaddingInputNote()],
                    changeAmount: item.value - amount,
                    totalInput: item.value,
                };
            }
        }

        // Try combinations of 2 notes
        for (let i = 0; i < sorted.length - 1; i++) {
            for (let j = i + 1; j < sorted.length; j++) {
                const sum = sorted[i].value + sorted[j].value;
                if (sum >= amount) {
                    return {
                        selectedInputs: [sorted[i].input, sorted[j].input],
                        changeAmount: sum - amount,
                        totalInput: sum,
                    };
                }
            }
        }

        return null;
    }

    /**
     * Build 128-byte blob: [outputCommit0 (32), outputCommit1 (32), nullifier0 (32), nullifier1 (32)]
     */
    async buildRawBlobData(outputNotes: [PrivateNote, PrivateNote], inputNotes: [InputNoteData, InputNoteData]): Promise<Uint8Array> {
        const [outputCommit0, outputCommit1, nullifier0, nullifier1] = await Promise.all([
            computeCommitment(outputNotes[0]),
            computeCommitment(outputNotes[1]),
            computeNullifier(inputNotes[0].note.psi, inputNotes[0].secretKey),
            computeNullifier(inputNotes[1].note.psi, inputNotes[1].secretKey),
        ]);

        const blob = new Uint8Array(128);
        blob.set(hexToBytes32(outputCommit0), 0);
        blob.set(hexToBytes32(outputCommit1), 32);
        blob.set(hexToBytes32(nullifier0), 64);
        blob.set(hexToBytes32(nullifier1), 96);
        return blob;
    }

    /**
     * Execute complete transfer via two-step blob/proof flow
     */
    async executeTransfer(
        recipientUtxoAddress: string,
        amount: number,
        availableInputs: InputNoteData[],
        senderIdentity: FullIdentity,
        playerName: string,
        recipientEncryptionPubkey?: string,
        onProgress?: (step: TransferStep) => void,
    ): Promise<{ txHash: string; transferNote: PrivateNote }> {
        // 1. Select notes
        const selection = this.selectNotesForTransfer(availableInputs, amount);
        if (!selection) {
            const total = availableInputs.reduce((sum, n) => sum + parseNoteValue(n.note), 0);
            throw new Error(`Insufficient balance. You have ${total} but need ${amount}`);
        }

        // 2. Mark notes as pending (by psi) to prevent double-spend
        const spentPsis = selection.selectedInputs.filter((n) => parseNoteValue(n.note) > 0).map((n) => n.note.psi);
        markNotesPending(playerName, spentPsis);

        try {
            const contract = selection.selectedInputs[0].note.contract;

            // Create output notes
            const transferNote = this.createOutputNote(recipientUtxoAddress, amount, contract);
            const changeNote =
                selection.changeAmount > 0
                    ? this.createOutputNote(senderIdentity.utxoAddress, selection.changeAmount, contract)
                    : createPaddingNote();
            const outputNotes: [PrivateNote, PrivateNote] = [transferNote, changeNote];

            // Compute input commitments (needed for SMT witness lookup)
            const [commit0, commit1] = await Promise.all([
                computeCommitment(selection.selectedInputs[0].note),
                computeCommitment(selection.selectedInputs[1].note),
            ]);

            // 3. Build raw blob (output commitments + nullifiers for inputs)
            const blobBytes = await this.buildRawBlobData(outputNotes, selection.selectedInputs);

            // 4. Fetch SMT witnesses from the server indexer
            onProgress?.("smt-witness");
            const [contractName, utxoStateContractName, smtContractName] = await Promise.all([
                fetchContractName(),
                fetchUtxoStateContractName(),
                fetchSmtContractName(),
            ]);
            const smtWitness = await nodeService.getSmtWitness(commit0, commit1, utxoStateContractName);

            // Build SMT blob: [nullifier0 (32B)][nullifier1 (32B)][notes_root (32B)] = 96 bytes
            // Nullifiers are at bytes 64-127 of the UTXO blob (blobBytes).
            const smtBlobBytes = new Uint8Array(96);
            smtBlobBytes.set(blobBytes.slice(64, 96), 0);
            smtBlobBytes.set(blobBytes.slice(96, 128), 32);
            smtBlobBytes.set(hexToBytes32(smtWitness.notes_root), 64);

            // 5. Compute deterministic tx_hash without submitting (so proofs use the real hash)
            onProgress?.("creating-blob");
            const { tx_hash: txHash } = await nodeService.hashBlob(blobBytes, smtBlobBytes, outputNotes);

            // 6. Compute all 4 commitments for the hyli_utxo proof
            const [commit2, commit3] = await Promise.all([
                computeCommitment(outputNotes[0]),
                computeCommitment(outputNotes[1]),
            ]);
            const commitments: [string, string, string, string] = [commit0, commit1, commit2, commit3];

            const blobData: BlobData = {
                blob: blobBytes,
                contractName,
                identity: `transfer@${contractName}`,
                txHash,
                blobCount: 3,
                blobIndex: 1,
            };

            // 7. Generate ZK proofs sequentially (parallel execution causes WASM heap corruption)
            onProgress?.("proving-utxo");
            const utxoResult = await proofService.generateProof(
                selection.selectedInputs,
                outputNotes,
                blobData,
                commitments,
                1, // kind = 1 (transfer)
            );
            onProgress?.("proving-smt");
            const smtResult = await smtProofService.generateProof({
                smtBlobBytes,
                contractName: smtContractName,
                identity: `transfer@${contractName}`,
                txHash,
                blobCount: 3,
                inputNotes: selection.selectedInputs.map((n) => n.note) as [PrivateNote, PrivateNote],
                secretKeys: selection.selectedInputs.map((n) => n.secretKey) as [string, string],
                siblings0: smtWitness.siblings_0,
                siblings1: smtWitness.siblings_1,
            });

            // 8. Submit blob tx + both proofs atomically
            onProgress?.("submitting-proofs");
            await nodeService.finalizeTransfer(
                blobBytes,
                smtBlobBytes,
                outputNotes,
                utxoResult.proof,
                utxoResult.publicInputs,
                smtResult.proof,
                smtResult.publicInputs,
            );

            // 9. Update stored notes: remove spent, add change note
            const currentNotes = getStoredNotes(playerName);
            const spentPsiSet = new Set(spentPsis);
            let updatedNotes = currentNotes.filter((stored) => {
                const note = stored.note as PrivateNote;
                return !spentPsiSet.has(note?.psi ?? "");
            });

            if (selection.changeAmount > 0) {
                const storedChangeNote: StoredNote = {
                    txHash: `change:${txHash}`,
                    note: changeNote,
                    storedAt: Date.now(),
                    player: playerName,
                };
                updatedNotes = [storedChangeNote, ...updatedNotes];
            }

            setStoredNotes(playerName, updatedNotes);

            // Clear pending state
            clearPendingNotes(playerName, spentPsis);

            // 10. Upload encrypted note for recipient (best effort)
            if (recipientEncryptionPubkey) {
                try {
                    await encryptedNoteService.uploadNote(
                        recipientUtxoAddress,
                        recipientEncryptionPubkey,
                        {
                            note: transferNote,
                            tx_hash: txHash,
                            amount,
                            from: senderIdentity.publicKey,
                            timestamp: Date.now(),
                        },
                        senderIdentity,
                    );
                } catch (err) {
                    console.warn("Failed to upload encrypted note for recipient:", err);
                }
            }

            return { txHash, transferNote };
        } catch (error) {
            clearPendingNotes(playerName, spentPsis);
            throw error;
        }
    }

    /**
     * Returns true when no 2-note combination covers `amount` but the total balance does.
     * In that case, notes must be consolidated before the transfer can proceed.
     */
    needsConsolidation(availableInputs: InputNoteData[], amount: number): boolean {
        const withValues = availableInputs
            .map((input) => ({ input, value: parseNoteValue(input.note) }))
            .filter((n) => n.value > 0);
        const total = withValues.reduce((sum, n) => sum + n.value, 0);
        if (total < amount) return false; // genuinely insufficient – not a consolidation problem
        return this.selectNotesForTransfer(availableInputs, amount) === null;
    }

    /**
     * Returns the two notes with the highest values (candidates for merging).
     */
    notesForConsolidation(availableInputs: InputNoteData[]): [InputNoteData, InputNoteData] | null {
        const withValues = availableInputs
            .map((input) => ({ input, value: parseNoteValue(input.note) }))
            .filter((n) => n.value > 0)
            .sort((a, b) => b.value - a.value); // descending
        if (withValues.length < 2) return null;
        return [withValues[0].input, withValues[1].input];
    }

    /**
     * Merge two notes into one via a self-transfer, then persist the resulting note locally.
     */
    async executeConsolidation(
        pair: [InputNoteData, InputNoteData],
        senderIdentity: FullIdentity,
        playerName: string,
        onProgress?: (step: TransferStep) => void,
    ): Promise<void> {
        const amount = parseNoteValue(pair[0].note) + parseNoteValue(pair[1].note);
        const { txHash, transferNote } = await this.executeTransfer(
            senderIdentity.utxoAddress,
            amount,
            pair,
            senderIdentity,
            playerName,
            undefined,
            onProgress,
        );
        addStoredNote(playerName, {
            txHash: `consolidation:${txHash}`,
            note: transferNote,
            storedAt: Date.now(),
            player: playerName,
        });
    }

    /**
     * Execute a transfer, automatically consolidating fragmented notes beforehand if needed.
     * `onConsolidating(step)` is called at the start of each consolidation round.
     */
    async executeTransferWithConsolidation(
        recipientUtxoAddress: string,
        amount: number,
        availableInputs: InputNoteData[],
        senderIdentity: FullIdentity,
        playerName: string,
        recipientEncryptionPubkey?: string,
        onConsolidating?: (step: number) => void,
        onProgress?: (step: TransferStep) => void,
    ): Promise<{ txHash: string; transferNote: PrivateNote }> {
        let currentInputs = [...availableInputs];
        let step = 0;

        while (this.needsConsolidation(currentInputs, amount)) {
            step++;
            onConsolidating?.(step);

            const pair = this.notesForConsolidation(currentInputs);
            if (!pair) break; // shouldn't happen – needsConsolidation already verified 2+ notes

            await this.executeConsolidation(pair, senderIdentity, playerName, onProgress);

            // Reload from storage so the freshly merged note is visible
            currentInputs = this.getSpendableNotes(
                getStoredNotes(playerName),
                senderIdentity.zkSecretKey,
                playerName,
            );
        }

        return this.executeTransfer(
            recipientUtxoAddress,
            amount,
            currentInputs,
            senderIdentity,
            playerName,
            recipientEncryptionPubkey,
            onProgress,
        );
    }

    /**
     * Merge all notes down to one by repeatedly consolidating the two largest.
     * Returns the number of consolidation rounds performed.
     * `onStep(step, total)` is called before each round.
     */
    async consolidateAll(
        availableInputs: InputNoteData[],
        senderIdentity: FullIdentity,
        playerName: string,
        onStep?: (step: number, total: number) => void,
        onProgress?: (step: TransferStep) => void,
    ): Promise<number> {
        let currentInputs = [...availableInputs];
        const total = Math.max(0, currentInputs.length - 1); // rounds needed
        let step = 0;

        while (currentInputs.length >= 2) {
            step++;
            onStep?.(step, total);

            const pair = this.notesForConsolidation(currentInputs);
            if (!pair) break;

            await this.executeConsolidation(pair, senderIdentity, playerName, onProgress);

            currentInputs = this.getSpendableNotes(
                getStoredNotes(playerName),
                senderIdentity.zkSecretKey,
                playerName,
            );
        }

        return step;
    }

    /**
     * Get all spendable input notes for a player (excluding pending and zero-value)
     */
    getSpendableNotes(storedNotes: StoredNote[], zkSecretKey: string, playerName: string): InputNoteData[] {
        const pendingPsis = getPendingNotePsis(playerName);

        return storedNotes
            .filter((stored) => {
                const note = stored.note as PrivateNote & { status?: string };
                // Exclude optimistic notes (zero-value placeholder)
                if (note?.status === "optimistic") return false;
                // Exclude pending notes by psi
                const psi = note?.psi;
                if (psi && pendingPsis.has(psi)) return false;
                return true;
            })
            .map((stored) => {
                const raw = stored.note;
                const note: PrivateNote = {
                    kind: raw.kind || raw.contract || "0".repeat(64),
                    contract: raw.contract || "0".repeat(64),
                    address: raw.address || "0".repeat(64),
                    psi: raw.psi || "0".repeat(64),
                    value: (() => {
                        const v = raw.value;
                        if (!v) return "0".repeat(64);
                        const hex = v.replace(/^0x/i, "");
                        if (/^[0-9a-fA-F]+$/.test(hex)) {
                            return hex.padStart(64, "0");
                        }
                        return parseInt(v, 10).toString(16).padStart(64, "0");
                    })(),
                };
                return { note, secretKey: zkSecretKey };
            })
            .filter((input) => parseNoteValue(input.note) > 0);
    }
}

export const transferService = new TransferService();
export { parseNoteValue };
