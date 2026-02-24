import { FullIdentity } from "./KeyService";
import { getServerBaseUrl } from "./ConfigService";
import { PrivateNote } from "../types/note";
import { encryptNote, decryptNote, deriveRecipientTag, EncryptedNote } from "./CryptoService";

export interface UploadNoteResponse {
    id: string;
    storedAt: number;
}

export interface EncryptedNoteRecord {
    id: string;
    encryptedPayload: string;
    ephemeralPubkey: string;
    senderTag?: string;
    storedAt: number;
}

export interface FetchNotesResponse {
    notes: EncryptedNoteRecord[];
    hasMore: boolean;
}

export interface DecryptedNoteRecord {
    id: string;
    noteData: unknown;
    senderTag?: string;
    storedAt: number;
}

export interface DecryptedNotePayload {
    note:      PrivateNote;
    txHash:    string;
    amount:    number;
    from:      string;   // sender's secp256k1 pubkey x-coord
    timestamp: number;   // ms since epoch
}

class EncryptedNoteService {
    private readonly baseUrl: string;

    constructor() {
        this.baseUrl = getServerBaseUrl();
    }

    private buildUrl(path: string): string {
        const normalizedBase = this.baseUrl?.replace(/\/$/, "") ?? "";
        const normalizedPath = path.startsWith("/") ? path : `/${path}`;
        return `${normalizedBase}${normalizedPath}`;
    }

    private async request<T>(path: string, options: RequestInit = {}): Promise<T | undefined> {
        const headers = new Headers(options.headers || {});
        if (options.body !== undefined && !headers.has("Content-Type")) {
            headers.set("Content-Type", "application/json");
        }

        const response = await fetch(this.buildUrl(path), {
            ...options,
            headers,
        });

        if (!response.ok) {
            const errorBody = await response.text().catch(() => "");
            throw new Error(`Request failed with status ${response.status}: ${errorBody}`);
        }

        if (response.status === 204) {
            return undefined;
        }

        try {
            return (await response.json()) as T;
        } catch {
            return undefined;
        }
    }

    /**
     * Uploads an encrypted note to the server.
     *
     * @param recipientUtxoAddress    - Recipient's UTXO address (poseidon2-derived, 64 hex chars)
     * @param recipientEncryptionPubkey - Recipient's secp256k1 pubkey x-coord (for ECDH)
     * @param noteData                - The note payload to encrypt
     * @param senderKeyPair           - Sender's full identity
     */
    async uploadNote(
        recipientUtxoAddress: string,
        recipientEncryptionPubkey: string,
        noteData: DecryptedNotePayload,
        senderKeyPair: FullIdentity
    ): Promise<UploadNoteResponse> {
        const normalizedRecipient = recipientUtxoAddress.replace(/^0x/i, "");
        if (normalizedRecipient.length !== 64) {
            throw new Error("Recipient UTXO address must be a 64-character hex string");
        }

        // Tag is derived from the UTXO address
        const recipientTag = deriveRecipientTag(normalizedRecipient);

        // ECDH encryption uses the secp256k1 pubkey
        const encrypted: EncryptedNote = encryptNote(recipientEncryptionPubkey, noteData);

        // Sender tag derived from sender's UTXO address
        const senderTag = deriveRecipientTag(senderKeyPair.utxoAddress);

        const payload = {
            recipient_tag:     recipientTag,
            encrypted_payload: encrypted.encryptedPayload,
            ephemeral_pubkey:  encrypted.ephemeralPubkey,
            sender_tag:        senderTag,
        };

        const response = await this.request<{ id: string; stored_at: number }>("/api/notes", {
            method: "POST",
            body: JSON.stringify(payload),
        });

        if (!response) {
            throw new Error("Unexpected empty response from server");
        }

        return {
            id:       response.id,
            storedAt: response.stored_at,
        };
    }

    /**
     * Fetches encrypted notes for the given identity from the server.
     * Tag is derived from the identity's UTXO address.
     */
    async fetchNotes(
        identity: FullIdentity,
        since?: number,
        limit?: number
    ): Promise<FetchNotesResponse> {
        const recipientTag = deriveRecipientTag(identity.utxoAddress);

        const params = new URLSearchParams();
        if (since !== undefined) params.set("since", since.toString());
        if (limit !== undefined) params.set("limit", limit.toString());

        const queryString = params.toString();
        const path = `/api/notes/${recipientTag}${queryString ? `?${queryString}` : ""}`;

        const response = await this.request<{
            notes: Array<{
                id: string;
                encrypted_payload: string;
                ephemeral_pubkey: string;
                sender_tag?: string;
                stored_at: number;
            }>;
            has_more: boolean;
        }>(path);

        if (!response) {
            return { notes: [], hasMore: false };
        }

        const notes: EncryptedNoteRecord[] = response.notes.map((n) => ({
            id:               n.id,
            encryptedPayload: n.encrypted_payload,
            ephemeralPubkey:  n.ephemeral_pubkey,
            senderTag:        n.sender_tag,
            storedAt:         n.stored_at,
        }));

        return { notes, hasMore: response.has_more };
    }

    /**
     * Fetches and decrypts notes for the given identity.
     * Decryption uses the identity's secp256k1 private key (ECDH).
     */
    async fetchAndDecryptNotes(
        identity: FullIdentity,
        since?: number,
        limit?: number
    ): Promise<{ notes: DecryptedNoteRecord[]; hasMore: boolean; failedCount: number }> {
        const { notes: encryptedNotes, hasMore } = await this.fetchNotes(identity, since, limit);

        const decryptedNotes: DecryptedNoteRecord[] = [];
        let failedCount = 0;

        for (const note of encryptedNotes) {
            try {
                const noteData = decryptNote(
                    identity.privateKey,
                    note.encryptedPayload,
                    note.ephemeralPubkey
                );
                decryptedNotes.push({
                    id:        note.id,
                    noteData,
                    senderTag: note.senderTag,
                    storedAt:  note.storedAt,
                });
            } catch (error) {
                console.warn(`Failed to decrypt note ${note.id}:`, error);
                failedCount++;
            }
        }

        return { notes: decryptedNotes, hasMore, failedCount };
    }

    /**
     * Deletes a note from the server after processing.
     */
    async deleteNote(identity: FullIdentity, noteId: string): Promise<void> {
        const recipientTag = deriveRecipientTag(identity.utxoAddress);
        await this.request(`/api/notes/${recipientTag}/${noteId}`, {
            method: "DELETE",
        });
    }

    /**
     * Fetches, decrypts, and deletes notes in one operation.
     */
    async processNotes(
        identity: FullIdentity,
        since?: number
    ): Promise<{ notes: DecryptedNoteRecord[]; failedCount: number }> {
        const { notes: encryptedNotes } = await this.fetchNotes(identity, since);

        const decryptedNotes: DecryptedNoteRecord[] = [];
        let failedCount = 0;

        for (const note of encryptedNotes) {
            try {
                const noteData = decryptNote(
                    identity.privateKey,
                    note.encryptedPayload,
                    note.ephemeralPubkey
                );
                decryptedNotes.push({
                    id:        note.id,
                    noteData,
                    senderTag: note.senderTag,
                    storedAt:  note.storedAt,
                });

                try {
                    await this.deleteNote(identity, note.id);
                } catch (deleteError) {
                    console.warn(`Failed to delete note ${note.id}:`, deleteError);
                }
            } catch (error) {
                console.warn(`Failed to decrypt note ${note.id}:`, error);
                failedCount++;
            }
        }

        return { notes: decryptedNotes, failedCount };
    }
}

export const encryptedNoteService = new EncryptedNoteService();
