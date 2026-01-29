import { ec as EC } from "elliptic";
import SHA256 from "crypto-js/sha256";
import Hex from "crypto-js/enc-hex";
import AES from "crypto-js/aes";
import Utf8 from "crypto-js/enc-utf8";

const curve = new EC("secp256k1");

export interface EncryptedNote {
  encryptedPayload: string; // Base64-encoded AES ciphertext
  ephemeralPubkey: string; // Hex-encoded ephemeral public key (x-coordinate)
}

/**
 * Derives a recipient tag from a public key.
 * This is a one-way, non-reversible identifier used to query for notes.
 *
 * @param publicKeyHex - The recipient's public key (x-coordinate, 64 hex chars)
 * @returns A 64-character hex string representing the recipient tag
 */
export function deriveRecipientTag(publicKeyHex: string): string {
  const normalized = publicKeyHex.replace(/^0x/i, "").toLowerCase();
  if (normalized.length !== 64) {
    throw new Error("Public key must be a 64-character hex string (32 bytes)");
  }

  const tagInput = normalized + ":recipient_tag";
  return SHA256(tagInput).toString(Hex);
}

/**
 * Encrypts a note for a recipient using ECDH key exchange + AES encryption.
 *
 * The process:
 * 1. Generate an ephemeral secp256k1 keypair
 * 2. Derive a shared secret using ECDH: SHA256(recipientPubkey * ephemeralPrivate)
 * 3. Encrypt the note data using AES with the shared secret
 *
 * @param recipientPubkeyHex - The recipient's public key (x-coordinate, 64 hex chars)
 * @param noteData - The note data to encrypt (will be JSON serialized)
 * @returns The encrypted note with ephemeral public key
 */
export function encryptNote(recipientPubkeyHex: string, noteData: unknown): EncryptedNote {
  const normalizedRecipient = recipientPubkeyHex.replace(/^0x/i, "").toLowerCase();
  if (normalizedRecipient.length !== 64) {
    throw new Error("Recipient public key must be a 64-character hex string");
  }

  // Generate ephemeral keypair
  const ephemeralKey = curve.genKeyPair();
  const ephemeralPrivate = ephemeralKey.getPrivate();
  const ephemeralPubkeyX = ephemeralKey.getPublic().getX().toString(16).padStart(64, "0");

  // Reconstruct recipient's public key point from x-coordinate
  // We try both possible y values (even and odd) and use the one that's on the curve
  const recipientPubkey = reconstructPublicKey(normalizedRecipient);

  // Derive shared secret: ECDH
  const sharedPoint = recipientPubkey.mul(ephemeralPrivate);
  const sharedSecretX = sharedPoint.getX().toString(16).padStart(64, "0");
  const sharedSecret = SHA256(sharedSecretX).toString(Hex);

  // Encrypt the note data
  const plaintext = JSON.stringify(noteData);
  const ciphertext = AES.encrypt(plaintext, sharedSecret);
  const encryptedPayload = ciphertext.toString(); // Base64 by default

  return {
    encryptedPayload,
    ephemeralPubkey: ephemeralPubkeyX,
  };
}

/**
 * Decrypts an encrypted note using the recipient's private key.
 *
 * The process:
 * 1. Derive the shared secret using ECDH: SHA256(ephemeralPubkey * recipientPrivate)
 * 2. Decrypt the payload using AES with the shared secret
 * 3. Parse the JSON result
 *
 * @param privateKeyHex - The recipient's private key (64 hex chars)
 * @param encryptedPayload - The Base64-encoded encrypted payload
 * @param ephemeralPubkeyHex - The ephemeral public key (x-coordinate, 64 hex chars)
 * @returns The decrypted note data
 */
export function decryptNote(
  privateKeyHex: string,
  encryptedPayload: string,
  ephemeralPubkeyHex: string
): unknown {
  const normalizedPrivate = privateKeyHex.replace(/^0x/i, "").toLowerCase();
  const normalizedEphemeral = ephemeralPubkeyHex.replace(/^0x/i, "").toLowerCase();

  if (normalizedPrivate.length !== 64) {
    throw new Error("Private key must be a 64-character hex string");
  }
  if (normalizedEphemeral.length !== 64) {
    throw new Error("Ephemeral public key must be a 64-character hex string");
  }

  // Reconstruct ephemeral public key point from x-coordinate
  const ephemeralPubkey = reconstructPublicKey(normalizedEphemeral);

  // Derive shared secret: ECDH
  const privateKey = curve.keyFromPrivate(normalizedPrivate, "hex");
  const sharedPoint = ephemeralPubkey.mul(privateKey.getPrivate());
  const sharedSecretX = sharedPoint.getX().toString(16).padStart(64, "0");
  const sharedSecret = SHA256(sharedSecretX).toString(Hex);

  // Decrypt the payload
  const decrypted = AES.decrypt(encryptedPayload, sharedSecret);
  const plaintext = decrypted.toString(Utf8);

  if (!plaintext) {
    throw new Error("Decryption failed - invalid key or corrupted data");
  }

  return JSON.parse(plaintext);
}

/**
 * Reconstructs a public key point from an x-coordinate.
 * Since we only have the x-coordinate, we need to derive the y-coordinate.
 *
 * For secp256k1: y^2 = x^3 + 7 (mod p)
 * There are two possible y values - we try both to find a valid point.
 */
function reconstructPublicKey(xHex: string) {
  // Try reconstructing with even y first (compressed format prefix 02)
  try {
    const compressedEven = "02" + xHex;
    const keyEven = curve.keyFromPublic(compressedEven, "hex");
    return keyEven.getPublic();
  } catch {
    // Try with odd y (compressed format prefix 03)
    const compressedOdd = "03" + xHex;
    const keyOdd = curve.keyFromPublic(compressedOdd, "hex");
    return keyOdd.getPublic();
  }
}

/**
 * Verifies that a private key matches a public key.
 *
 * @param privateKeyHex - The private key to verify
 * @param publicKeyHex - The public key (x-coordinate) to match
 * @returns True if the keys match
 */
export function verifyKeyPair(privateKeyHex: string, publicKeyHex: string): boolean {
  const normalizedPrivate = privateKeyHex.replace(/^0x/i, "").toLowerCase();
  const normalizedPublic = publicKeyHex.replace(/^0x/i, "").toLowerCase();

  try {
    const key = curve.keyFromPrivate(normalizedPrivate, "hex");
    const derivedPublic = key.getPublic().getX().toString(16).padStart(64, "0");
    return derivedPublic === normalizedPublic;
  } catch {
    return false;
  }
}
