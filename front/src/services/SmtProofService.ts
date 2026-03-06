import { Noir } from "@noir-lang/noir_js";
import { UltraHonkBackend } from "@aztec/bb.js";

/**
 * Convert a 32-byte LE array to a "0x..." hex field string (BE).
 */
function siblingsToFields(siblings: number[][]): string[] {
    return siblings.map((sib) => {
        // Reverse LE bytes to BE and format as hex
        const be = new Uint8Array(32);
        for (let i = 0; i < 32; i++) be[31 - i] = sib[i];
        return "0x" + Array.from(be).map((b) => b.toString(16).padStart(2, "0")).join("");
    });
}

class SmtProofService {
    // Cache only the circuit JSON; backend is created fresh for each proof
    // to avoid bb.js WASM singleton state corruption between sequential proofs.
    private circuit: object | null = null;

    private async loadCircuit(): Promise<object> {
        if (!this.circuit) {
            this.circuit = await fetch("/hyli_smt_incl_proof.json").then((r) => r.json());
        }
        return this.circuit;
    }

    async generateProof(params: {
        smtBlobBytes: Uint8Array; // 96 bytes: [commit0, commit1, notes_root]
        contractName: string; // smt_incl_proof_contract_name
        identity: string; // "transfer@{utxo_contract_name}"
        txHash: string;
        blobCount: number; // 3
        siblings0: number[][]; // 256 x 32
        siblings1: number[][]; // 256 x 32
    }): Promise<{ proof: string; publicInputs: string[] }> {
        const circuit = await this.loadCircuit();
        const backend = new UltraHonkBackend((circuit as any).bytecode);
        const noir = new Noir(circuit as any);

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
                siblings_0: siblingsToFields(params.siblings0),
                siblings_1: siblingsToFields(params.siblings1),
            };

            // Debug: dump blob bytes and ALL non-zero siblings
            const blobHex = Array.from(params.smtBlobBytes).map((b) => b.toString(16).padStart(2, "0")).join("");
            console.log("SMT blob commit0:", blobHex.slice(0, 64));
            console.log("SMT blob commit1:", blobHex.slice(64, 128));
            console.log("SMT blob root:   ", blobHex.slice(128, 192));
            for (let i = 0; i < 256; i++) {
                if (params.siblings0[i].some((b: number) => b !== 0)) {
                    const leHex = params.siblings0[i].map((b: number) => b.toString(16).padStart(2, "0")).join("");
                    console.log(`SMT sib0[${i}] LE: ${leHex}, field: ${siblingsToFields([params.siblings0[i]])[0]}`);
                }
            }
            for (let i = 0; i < 256; i++) {
                if (params.siblings1[i].some((b: number) => b !== 0)) {
                    const leHex = params.siblings1[i].map((b: number) => b.toString(16).padStart(2, "0")).join("");
                    console.log(`SMT sib1[${i}] LE: ${leHex}, field: ${siblingsToFields([params.siblings1[i]])[0]}`);
                }
            }
            console.log("Generating SMT inclusion proof with inputs:", inputs);
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
