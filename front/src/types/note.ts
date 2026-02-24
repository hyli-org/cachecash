export interface PrivateNote {
    kind:     string;  // 64-char hex – token type (same value as contract)
    contract: string;  // 64-char hex – token identifier
    address:  string;  // 64-char hex – UTXO address = poseidon2([zkSecretKey, 0], 2)
    psi:      string;  // 64-char hex – random nonce (unique per note)
    value:    string;  // 64-char hex – amount big-endian
}

export interface StoredNote {
    txHash:   string;
    note:     PrivateNote;
    storedAt: number;
    player:   string;
}
