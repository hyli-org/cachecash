interface FaucetResponse {
    name: string;
    key_pair: {
        private_key_hex: string;
        public_key_hex: string;
    };
    contract_name: string;
    amount: number;
    tx_hash: string;
    transaction: unknown;
    utxo: unknown;
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

        if (!data || typeof data.tx_hash !== "string") {
            throw new Error("Unexpected faucet response");
        }

        return data;
    }
}

export const nodeService = new NodeService();
