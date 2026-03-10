import { Noir } from "@noir-lang/noir_js";
import { UltraHonkBackend } from "@aztec/bb.js";
import { PrivateNote } from "../types/note";

const HYLI_IDENTITY_MAX = 256;
const HYLI_SMT_INCL_PAYLOAD_LENGTH = 96;
const HYLI_SMT_INCL_STRUCTURED_BLOB_LENGTH = 110;

function encodeLeU64(value: number): number[] {
    if (!Number.isInteger(value) || value < 0) {
        throw new Error(`u64 value must be a non-negative integer, got ${value}`);
    }
    const bytes = new Array<number>(8).fill(0);
    let remaining = value;
    for (let i = 0; i < 8; i++) {
        bytes[i] = remaining & 0xff;
        remaining = Math.floor(remaining / 256);
    }
    return bytes;
}

function encodeLeU32(value: number): number[] {
    if (!Number.isInteger(value) || value < 0 || value > 0xffffffff) {
        throw new Error(`u32 value out of range: ${value}`);
    }
    return [
        value & 0xff,
        (value >>> 8) & 0xff,
        (value >>> 16) & 0xff,
        (value >>> 24) & 0xff,
    ];
}

function buildStructuredSmtBlob(payload: Uint8Array): number[] {
    if (payload.length !== HYLI_SMT_INCL_PAYLOAD_LENGTH) {
        throw new Error(
            `SMT payload must be ${HYLI_SMT_INCL_PAYLOAD_LENGTH} bytes, got ${payload.length}`,
        );
    }

    const blob = [
        1, // caller = Some(...)
        ...encodeLeU64(0), // BlobIndex(0)
        0, // callees = None
        ...encodeLeU32(payload.length),
        ...Array.from(payload),
    ];

    if (blob.length !== HYLI_SMT_INCL_STRUCTURED_BLOB_LENGTH) {
        throw new Error(
            `Structured SMT blob must be ${HYLI_SMT_INCL_STRUCTURED_BLOB_LENGTH} bytes, got ${blob.length}`,
        );
    }

    return blob;
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
            const blob = buildStructuredSmtBlob(params.smtBlobBytes);
            const inputs = {
                hyli_output: {
                    version:             2,
                    initial_state_len:   4,
                    initial_state_max:   4,
                    initial_state:       [0, 0, 0, 0],
                    next_state_len:      4,
                    next_state_max:      4,
                    next_state:          [0, 0, 0, 0],
                    identity_len:        params.identity.length,
                    identity_max:        HYLI_IDENTITY_MAX,
                    identity:            params.identity.padEnd(HYLI_IDENTITY_MAX, "\0"),
                    index:               2,
                    blob_count:          1,
                    blob_slots:          1,
                    blob_name_max:       256,
                    blob_data_max:       HYLI_SMT_INCL_STRUCTURED_BLOB_LENGTH,
                    blobs:               [{
                        index:             2,
                        contract_name_len: params.contractName.length,
                        contract_name:     params.contractName.padEnd(256, "\0"),
                        data_len:          blob.length,
                        data:              blob,
                    }],
                    tx_blob_count:       params.blobCount,
                    tx_hash:             txHashToBytes32(params.txHash),
                    success:             true,
                    program_outputs_max: 5,
                    program_outputs_len: 0,
                    program_outputs:     [0, 0, 0, 0, 0],
                },
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
