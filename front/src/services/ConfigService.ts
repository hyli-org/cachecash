interface RuntimeEnv {
    SERVER_BASE_URL?: string;
    NODE_BASE_URL?: string;
    WALLET_SERVER_BASE_URL?: string;
    INDEXER_BASE_URL?: string;
    DEBUG_MODE?: string;
}

function getRuntimeEnv(): RuntimeEnv {
    return (window as any).__ENV__ ?? {};
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

export function getServerBaseUrl(): string {
    const runtime = getRuntimeEnv();
    return isValidUrl(runtime.SERVER_BASE_URL)
        ? runtime.SERVER_BASE_URL!
        : import.meta.env.VITE_SERVER_BASE_URL || "http://localhost:9002";
}

export function getDebugMode(): boolean {
    const runtime = getRuntimeEnv();
    const flag = String(runtime.DEBUG_MODE || import.meta.env.VITE_DEBUG_MODE || "").toLowerCase();
    return flag === "true" || flag === "1";
}

export function getNodeBaseUrl(): string {
    const runtime = getRuntimeEnv();
    return isValidUrl(runtime.NODE_BASE_URL)
        ? runtime.NODE_BASE_URL!
        : import.meta.env.VITE_NODE_BASE_URL || "http://localhost:4321";
}

export function getWalletServerBaseUrl(): string {
    const runtime = getRuntimeEnv();
    return isValidUrl(runtime.WALLET_SERVER_BASE_URL)
        ? runtime.WALLET_SERVER_BASE_URL!
        : import.meta.env.VITE_WALLET_SERVER_BASE_URL || "http://localhost:4000";
}

export function getApplicationWsUrl(): string {
    return import.meta.env.VITE_APPLICATION_WS_URL || "ws://localhost:4000";
}

export function getIndexerBaseUrl(): string {
    const runtime = getRuntimeEnv();
    return isValidUrl(runtime.INDEXER_BASE_URL)
        ? runtime.INDEXER_BASE_URL!
        : import.meta.env.VITE_INDEXER_BASE_URL || getNodeBaseUrl();
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
