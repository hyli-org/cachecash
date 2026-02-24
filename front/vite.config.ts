import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vite.dev/config/
export default defineConfig({
    plugins: [react()],
    server: {
        headers: {
            "Cross-Origin-Opener-Policy": "same-origin",
            "Cross-Origin-Embedder-Policy": "require-corp",
        },
    },
    optimizeDeps: {
        esbuildOptions: { target: "esnext" },
        exclude: ["@noir-lang/noirc_abi", "@noir-lang/acvm_js", "@aztec/bb.js"],
        include: ["pino", "buffer"],
    },
    resolve: {
        alias: {
            buffer: "buffer/",
        },
    },
});
