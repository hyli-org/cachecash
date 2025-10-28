import { ec as EC } from "elliptic";
import SHA256 from "crypto-js/sha256";

const curve = new EC("secp256k1");

// TODO: Replace deterministic derivation with secure randomness when persistent key storage is available.

export interface DerivedKeyPair {
  privateKey: string;
  publicKey: string;
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
