import { useState, useEffect, useCallback, useMemo, ChangeEvent, FormEvent } from "react";
import "./App.css";
import { deriveFullIdentity, FullIdentity } from "./services/KeyService";
import { addressService } from "./services/AddressService";
import { getNodeBaseUrl } from "./services/ConfigService";

import { TransactionList } from "./components/TransactionList";
import { DebugNotesPanel } from "./components/DebugNotesPanel";
import { ManageNotesModal } from "./components/ManageNotesModal";
import { TransferModal } from "./components/TransferModal";
import { transferService, parseNoteValue } from "./services/TransferService";
import { nodeService } from "./services/NodeService";
import { addStoredNote } from "./services/noteStorage";
import { declareCustomElement } from "testnet-maintenance-widget";
import { useStoredNotes } from "./hooks/useStoredNotes";
import { useDebugMode } from "./hooks/useDebugMode";
import { useEncryptedNotes } from "./hooks/useEncryptedNotes";
import { PrivateNote, StoredNote } from "./types/note";
declareCustomElement();

interface TransactionEntry {
    title: string;
    hash?: string;
    timestamp: number;
}

function shortHex(hex: string, chars = 8): string {
    const h = hex.replace(/^0x/i, "");
    if (h.length <= chars * 2) return h;
    return `${h.slice(0, chars)}…${h.slice(-4)}`;
}

function parseValue(note: PrivateNote): number {
    const hex = (note.value ?? "").replace(/^0x/i, "");
    if (!hex || hex === "0".repeat(64)) return 0;
    const n = parseInt(hex, 16);
    return isNaN(n) ? 0 : n;
}

