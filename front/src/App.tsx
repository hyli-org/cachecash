import { useState, useEffect, useCallback, useMemo } from "react";
import "./App.css";
import { deriveZkSecretKey, deriveUtxoAddress, FullIdentity } from "./services/KeyService";
import SHA256 from "crypto-js/sha256";
import { addressService } from "./services/AddressService";
import { getNodeBaseUrl, getWalletServerBaseUrl, getWalletWebsocketUrl, getIndexerBaseUrl } from "./services/ConfigService";

import { TransactionList } from "./components/TransactionList";
import { DebugNotesPanel } from "./components/DebugNotesPanel";
import { ManageNotesModal } from "./components/ManageNotesModal";
import { TransferModal } from "./components/TransferModal";
import { DepositModal } from "./components/DepositModal";
import { WithdrawModal } from "./components/WithdrawModal";
import { transferService, parseNoteValue } from "./services/TransferService";
import { nodeService } from "./services/NodeService";
import { addStoredNote } from "./services/noteStorage";
import { declareCustomElement } from "testnet-maintenance-widget";
import { useStoredNotes } from "./hooks/useStoredNotes";
import { useDebugMode } from "./hooks/useDebugMode";
import { useEncryptedNotes } from "./hooks/useEncryptedNotes";
import { PrivateNote, StoredNote } from "./types/note";
import { HyliWallet, WalletProvider, useWallet } from "hyli-wallet";
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

