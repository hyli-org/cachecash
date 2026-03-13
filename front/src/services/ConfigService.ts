interface RuntimeEnv {
    SERVER_BASE_URL?: string;
    NODE_BASE_URL?: string;
    WALLET_SERVER_BASE_URL?: string;
    WALLET_WEBSOCKET_URL?: string;
    APPLICATION_WS_URL?: string;
    INDEXER_BASE_URL?: string;
    ORANJ_STATE_INDEXER_URL?: string;
    DEBUG_MODE?: string;
}

function getRuntimeEnv(): RuntimeEnv {
    return (window as any).__ENV__ ?? {};
}

function getWindowLocation(): Location | null {
    if (typeof window === "undefined") {
        return null;
    }
    return window.location;
}

function isValidUrl(url: string | undefined): boolean {
    if (!url) return false;
    try {
        new URL(url);
        return true;
    } catch {
        return false;
    }
}

function isLocalHostname(hostname: string): boolean {
    return hostname === "localhost" || hostname === "127.0.0.1" || hostname === "::1";
}

function getRemoteOrigin(protocol?: string): string | null {
    const location = getWindowLocation();
    if (!location || isLocalHostname(location.hostname)) {
        return null;
    }
    return `${protocol ?? location.protocol}//${location.host}`;
}

function getConfiguredUrl(
    runtimeValue: string | undefined,
    buildValue: string | undefined,
    fallback: () => string,
): string {
    if (isValidUrl(runtimeValue)) {
        return runtimeValue!;
    }
    if (isValidUrl(buildValue)) {
        return buildValue!;
    }
    return fallback();
}

export function getServerBaseUrl(): string {
    const runtime = getRuntimeEnv();
    return getConfiguredUrl(runtime.SERVER_BASE_URL, import.meta.env.VITE_SERVER_BASE_URL, () => {
        return getRemoteOrigin() ?? "http://localhost:9002";
    });
}

export function getDebugMode(): boolean {
    const runtime = getRuntimeEnv();
    const flag = String(runtime.DEBUG_MODE || import.meta.env.VITE_DEBUG_MODE || "").toLowerCase();
    return flag === "true" || flag === "1";
}

export function getOranjIndexerUrl(): string {
    const runtime = getRuntimeEnv();
    return getConfiguredUrl(runtime.ORANJ_STATE_INDEXER_URL, import.meta.env.VITE_ORANJ_STATE_INDEXER_URL, () => {
        return "http://localhost:4322";
    });
}

export function getNodeBaseUrl(): string {
    const runtime = getRuntimeEnv();
    return getConfiguredUrl(runtime.NODE_BASE_URL, import.meta.env.VITE_NODE_BASE_URL, () => {
        return "http://localhost:4321";
    });
}

export function getWalletServerBaseUrl(): string {
    const runtime = getRuntimeEnv();
    return getConfiguredUrl(runtime.WALLET_SERVER_BASE_URL, import.meta.env.VITE_WALLET_SERVER_BASE_URL, () => {
        return getRemoteOrigin() ?? "http://localhost:4000";
    });
}

export function getWalletWebsocketUrl(): string {
    const runtime = getRuntimeEnv();
    return getConfiguredUrl(
        runtime.WALLET_WEBSOCKET_URL ?? runtime.APPLICATION_WS_URL,
        import.meta.env.VITE_WALLET_WEBSOCKET_URL ?? import.meta.env.VITE_APPLICATION_WS_URL,
        () => {
        const location = getWindowLocation();
        const protocol = location?.protocol === "https:" ? "wss:" : "ws:";
        return getRemoteOrigin(protocol) ?? "ws://localhost:4000";
        },
    );
}

export function getIndexerBaseUrl(): string {
    const runtime = getRuntimeEnv();
    return getConfiguredUrl(runtime.INDEXER_BASE_URL, import.meta.env.VITE_INDEXER_BASE_URL, () => {
        return getNodeBaseUrl();
    });
}

interface ServerConfig {
    contract_name: string;
    utxo_state_contract_name: string;
    smt_incl_proof_contract_name: string;
}

let configCache: Promise<ServerConfig> | null = null;

function fetchConfig(): Promise<ServerConfig> {
    if (!configCache) {
        configCache = fetch(`${getServerBaseUrl()}/api/config`)
            .then((r) => r.json())
            .catch(() => ({
                contract_name: "hyli_utxo",
                utxo_state_contract_name: "hyli-utxo-state",
                smt_incl_proof_contract_name: "hyli_smt_incl_proof",
            }));
    }
    return configCache;
}

export function fetchContractName(): Promise<string> {
    return fetchConfig().then((c) => c.contract_name);
}

export function fetchUtxoStateContractName(): Promise<string> {
    return fetchConfig().then((c) => c.utxo_state_contract_name);
}

export function fetchSmtContractName(): Promise<string> {
    return fetchConfig().then((c) => c.smt_incl_proof_contract_name);
}
