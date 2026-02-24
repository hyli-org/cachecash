// Must be the first import in main.tsx so Buffer is set before @aztec/bb.js
// initialises its static class members (e.g. Fr.ZERO = new Fr(0n)).
import { Buffer } from "buffer/";

if (typeof globalThis !== "undefined") {
    (globalThis as unknown as { Buffer: typeof Buffer }).Buffer = Buffer;
}
