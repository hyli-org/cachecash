import { useMemo } from "react";
import { getDebugMode } from "../services/ConfigService";

function computeDebugFlag(): boolean {
    if (getDebugMode()) return true;
    if (typeof window === "undefined") return false;
    const params = new URLSearchParams(window.location.search);
    return params.has("debug");
}

export function useDebugMode(): boolean {
    return useMemo(() => computeDebugFlag(), []);
}
