import { Noir } from "@noir-lang/noir_js";
import { UltraHonkBackend } from "@aztec/bb.js";
import { PrivateNote } from "../types/note";
import { InputNoteData, BlobData } from "./TransferService";

const HYLI_IDENTITY_MAX = 256;

function noteToCircuit(note: PrivateNote) {
    return {
        kind:    "0x" + note.contract,
        value:   "0x" + note.value,
        address: "0x" + note.address,
        psi:     "0x" + note.psi,
    };
}

function txHashToBytes32(txHash: string): number[] {
    const normalized = txHash.startsWith("0x") ? txHash.slice(2) : txHash;
    if (normalized.length !== 64) {
        throw new Error(`tx_hash must be 64 hex chars, got ${normalized.length}`);
    }
    const bytes: number[] = [];
    for (let i = 0; i < 64; i += 2) {
        const byte = Number.parseInt(normalized.slice(i, i + 2), 16);
        if (Number.isNaN(byte)) {
            throw new Error("tx_hash contains non-hex characters");
        }
        bytes.push(byte);
    }
    return bytes;
}

class ProofService {
    // Cache only the circuit JSON; backend is created fresh for each proof
    // to avoid bb.js WASM singleton state corruption between sequential proofs.
    private circuit: object | null = null;

    private async loadCircuit(): Promise<object> {
        if (!this.circuit) {
            this.circuit = await fetch("/hyli_utxo.json").then((r) => r.json());
        }
        return this.circuit!;
    }

    async generateProof(
        inputNotes:  [InputNoteData, InputNoteData],
        outputNotes: [PrivateNote, PrivateNote],
        blobData:    BlobData,
        commitments: [string, string, string, string],
        kind:        1 | 2 | 3
    ): Promise<{ proof: string; publicInputs: string[] }> {
        const circuit = await this.loadCircuit();
        const backend = new UltraHonkBackend((circuit as any).bytecode);
        const noir    = new Noir(circuit as any);

        try {
            const identity = blobData.identity; // "transfer@hyli_utxo" (18 chars)
            const blob = Array.from(blobData.blob);

            const inputs = {
                hyli_output: {
                    version:             2,
                    initial_state_len:   4,
                    initial_state_max:   4,
                    initial_state:       [0, 0, 0, 0],
                    next_state_len:      4,
                    next_state_max:      4,
                    next_state:          [0, 0, 0, 0],
                    identity_len:        identity.length,
                    identity_max:        HYLI_IDENTITY_MAX,
                    identity:            identity.padEnd(HYLI_IDENTITY_MAX, "\0"),
                    index:               blobData.blobIndex,
                    blob_count:          1,
                    blob_slots:          1,
                    blob_name_max:       256,
                    blob_data_max:       128,
                    blobs:               [{
                        index:             blobData.blobIndex,
                        contract_name_len: blobData.contractName.length,
                        contract_name:     blobData.contractName.padEnd(256, "\0"),
                        data_len:          blob.length,
                        data:              blob,
                    }],
                    tx_blob_count:       blobData.blobCount,
                    tx_hash:             txHashToBytes32(blobData.txHash),
                    success:             true,
                    program_outputs_max: 5,
                    program_outputs_len: 0,
                    program_outputs:     [0, 0, 0, 0, 0],
                },
                input_notes: [
                    {
                        note:       noteToCircuit(inputNotes[0].note),
                        secret_key: "0x" + inputNotes[0].secretKey,
                    },
                    {
                        note:       noteToCircuit(inputNotes[1].note),
                        secret_key: "0x" + inputNotes[1].secretKey,
                    },
                ],
                output_notes: [
                    noteToCircuit(outputNotes[0]),
                    noteToCircuit(outputNotes[1]),
                ],
                pmessage4:   "0x" + "0".repeat(64),
                commitments: commitments.map((h) => "0x" + h),
                messages:    [
                    "0x" + kind.toString(16),
                    "0x0",
                    "0x0",
                    "0x0",
                    "0x0",
                ],
            };

            const { witness }             = await noir.execute(inputs);
            const { proof, publicInputs } = await backend.generateProof(witness);

            const proofBase64 = btoa(String.fromCharCode(...proof));
            return { proof: proofBase64, publicInputs: publicInputs as string[] };
        } finally {
            await backend.destroy();
        }
    }
}

export const proofService = new ProofService();
