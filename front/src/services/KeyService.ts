import { ec as EC } from "elliptic";
import SHA256 from "crypto-js/sha256";
import { poseidon2Service } from "./Poseidon2Service";

const curve = new EC("secp256k1");

// TODO: Replace deterministic derivation with secure randomness when persistent key storage is available.

export interface DerivedKeyPair {
  privateKey: string;
  publicKey: string;
}

export interface FullIdentity {
  privateKey:  string;  // secp256k1 private key (64-char hex)
  publicKey:   string;  // secp256k1 pubkey x-coord (64-char hex) – used for ECDH encryption
  zkSecretKey: string;  // poseidon2-derived (64-char hex) – proves UTXO ownership
  utxoAddress: string;  // poseidon2-derived (64-char hex) – appears in notes on-chain
}

const normalizeName = (name: string): string => name.trim().toLowerCase().normalize("NFKC");

export const deriveKeyPairFromName = (name: string): DerivedKeyPair => {
  const normalized = normalizeName(name);

  if (!normalized) {
    throw new Error("Cannot derive key pair from empty name");
  }

  const privateKeyHex = SHA256(normalized).toString();
  const key = curve.keyFromPrivate(privateKeyHex, "hex");
  const privateKey = key.getPrivate("hex").padStart(64, "0");
  const publicKeyX = key.getPublic().getX().toString(16).padStart(64, "0");

  return {
    privateKey,
    publicKey: publicKeyX,
  };
};

/**
 * ZK secret key: poseidon2([low_128, high_128], 2)
 * Split 32-byte secp256k1 private key:
 *   high = first 16 bytes, zero-padded to 32 bytes (leading zeros)
 *   low  = last  16 bytes, zero-padded to 32 bytes (leading zeros)
 */
export async function deriveZkSecretKey(privateKeyHex: string): Promise<string> {
  const high = privateKeyHex.slice(0, 32).padStart(64, "0"); // bytes 0-15 → 32-byte field
  const low  = privateKeyHex.slice(32).padStart(64, "0");    // bytes 16-31 → 32-byte field
  return poseidon2Service.hash([low, high]);
}

/** UTXO address: poseidon2([zkSecretKey, 0], 2) */
export async function deriveUtxoAddress(zkSecretKey: string): Promise<string> {
  return poseidon2Service.hash([zkSecretKey, "0".repeat(64)]);
}

/** Convenience: derive everything from a player name */
export async function deriveFullIdentity(name: string): Promise<FullIdentity> {
  const { privateKey, publicKey } = deriveKeyPairFromName(name);
  const zkSecretKey = await deriveZkSecretKey(privateKey);
  const utxoAddress = await deriveUtxoAddress(zkSecretKey);
  return { privateKey, publicKey, zkSecretKey, utxoAddress };
}
