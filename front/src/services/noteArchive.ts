import JSZip from "jszip";
import { StoredNote } from "../types/note";

const ARCHIVE_DATA_FILENAME = "notes.json" as const;

export interface NotesArchiveMetadata {
    player: string;
    exportedAt: number;
}

interface NotesArchivePayload extends NotesArchiveMetadata {
    notes: StoredNote[];
}

export interface NotesArchiveDownload {
    filename: string;
    blob: Blob;
}

export type NotesArchiveReadResult =
    | { ok: true; data: NotesArchivePayload }
    | { ok: false; error: string };

function buildArchiveFilename(playerName: string): string {
    const slug = playerName
        .trim()
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, "-")
        .replace(/^-+|-+$/g, "");

    return slug ? `notes-${slug}.zip` : "notes.zip";
}

function serializePayload(playerName: string, notes: StoredNote[]): string {
    const payload: NotesArchivePayload = {
        player: playerName,
        exportedAt: Date.now(),
        notes,
    };

    return JSON.stringify(payload, null, 2);
}

function isStoredNote(value: unknown): value is StoredNote {
    if (!value || typeof value !== "object") {
        return false;
    }

    const candidate = value as Partial<StoredNote>;
    return (
        typeof candidate.txHash === "string" &&
        "note" in candidate &&
        typeof candidate.storedAt === "number" &&
        typeof candidate.player === "string"
    );
}

export async function createNotesArchive(playerName: string, notes: StoredNote[]): Promise<NotesArchiveDownload> {
    const zip = new JSZip();
    zip.file(ARCHIVE_DATA_FILENAME, serializePayload(playerName, notes));

    const blob = await zip.generateAsync({ type: "blob" });

    return {
        filename: buildArchiveFilename(playerName),
        blob,
    };
}

export async function readNotesArchive(file: File | Blob): Promise<NotesArchiveReadResult> {
    try {
        const zip = await JSZip.loadAsync(file);
        const entry = zip.file(ARCHIVE_DATA_FILENAME);

        if (!entry) {
            return { ok: false, error: `Archive missing ${ARCHIVE_DATA_FILENAME}` };
        }

        const content = await entry.async("string");
        const parsed = JSON.parse(content) as Partial<NotesArchivePayload> | StoredNote[];

        if (Array.isArray(parsed)) {
            if (parsed.every(isStoredNote)) {
                return {
                    ok: true,
                    data: {
                        player: "",
                        exportedAt: Date.now(),
                        notes: parsed,
                    },
                };
            }
            return { ok: false, error: "notes.json does not contain valid notes" };
        }

        if (!parsed || typeof parsed !== "object") {
            return { ok: false, error: "notes.json payload is invalid" };
        }

        const { player, notes } = parsed as Partial<NotesArchivePayload>;
        if (typeof player !== "string" || !Array.isArray(notes)) {
            return { ok: false, error: "notes.json missing player or notes" };
        }
        if (!notes.every(isStoredNote)) {
            return { ok: false, error: "notes.json contains malformed notes" };
        }

        return {
            ok: true,
            data: {
                player,
                exportedAt: typeof parsed.exportedAt === "number" ? parsed.exportedAt : Date.now(),
                notes,
            },
        };
    } catch (error) {
        console.warn("Failed to read notes archive", error);
        return { ok: false, error: "Unable to read notes archive" };
    }
}