function AppContent() {
    const debugMode = useDebugMode();
    const { wallet, logout, getEthereumProvider } = useWallet();
    const playerName = wallet?.username ?? "";

    const { notes: storedNotes } = useStoredNotes(playerName);
    const [isManageModalOpen, setIsManageModalOpen] = useState(false);
    const [isTransferModalOpen, setIsTransferModalOpen] = useState(false);
    const [isDepositModalOpen, setIsDepositModalOpen] = useState(false);
    const [isWithdrawModalOpen, setIsWithdrawModalOpen] = useState(false);
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
        return transferService.getSpendableNotes(
            storedNotes,
            playerKeys.zkSecretKey,
            playerKeys.utxoAddress,
            playerName,
        );
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

    // Derive zkSecretKey/utxoAddress only from the connected Ethereum wallet.
    // The seed is cached in localStorage so the signing prompt only appears once per browser.
    useEffect(() => {
        if (!wallet?.sessionKey) {
            setPlayerKeys(null);
            return;
        }

        const sk = wallet.sessionKey;
        let cancelled = false;
        let retryTimeout: number | undefined;

        const ZK_SEED_CACHE_PREFIX = "cachecash:zk-seed:v2:";
        const ETH_ACCOUNT_CACHE_PREFIX = "cachecash:eth-account:";

        const getEthAccountCacheKey = () => {
            const providerUuid = wallet.ethereumProviderUuid ?? "default";
            return `${ETH_ACCOUNT_CACHE_PREFIX}${wallet.username}:${providerUuid}`;
        };

        const getSeedCacheKey = (ethAddress: string) => {
            return ZK_SEED_CACHE_PREFIX + SHA256(ethAddress.toLowerCase()).toString();
        };

        const deriveZkFromEthSignature = async (
            account: string,
            walletAddress: string,
        ): Promise<{ zkSecretKey: string; utxoAddress: string }> => {
            const ethAddress = account.toLowerCase();
            const cacheKey = getSeedCacheKey(ethAddress);
            const provider = getEthereumProvider();

            if (!provider) {
                throw new Error("ethereum provider unavailable");
            }

            let seed = localStorage.getItem(cacheKey);
            if (!seed) {
                const message =
                    `CacheCash identity seed v2\n\nWallet: ${walletAddress}\nAccount: ${ethAddress}\n\n` +
                    `This signature derives your private CacheCash identity key. ` +
                    `It will never be broadcast to any network.`;
                const signature = await (provider!.request({
                    method: "personal_sign",
                    params: [message, account],
                }) as Promise<string>);
                seed = SHA256(signature).toString();
                localStorage.setItem(cacheKey, seed);
            }

            const zkSecretKey = await deriveZkSecretKey(seed);
            const utxoAddress = await deriveUtxoAddress(zkSecretKey);
            return { zkSecretKey, utxoAddress };
        };

        const deriveZkFromCachedSeed = async (account: string): Promise<{ zkSecretKey: string; utxoAddress: string }> => {
            const ethAddress = account.toLowerCase();
            const seed = localStorage.getItem(getSeedCacheKey(ethAddress));

            if (!seed) {
                throw new Error("cached ethereum seed unavailable");
            }

            const zkSecretKey = await deriveZkSecretKey(seed);
            const utxoAddress = await deriveUtxoAddress(zkSecretKey);
            return { zkSecretKey, utxoAddress };
        };

        const resolveIdentity = async () => {
            const provider = getEthereumProvider();

            if (!provider) {
                const cachedAccount = localStorage.getItem(getEthAccountCacheKey());
                if (cachedAccount) {
                    try {
                        const { zkSecretKey, utxoAddress } = await deriveZkFromCachedSeed(cachedAccount);
                        if (cancelled) return;

                        const identity: FullIdentity = {
                            privateKey: sk.privateKey,
                            publicKey: sk.publicKey,
                            zkSecretKey,
                            utxoAddress,
                        };
                        setPlayerKeys(identity);
                        return;
                    } catch {
                        // Fall through and retry once the provider becomes available.
                    }
                }

                retryTimeout = window.setTimeout(() => {
                    if (!cancelled) {
                        void resolveIdentity();
                    }
                }, 500);
                return;
            }

            try {
                const [account] = (await provider.request({ method: "eth_accounts" })) as string[];
                if (!account) throw new Error("no eth account");
                localStorage.setItem(getEthAccountCacheKey(), account.toLowerCase());

                const { zkSecretKey, utxoAddress } = await deriveZkFromEthSignature(account, wallet.address ?? account);
                if (cancelled) return;

                const identity: FullIdentity = {
                    privateKey: sk.privateKey,
                    publicKey: sk.publicKey,
                    zkSecretKey,
                    utxoAddress,
                };
                setPlayerKeys(identity);
                addressService
                    .register(wallet.username, utxoAddress, sk.publicKey)
                    .catch((error) => console.warn("Address registration failed:", error));
            } catch (error) {
                console.error("Failed to derive identity", error);
                if (!cancelled) setPlayerKeys(null);
            }
        };

        void resolveIdentity();

        return () => {
            cancelled = true;
            if (retryTimeout !== undefined) {
                window.clearTimeout(retryTimeout);
            }
        };
    }, [getEthereumProvider, wallet?.sessionKey?.publicKey, wallet?.username, wallet?.ethereumProviderUuid]);

    useEffect(() => {
        document.documentElement.dataset.theme = theme;
        localStorage.setItem("theme", theme);
    }, [theme]);

    const handleToggleTheme = useCallback(() => {
        setTheme((t) => (t === "dark" ? "light" : "dark"));
    }, []);

    const handleLogout = useCallback(() => {
        logout();
    }, [logout]);

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

    const handleOpenDepositModal = useCallback(() => {
        if (!playerName) return;
        setIsDepositModalOpen(true);
    }, [playerName]);

    const handleCloseDepositModal = useCallback(() => {
        setIsDepositModalOpen(false);
    }, []);

    const handleOpenWithdrawModal = useCallback(() => {
        if (!playerName) return;
        setIsWithdrawModalOpen(true);
    }, [playerName]);

    const handleCloseWithdrawModal = useCallback(() => {
        setIsWithdrawModalOpen(false);
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

            {!wallet && (
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

                        <div className="login-form">
                            <HyliWallet providers={["password", "ethereum"]} />
                        </div>
                    </div>
                </div>
            )}

            {wallet && (
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
                                    className="btn btn-secondary"
                                    onClick={handleOpenDepositModal}
                                    disabled={faucetStatus === "loading" || !playerKeys}
                                >
                                    Deposit
                                </button>
                                <button
                                    type="button"
                                    className="btn btn-secondary"
                                    onClick={handleOpenWithdrawModal}
                                    disabled={faucetStatus === "loading" || !playerKeys}
                                >
                                    Withdraw
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
            {isDepositModalOpen && (
                <DepositModal
                    playerName={playerName}
                    walletAddress={wallet?.address ?? ""}
                    identity={playerKeys}
                    onClose={handleCloseDepositModal}
                />
            )}
            {isWithdrawModalOpen && (
                <WithdrawModal
                    playerName={playerName}
                    walletAddress={wallet?.address ?? ""}
                    identity={playerKeys}
                    availableNotes={availableNotesForTransfer}
                    onClose={handleCloseWithdrawModal}
                />
            )}
            {debugMode && (
                <DebugNotesPanel notes={storedNotes} onClear={() => {}} />
            )}
        </div>
    );
}

function App() {
    return (
        <WalletProvider
            config={{
                nodeBaseUrl: getNodeBaseUrl(),
                walletServerBaseUrl: getWalletServerBaseUrl(),
                applicationWsUrl: getWalletWebsocketUrl(),
                indexerBaseUrl: getIndexerBaseUrl(),
            }}
            sessionKeyConfig={{
                duration: 24 * 60 * 60 * 1000, // 24 hours
            }}
            forceSessionKey={true}
        >
            <AppContent />
        </WalletProvider>
    );
}

export default App;
