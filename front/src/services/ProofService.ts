import { Noir } from "@noir-lang/noir_js";
import { UltraHonkBackend } from "@aztec/bb.js";
import { PrivateNote } from "../types/note";
import { InputNoteData, BlobData } from "./TransferService";

function noteToCircuit(note: PrivateNote) {
    return {
        kind:    "0x" + note.contract,
        value:   "0x" + note.value,
        address: "0x" + note.address,
        psi:     "0x" + note.psi,
    };
}

class ProofService {
    private backend: UltraHonkBackend | null = null;
    private noir:    Noir | null = null;

    async initialize(): Promise<void> {
        if (this.backend && this.noir) return;
        const circuit = await fetch("/hyli_utxo.json").then((r) => r.json());
        this.backend = new UltraHonkBackend(circuit.bytecode);
        this.noir    = new Noir(circuit);
    }

    async generateProof(
        inputNotes:  [InputNoteData, InputNoteData],
        outputNotes: [PrivateNote, PrivateNote],
        blobData:    BlobData,
        commitments: [string, string, string, string],
        kind:        1 | 2 | 3
    ): Promise<{ proof: string; publicInputs: string[] }> {
        await this.initialize();

        const identity = blobData.identity; // "transfer@hyli_utxo" (18 chars)

        const inputs = {
            version:                1,
            initial_state_len:      4,
            initial_state:          [0, 0, 0, 0],
            next_state_len:         4,
            next_state:             [0, 0, 0, 0],
            identity_len:           identity.length,
            identity:               identity.padEnd(256, "\0"),
            tx_hash:                blobData.txHash,
            index:                  blobData.blobIndex,
            blob_number:            1,
            blob_index:             blobData.blobIndex,
            blob_contract_name_len: 9,
            blob_contract_name:     "hyli_utxo".padEnd(256, "\0"),
            blob_capacity:          128,
            blob_len:               128,
            blob:                   Array.from(blobData.blob),
            tx_blob_count:          blobData.blobCount,
            success:                true,
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

        const { witness }           = await this.noir!.execute(inputs);
        const { proof, publicInputs } = await this.backend!.generateProof(witness);

        const proofBase64 = btoa(String.fromCharCode(...proof));

        return { proof: proofBase64, publicInputs: publicInputs as string[] };
    }
}

export const proofService = new ProofService();