function App() {
    const debugMode = useDebugMode();
    const [playerName, setPlayerName] = useState(() => localStorage.getItem("playerName") || "");
    const { notes: storedNotes } = useStoredNotes(playerName);
    const [isManageModalOpen, setIsManageModalOpen] = useState(false);
    const [isTransferModalOpen, setIsTransferModalOpen] = useState(false);
    const [nameInput, setNameInput] = useState(() => localStorage.getItem("playerName") || "");
    const [playerKeys, setPlayerKeys] = useState<FullIdentity | null>(null);
    const [transactions, setTransactions] = useState<TransactionEntry[]>([]);
    const [addressCopied, setAddressCopied] = useState(false);
    const [faucetStatus, setFaucetStatus] = useState<"idle" | "loading" | "success" | "error">("idle");
    const [faucetError, setFaucetError] = useState<string | null>(null);
    const [theme, setTheme] = useState<"dark" | "light">(() => {
        return (localStorage.getItem("theme") as "dark" | "light") ?? "dark";
    });

    const handleNotesReceived = useCallback((notes: any[]) => {
        console.log(`Received ${notes.length} encrypted notes`);
    }, []);

    const handleNotesError = useCallback((error: Error) => {
        console.error("Encrypted notes polling error:", error);
    }, []);

    useEncryptedNotes(playerKeys, playerName, {
        enabled: !!playerKeys && !!playerName,
        onNotesReceived: handleNotesReceived,
        onError: handleNotesError,
    });

    const availableNotesForTransfer = useMemo(() => {
        if (!playerKeys || !playerName) return [];
        return transferService.getSpendableNotes(storedNotes, playerKeys.zkSecretKey, playerName);
    }, [storedNotes, playerKeys, playerName]);

    const totalBalance = useMemo(
        () => availableNotesForTransfer.reduce((sum, n) => sum + parseNoteValue(n.note), 0),
        [availableNotesForTransfer],
    );

    const displayNotes = useMemo(
        () =>
            storedNotes
                .map((stored) => {
                    const note = stored.note as PrivateNote;
                    const value = parseValue(note);
                    return { stored, note, value };
                })
                .filter(({ value }) => value > 0)
                .sort((a, b) => b.stored.storedAt - a.stored.storedAt),
        [storedNotes],
    );

    useEffect(() => {
        if (!playerName) {
            localStorage.removeItem("playerName");
            return;
        }
        localStorage.setItem("playerName", playerName);
    }, [playerName]);

    useEffect(() => {
        if (!playerName) {
            setPlayerKeys(null);
            return;
        }

        let cancelled = false;
        deriveFullIdentity(playerName)
            .then((identity) => {
                if (!cancelled) setPlayerKeys(identity);
                addressService
                    .register(playerName, identity.utxoAddress, identity.publicKey)
                    .catch((error) => console.warn("Address registration failed:", error));
            })
            .catch((error) => {
                console.error("Failed to derive full identity", error);
                if (!cancelled) setPlayerKeys(null);
            });

        return () => {
            cancelled = true;
        };
    }, [playerName]);

    useEffect(() => {
        if (!playerName) {
            setNameInput("");
        } else {
            setNameInput(playerName);
        }
    }, [playerName]);

    useEffect(() => {
        document.documentElement.dataset.theme = theme;
        localStorage.setItem("theme", theme);
    }, [theme]);

    const handleToggleTheme = useCallback(() => {
        setTheme((t) => (t === "dark" ? "light" : "dark"));
    }, []);

    const handleNameChange = useCallback((event: ChangeEvent<HTMLInputElement>) => {
        setNameInput(event.target.value);
    }, []);

    const handleNameSubmit = useCallback(
        (event: FormEvent<HTMLFormElement>) => {
            event.preventDefault();
            const trimmed = nameInput.trim();
            if (!trimmed) {
                setPlayerName("");
                return;
            }
            setPlayerName(trimmed);
        },
        [nameInput, setPlayerName],
    );

    const handleLogout = useCallback(() => {
        setPlayerName("");
    }, [setPlayerName]);

    const handleOpenManageModal = useCallback(() => {
        if (!playerName) return;
        setIsManageModalOpen(true);
    }, [playerName]);

    const handleCloseManageModal = useCallback(() => {
        setIsManageModalOpen(false);
    }, []);

    const handleOpenTransferModal = useCallback(() => {
        if (!playerName) return;
        setIsTransferModalOpen(true);
    }, [playerName]);

    const handleCloseTransferModal = useCallback(() => {
        setIsTransferModalOpen(false);
    }, []);

    const handleFaucet = useCallback(async () => {
        if (!playerKeys?.utxoAddress || !playerName || faucetStatus === "loading") return;
        setFaucetStatus("loading");
        setFaucetError(null);
        try {
            const response = await nodeService.requestFaucet(playerKeys.utxoAddress);
            const { tx_hash: txHash, note } = response;
            const reference = txHash ?? (note as any)?.psi;
            const entry: StoredNote = {
                txHash: reference ?? `faucet-${Date.now()}`,
                note: (note ?? response) as unknown as PrivateNote,
                storedAt: Date.now(),
                player: playerName,
            };
            addStoredNote(playerName, entry);
            setFaucetStatus("success");
            setTimeout(() => setFaucetStatus("idle"), 3000);
        } catch (err) {
            console.error("Faucet failed:", err);
            setFaucetError(err instanceof Error ? err.message : "Faucet request failed.");
            setFaucetStatus("error");
            setTimeout(() => setFaucetStatus("idle"), 4000);
        }
    }, [playerKeys?.utxoAddress, playerName, faucetStatus]);

    const handleCopyAddress = useCallback(() => {
        if (!playerKeys?.utxoAddress) return;
        navigator.clipboard.writeText(playerKeys.utxoAddress).then(() => {
            setAddressCopied(true);
            setTimeout(() => setAddressCopied(false), 2500);
        });
    }, [playerKeys?.utxoAddress]);

    return (
        <div className="app">
            <TransactionList
                transactions={transactions}
                setTransactions={setTransactions}
                isMobile={false}
                isSecretVideoOpen={false}
            />
            <maintenance-widget nodeUrl={getNodeBaseUrl()} />

            {!playerName && (
                <div className="login-screen">
                    <div className="login-card">
                        <div className="login-card-top">
                            <div className="wallet-logo">Cache Cash</div>
                            <button
                                type="button"
                                className="btn btn-ghost btn-sm theme-toggle"
                                onClick={handleToggleTheme}
                                aria-label="Toggle theme"
                            >
                                {theme === "dark" ? "☀" : "☾"}
                            </button>
                        </div>
                        <p className="login-subtitle">A private, zero-knowledge wallet on Hyli</p>

                        <div className="login-divider" />

                        <p className="login-description">
                            CacheCash is a UTXO-based cash system proved with Noir and SP1, inspired by Payy.
                        </p>
                        <div className="login-links">
                            <a
                                href="https://blog.hyli.org"
                                target="_blank"
                                rel="noreferrer"
                                className="login-link"
                            >
                                Blog post ↗
                            </a>
                            <a
                                href="https://github.com/hyli-org/cachecash"
                                target="_blank"
                                rel="noreferrer"
                                className="login-link"
                            >
                                Source code ↗
                            </a>
                        </div>
                        <div className="login-disclaimer">⚠ Experimental — not connected to any airdrop.</div>

                        <div className="login-divider" />

                        <form className="login-form" onSubmit={handleNameSubmit}>
                            <input
                                type="text"
                                className="form-input"
                                value={nameInput}
                                onChange={handleNameChange}
                                placeholder="Enter your username"
                                maxLength={32}
                                required
                            />
                            <button type="submit" className="btn btn-primary">
                                Connect
                            </button>
                        </form>
                    </div>
                </div>
            )}

            {playerName && (
                <div className="wallet">
                    <header className="wallet-header">
                        <div className="wallet-logo">Cache Cash</div>
                        <div className="wallet-user">
                            <button
                                type="button"
                                className="btn btn-ghost btn-sm theme-toggle"
                                onClick={handleToggleTheme}
                                aria-label="Toggle theme"
                            >
                                {theme === "dark" ? "☀" : "☾"}
                            </button>
                            <span className="wallet-user-pill">{playerName}</span>
                            <button type="button" className="btn btn-ghost btn-sm" onClick={handleLogout}>
                                Disconnect
                            </button>
                        </div>
                    </header>

                    <main className="wallet-main">
                        <div className="balance-card">
                            <div className="balance-label">Total Balance</div>
                            <div className="balance-amount">{totalBalance.toLocaleString()}</div>
                            <div className="balance-note-count">
                                {displayNotes.length} note{displayNotes.length !== 1 ? "s" : ""}
                            </div>
                            <div className="action-bar">
                                <button
                                    type="button"
                                    className="btn btn-primary"
                                    onClick={handleOpenTransferModal}
                                >
                                    Send
                                </button>
                                <button
                                    type="button"
                                    className="btn btn-secondary"
                                    onClick={handleFaucet}
                                    disabled={faucetStatus === "loading" || !playerKeys}
                                >
                                    {faucetStatus === "loading" ? "Requesting…" : "Faucet"}
                                </button>
                                <button
                                    type="button"
                                    className="btn btn-ghost"
                                    onClick={handleOpenManageModal}
                                >
                                    Manage
                                </button>
                            </div>
                            {faucetStatus === "success" && (
                                <div className="status-success" style={{ marginTop: "0.5rem" }}>
                                    Note received from faucet.
                                </div>
                            )}
                            {faucetStatus === "error" && faucetError && (
                                <div className="status-error" style={{ marginTop: "0.5rem" }}>
                                    {faucetError}
                                </div>
                            )}
                        </div>

                        {playerKeys?.utxoAddress && (
                            <div className="address-card">
                                <div className="address-label">Your Address (to receive)</div>
                                <div className="address-row">
                                    <code className="address-value" title={playerKeys.utxoAddress}>
                                        {shortHex(playerKeys.utxoAddress, 10)}
                                    </code>
                                    <button
                                        type="button"
                                        className="btn btn-ghost btn-sm"
                                        onClick={handleCopyAddress}
                                    >
                                        {addressCopied ? "Copied!" : "Copy"}
                                    </button>
                                </div>
                            </div>
                        )}

                        <div className="notes-section">
                            <div className="notes-section-header">Notes</div>
                            {displayNotes.length === 0 ? (
                                <div className="notes-empty">
                                    No notes yet. Receive a transfer from another user.
                                </div>
                            ) : (
                                <ul className="notes-list">
                                    {displayNotes.map(({ stored, note, value }) => (
                                        <li key={stored.txHash} className="note-item">
                                            <span className="note-value">{value.toLocaleString()}</span>
                                            <span className="note-hash" title={note.psi}>
                                                psi:{shortHex(note.psi, 6)}
                                            </span>
                                            <span className="note-date">
                                                {new Date(stored.storedAt).toLocaleDateString()}
                                            </span>
                                        </li>
                                    ))}
                                </ul>
                            )}
                        </div>
                    </main>

                    <footer className="wallet-footer">
                        <span>Experimental · Keys stay local</span>
                        <span className="wallet-footer-links">
                            <a href="https://blog.hyli.org" target="_blank" rel="noreferrer">
                                Blog
                            </a>
                            {" · "}
                            <a href="https://github.com/hyli-org/cachecash" target="_blank" rel="noreferrer">
                                GitHub
                            </a>
                        </span>
                    </footer>
                </div>
            )}

            {isManageModalOpen && (
                <ManageNotesModal
                    playerName={playerName}
                    notes={storedNotes}
                    identity={playerKeys}
                    onClose={handleCloseManageModal}
                />
            )}
            {isTransferModalOpen && (
                <TransferModal
                    playerName={playerName}
                    identity={playerKeys}
                    availableNotes={availableNotesForTransfer}
                    onClose={handleCloseTransferModal}
                />
            )}
            {debugMode && (
                <DebugNotesPanel notes={storedNotes} playerName={playerName} identity={playerKeys} />
            )}
        </div>
    );
}

export default App;
