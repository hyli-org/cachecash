import { addStoredNote } from "./noteStorage";
import { StoredNote } from "../types/note";

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

class NodeService {
    private readonly baseUrl: string;

    constructor() {
        this.baseUrl = import.meta.env.VITE_SERVER_BASE_URL;
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

    async requestFaucet(playerName: string, amount?: number): Promise<FaucetResponse> {
        const trimmedName = playerName.trim();
        if (!trimmedName) {
            throw new Error("Player name must not be empty");
        }

        const payload: Record<string, unknown> = { name: trimmedName };
        if (typeof amount === "number") {
            payload.amount = amount;
        }

        const data = await this.request<FaucetResponse>("/api/faucet", {
            method: "POST",
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

        if (hasTxHash || hasNote) {
            const reference = hasTxHash ? data.tx_hash : deriveNoteReference(data.note);
            if (reference) {
                const stored: StoredNote = {
                    txHash: reference,
                    note: hasNote ? data.note : data,
                    storedAt: Date.now(),
                    player: trimmedName,
                };
                addStoredNote(trimmedName, stored);
            }
        }

        return data;
    }
}

export const nodeService = new NodeService();

function deriveNoteReference(note: MaybeFaucetNote | null | undefined): string | undefined {
    if (!note || typeof note !== "object") {
        return undefined;
    }

    const candidates = [note.psi, note.address, note.contract, note.value, note.kind];
    return candidates.find((candidate): candidate is string => typeof candidate === "string" && candidate.length > 0);
}
