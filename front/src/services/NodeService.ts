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

    async recordSlice(playerName: string): Promise<{ id: string; total?: number }> {
        const payload = { playerName };
        const data = await this.request<any>("/api/slices", {
            method: "POST",
            body: JSON.stringify(payload),
        });

        const fallbackId = `${playerName || "anonymous"}-${Date.now()}`;

        const id = typeof data?.id === "string" ? data.id : fallbackId;
        const total =
            typeof data?.total === "number"
                ? data.total
                : typeof data?.score === "number"
                ? data.score
                : undefined;

        return { id, total };
    }

    async fetchScore(playerName: string): Promise<number> {
        if (!playerName) {
            return 0;
        }

        const data = await this.request<any>(`/api/score/${encodeURIComponent(playerName)}`);

        if (typeof data === "number") {
            return data;
        }

        if (typeof data?.score === "number") {
            return data.score;
        }

        if (typeof data?.total === "number") {
            return data.total;
        }

        return 0;
    }

}

export const nodeService = new NodeService();
