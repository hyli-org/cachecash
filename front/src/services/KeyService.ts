import SHA256 from "crypto-js/sha256";
import { BarretenbergSync, Fr } from "@aztec/bb.js";

// TODO: Replace deterministic derivation with secure randomness when persistent key storage is available.

export interface DerivedKeyPair {
  privateKey: string;
  publicKey: string;
}

const normalizeName = (name: string): string => name.trim().toLowerCase().normalize("NFKC");

// Singleton promise for Barretenberg initialization
let initPromise: Promise<void> | null = null;

/**
 * Initialize the Barretenberg WASM module.
 * This must be called before using deriveKeyPairFromName.
 * Safe to call multiple times - only initializes once.
 */
export async function initBarretenberg(): Promise<void> {
  if (!initPromise) {
    initPromise = BarretenbergSync.initSingleton().then(() => {});
  }
  await initPromise;
}

/**
 * Check if Barretenberg has been initialized
 */
export function isBarretenbergInitialized(): boolean {
  try {
    BarretenbergSync.getSingleton();
    return true;
  } catch {
    return false;
  }
}

/**
 * Compute address from private key using Poseidon2 hash (Noir-compatible)
 * address = poseidon2([privateKey, 0])
 * This matches the Noir circuit: get_address(secret_key) = poseidon2([secret_key, 0])
 */
function computeAddress(privateKey: string): string {
  const bb = BarretenbergSync.getSingleton();
  const privateKeyFr = Fr.fromString("0x" + privateKey);
  const zeroFr = Fr.fromString("0x0");
  const address = bb.poseidon2Hash([privateKeyFr, zeroFr]);
  return address.toString().slice(2); // Remove "0x" prefix
}

export const deriveKeyPairFromName = (name: string): DerivedKeyPair => {
  if (!isBarretenbergInitialized()) {
    throw new Error("Barretenberg not initialized. Call initBarretenberg() first.");
  }

  const normalized = normalizeName(name);

  if (!normalized) {
    throw new Error("Cannot derive key pair from empty name");
  }

  const privateKey = SHA256(normalized).toString();
  const publicKey = computeAddress(privateKey);

  return {
    privateKey,
    publicKey,
  };
};
