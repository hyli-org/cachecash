import { Noir } from "@noir-lang/noir_js";
import { UltraHonkBackend } from "@aztec/bb.js";
import { PrivateNote } from "../types/note";

class SmtProofService {
    // Cache only the circuit JSON; backend is created fresh for each proof
    // to avoid bb.js WASM singleton state corruption between sequential proofs.
    private circuit: object | null = null;

    private async loadCircuit(): Promise<object> {
        if (!this.circuit) {
            this.circuit = await fetch("/hyli_smt_incl_proof.json").then((r) => r.json());
        }
        return this.circuit!;
    }

    async generateProof(params: {
        smtBlobBytes: Uint8Array; // 96 bytes: [nullifier0, nullifier1, notes_root]
        contractName: string; // smt_incl_proof_contract_name
        identity: string; // "transfer@{utxo_contract_name}"
        txHash: string;
        blobCount: number; // 3
        inputNotes: [PrivateNote, PrivateNote]; // private: used to compute commitments for SMT lookup
        secretKeys: [string, string]; // private: used to compute nullifiers
        siblings0: string[]; // 256 "0x..." hex field elements
        siblings1: string[]; // 256 "0x..." hex field elements
    }): Promise<{ proof: string; publicInputs: string[] }> {
        const circuit = await this.loadCircuit();
        const backend = new UltraHonkBackend((circuit as any).bytecode);
        const noir = new Noir(circuit as any);

        const toCircuitNote = (note: PrivateNote) => ({
            kind:    "0x" + note.contract,
            value:   "0x" + note.value,
            address: "0x" + note.address,
            psi:     "0x" + note.psi,
        });

        try {
            const inputs = {
                version: 1,
                initial_state_len: 4,
                initial_state: [0, 0, 0, 0],
                next_state_len: 4,
                next_state: [0, 0, 0, 0],
                identity_len: params.identity.length,
                identity: params.identity.padEnd(256, "\0"),
                tx_hash: params.txHash,
                index: 2,
                blob_number: 1,
                blob_index: 2,
                blob_contract_name_len: params.contractName.length,
                blob_contract_name: params.contractName.padEnd(256, "\0"),
                blob_capacity: 96,
                blob_len: 96,
                blob: Array.from(params.smtBlobBytes),
                tx_blob_count: params.blobCount,
                success: true,
                input_notes: [
                    { note: toCircuitNote(params.inputNotes[0]), secret_key: "0x" + params.secretKeys[0] },
                    { note: toCircuitNote(params.inputNotes[1]), secret_key: "0x" + params.secretKeys[1] },
                ],
                siblings_0: params.siblings0,
                siblings_1: params.siblings1,
            };

            const { witness } = await noir.execute(inputs);
            console.log("Witness generated successfully, now generating proof...");
            const { proof, publicInputs } = await backend.generateProof(witness);

            const proofBase64 = btoa(String.fromCharCode(...proof));
            return { proof: proofBase64, publicInputs: publicInputs as string[] };
        } catch (error) {
            console.error("Error generating SMT inclusion proof:", error);
            throw error;
        } finally {
            await backend.destroy();
        }
    }
}

export const smtProofService = new SmtProofService();
