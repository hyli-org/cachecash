interface RuntimeEnv {
    SERVER_BASE_URL?: string;
    NODE_BASE_URL?: string;
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

let contractNameCache: Promise<string> | null = null;

export function fetchContractName(): Promise<string> {
    if (!contractNameCache) {
        contractNameCache = fetch(`${getServerBaseUrl()}/api/config`)
            .then((r) => r.json())
            .then((data) => data.contract_name as string)
            .catch(() => "hyli_utxo");
    }
    return contractNameCache;
}
