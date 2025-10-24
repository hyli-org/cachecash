import { useMemo } from "react";

const DEBUG_ENV_FLAG = String(import.meta.env.VITE_DEBUG_MODE || "").toLowerCase();

function computeDebugFlag(): boolean {
    if (typeof window === "undefined") {
        return DEBUG_ENV_FLAG === "true" || DEBUG_ENV_FLAG === "1";
    }

    if (DEBUG_ENV_FLAG === "true" || DEBUG_ENV_FLAG === "1") {
        return true;
    }

    const params = new URLSearchParams(window.location.search);
    return params.has("debug");
}

export function useDebugMode(): boolean {
    return useMemo(() => computeDebugFlag(), []);
}
