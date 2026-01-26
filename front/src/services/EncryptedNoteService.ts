import { DerivedKeyPair } from "./KeyService";
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

class EncryptedNoteService {
  private readonly baseUrl: string;

  constructor() {
    this.baseUrl = import.meta.env.VITE_SERVER_BASE_URL;
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
   * @param recipientPubkey - The recipient's public key (x-coordinate)
   * @param noteData - The note data to encrypt and upload
   * @param senderKeyPair - Optional sender keypair for creating a sender tag
   * @returns The upload response with note ID and timestamp
   */
  async uploadNote(
    recipientPubkey: string,
    noteData: unknown,
    senderKeyPair?: DerivedKeyPair
  ): Promise<UploadNoteResponse> {
    const normalizedRecipient = recipientPubkey.replace(/^0x/i, "");
    if (normalizedRecipient.length !== 64) {
      throw new Error("Recipient public key must be a 64-character hex string");
    }

    // Derive recipient tag from their public key
    const recipientTag = deriveRecipientTag(normalizedRecipient);

    // Encrypt the note
    const encrypted: EncryptedNote = encryptNote(normalizedRecipient, noteData);

    // Optionally derive sender tag
    const senderTag = senderKeyPair ? deriveRecipientTag(senderKeyPair.publicKey) : undefined;

    const payload = {
      recipient_tag: recipientTag,
      encrypted_payload: encrypted.encryptedPayload,
      ephemeral_pubkey: encrypted.ephemeralPubkey,
      sender_tag: senderTag,
    };

    const response = await this.request<{ id: string; stored_at: number }>("/api/notes", {
      method: "POST",
      body: JSON.stringify(payload),
    });

    if (!response) {
      throw new Error("Unexpected empty response from server");
    }

    return {
      id: response.id,
      storedAt: response.stored_at,
    };
  }

  /**
   * Fetches encrypted notes for the given keypair from the server.
   *
   * @param keyPair - The recipient's keypair
   * @param since - Optional timestamp to fetch notes newer than
   * @param limit - Maximum number of notes to fetch
   * @returns The fetch response with notes and pagination info
   */
  async fetchNotes(
    keyPair: DerivedKeyPair,
    since?: number,
    limit?: number
  ): Promise<FetchNotesResponse> {
    const recipientTag = deriveRecipientTag(keyPair.publicKey);

    const params = new URLSearchParams();
    if (since !== undefined) {
      params.set("since", since.toString());
    }
    if (limit !== undefined) {
      params.set("limit", limit.toString());
    }

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
      id: n.id,
      encryptedPayload: n.encrypted_payload,
      ephemeralPubkey: n.ephemeral_pubkey,
      senderTag: n.sender_tag,
      storedAt: n.stored_at,
    }));

    return {
      notes,
      hasMore: response.has_more,
    };
  }

  /**
   * Fetches and decrypts notes for the given keypair.
   *
   * @param keyPair - The recipient's keypair (needs private key for decryption)
   * @param since - Optional timestamp to fetch notes newer than
   * @param limit - Maximum number of notes to fetch
   * @returns Array of decrypted notes (failed decryptions are logged and skipped)
   */
  async fetchAndDecryptNotes(
    keyPair: DerivedKeyPair,
    since?: number,
    limit?: number
  ): Promise<{ notes: DecryptedNoteRecord[]; hasMore: boolean; failedCount: number }> {
    const { notes: encryptedNotes, hasMore } = await this.fetchNotes(keyPair, since, limit);

    const decryptedNotes: DecryptedNoteRecord[] = [];
    let failedCount = 0;

    for (const note of encryptedNotes) {
      try {
        const noteData = decryptNote(
          keyPair.privateKey,
          note.encryptedPayload,
          note.ephemeralPubkey
        );

        decryptedNotes.push({
          id: note.id,
          noteData,
          senderTag: note.senderTag,
          storedAt: note.storedAt,
        });
      } catch (error) {
        console.warn(`Failed to decrypt note ${note.id}:`, error);
        failedCount++;
      }
    }

    return { notes: decryptedNotes, hasMore, failedCount };
  }

  /**
   * Deletes a note from the server after it has been processed.
   *
   * @param keyPair - The recipient's keypair (for deriving the recipient tag)
   * @param noteId - The ID of the note to delete
   */
  async deleteNote(keyPair: DerivedKeyPair, noteId: string): Promise<void> {
    const recipientTag = deriveRecipientTag(keyPair.publicKey);

    await this.request(`/api/notes/${recipientTag}/${noteId}`, {
      method: "DELETE",
    });
  }

  /**
   * Fetches, decrypts, and deletes notes in one operation.
   * Notes are deleted after successful decryption.
   *
   * @param keyPair - The recipient's keypair
   * @param since - Optional timestamp to fetch notes newer than
   * @returns Array of decrypted notes
   */
  async processNotes(
    keyPair: DerivedKeyPair,
    since?: number
  ): Promise<{ notes: DecryptedNoteRecord[]; failedCount: number }> {
    const { notes: encryptedNotes } = await this.fetchNotes(keyPair, since);

    const decryptedNotes: DecryptedNoteRecord[] = [];
    let failedCount = 0;

    for (const note of encryptedNotes) {
      try {
        const noteData = decryptNote(
          keyPair.privateKey,
          note.encryptedPayload,
          note.ephemeralPubkey
        );

        decryptedNotes.push({
          id: note.id,
          noteData,
          senderTag: note.senderTag,
          storedAt: note.storedAt,
        });

        // Delete the note after successful decryption
        try {
          await this.deleteNote(keyPair, note.id);
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
