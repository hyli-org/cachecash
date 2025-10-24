import { useMemo, useState } from "react";
import { StoredNote } from "../types/note";

interface DebugNotesPanelProps {
    notes: StoredNote[];
    onClear: () => void;
}

export function DebugNotesPanel({ notes, onClear }: DebugNotesPanelProps) {
    const [isCollapsed, setIsCollapsed] = useState(false);

    const formattedNotes = useMemo(
        () =>
            notes.map((entry) => ({
                txHash: entry.txHash,
                storedAt: new Date(entry.storedAt).toLocaleTimeString(),
                payloadText:
                    entry.note === undefined
                        ? "undefined"
                        : JSON.stringify(entry.note, null, 2) ?? "undefined",
                player: entry.player || "Unknown",
                id: `${entry.player || "unknown"}-${entry.txHash}-${entry.storedAt}`,
            })),
        [notes],
    );

    const hasNotes = formattedNotes.length > 0;

    return (
        <div className="debug-notes nes-hud nes-hud--debug">
            <div className="nes-hud__panel nes-hud__panel--pixel debug-notes__panel">
                <div className="debug-notes__header">
                    <div>
                        <div className="debug-notes__eyebrow">Debug Panel</div>
                        <h2 className="debug-notes__title">Stored Notes</h2>
                    </div>
                    <div className="debug-notes__actions">
                        <button
                            type="button"
                            className="pixel-button pixel-button--ghost pixel-button--compact"
                            onClick={() => setIsCollapsed((prev) => !prev)}
                        >
                            {isCollapsed ? "SHOW" : "HIDE"}
                        </button>
                        <button
                            type="button"
                            className="pixel-button pixel-button--compact"
                            onClick={onClear}
                            disabled={!hasNotes}
                        >
                            CLEAR
                        </button>
                    </div>
                </div>
                {!isCollapsed && (
                    <div className="debug-notes__content">
                        {hasNotes ? (
                            <ul className="debug-notes__list">
                                {formattedNotes.map((note) => (
                                    <li key={note.id} className="debug-notes__item">
                                        <div className="debug-notes__meta">
                                            <div className="debug-notes__meta-line">
                                                <span className="debug-notes__label">Player</span>
                                                <span className="debug-notes__value">{note.player}</span>
                                                <span className="debug-notes__timestamp">{note.storedAt}</span>
                                            </div>
                                            <div className="debug-notes__meta-line">
                                                <span className="debug-notes__label">Ref</span>
                                                <span className="debug-notes__value">{note.txHash}</span>
                                            </div>
                                        </div>
                                        <pre className="debug-notes__payload">{note.payloadText}</pre>
                                    </li>
                                ))}
                            </ul>
                        ) : (
                            <div className="debug-notes__empty">No notes have been stored yet.</div>
                        )}
                    </div>
                )}
            </div>
        </div>
    );
}
