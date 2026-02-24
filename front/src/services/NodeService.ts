import { PrivateNote } from "../types/note";
import { getServerBaseUrl } from "./ConfigService";

type MaybeFaucetNote = {
    kind?: string;
    contract?: string;
    address?: string;
    psi?: string;
    value?: string;
    [key: string]: unknown;
};

interface FaucetResponse {
    name?: string;
    key_pair?: {
        private_key_hex: string;
        public_key_hex: string;
    };
    contract_name?: string;
    amount?: number;
    tx_hash?: string;
    transaction?: unknown;
    note?: MaybeFaucetNote | null;
    [key: string]: unknown;
}

export interface CreateBlobResponse {
    tx_hash: string;
    blobs: Array<{ contract_name: string; data: string }>;
}

class NodeService {
    private readonly baseUrl: string;

    constructor() {
        this.baseUrl = getServerBaseUrl();
    }

    private buildUrl(path: string): string {
        const normalizedBase = this.baseUrl?.replace(/\/$/, "") ?? "";
        const normalizedPath = path.startsWith("/") ? path : `/${path}`;
        return `${normalizedBase}${normalizedPath}`;
    }

    private async request<T>(path: string, options: RequestInit = {}): Promise<T | undefined> {
        const headers = new Headers(options.headers || {});
        if (options.body !== undefined && !headers.has("Content-Type")) {
            headers.set("Content-Type", "application/json");
        }

        const response = await fetch(this.buildUrl(path), {
            ...options,
            headers,
        });

        if (!response.ok) {
            throw new Error(`Request failed with status ${response.status}`);
        }

        if (response.status === 204) {
            return undefined;
        }

        try {
            return (await response.json()) as T;
        } catch (_error) {
            return undefined;
        }
    }

    async requestFaucet(utxoAddressHex: string, amount?: number): Promise<FaucetResponse> {
        const normalized = utxoAddressHex.trim().replace(/^0x/i, "");
        if (normalized.length === 0) {
            throw new Error("UTXO address must not be empty");
        }
        if (normalized.length !== 64) {
            throw new Error("UTXO address must be a 32-byte hex string");
        }

        const payload: Record<string, unknown> = {
            pubkey_hex: normalized,
        };
        if (typeof amount === "number") {
            payload.amount = amount;
        }

        const data = await this.request<FaucetResponse>("/api/faucet", {
            method: "POST",
            headers: {
                "X-Pubkey": normalized,
            },
            body: JSON.stringify(payload),
        });

        if (!data) {
            throw new Error("Unexpected faucet response");
        }

        const hasTxHash = typeof data.tx_hash === "string" && data.tx_hash.length > 0;
        const hasNote = data.note !== undefined && data.note !== null;

        if (!hasTxHash && !hasNote) {
            throw new Error("Unexpected faucet response");
        }

        return data;
    }

    /**
     * POST /api/blob/create
     * Submits the raw blob (commitments + nullifiers) and output notes.
     * Returns tx_hash and blob info.
     */
    async createBlob(
        blobBytes: Uint8Array,
        outputNotes: [PrivateNote, PrivateNote]
    ): Promise<CreateBlobResponse> {
        const data = await this.request<CreateBlobResponse>("/api/blob/create", {
            method: "POST",
            body: JSON.stringify({
                blob_data:    Array.from(blobBytes),
                output_notes: outputNotes,
            }),
        });

        if (!data) {
            throw new Error("Unexpected empty response from /api/blob/create");
        }

        return data;
    }

    /**
     * POST /api/proof/submit
     * Submits the generated proof for the transaction.
     */
    async submitProof(
        txHash: string,
        proof: string,
        publicInputs: string[]
    ): Promise<void> {
        await this.request("/api/proof/submit", {
            method: "POST",
            body: JSON.stringify({
                tx_hash:       txHash,
                proof,
                public_inputs: publicInputs,
            }),
        });
    }
}

export const nodeService = new NodeService();
