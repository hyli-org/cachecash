import { getServerBaseUrl } from "./ConfigService";

interface ResolveAddressResponse {
    username: string;
    utxo_address: string;
    encryption_pubkey: string;
    registered_at: number;
}

class AddressService {
    private buildUrl(path: string): string {
        const base = getServerBaseUrl().replace(/\/$/, "");
        return `${base}${path}`;
    }

    async register(username: string, utxoAddress: string, encryptionPubkey: string): Promise<void> {
        const response = await fetch(this.buildUrl("/api/address/register"), {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
                username,
                utxo_address: utxoAddress,
                encryption_pubkey: encryptionPubkey,
            }),
        });

        if (!response.ok) {
            throw new Error(`Address registration failed with status ${response.status}`);
        }
    }

    async resolve(username: string): Promise<{ utxoAddress: string; encryptionPubkey: string }> {
        const response = await fetch(this.buildUrl(`/api/address/resolve/${encodeURIComponent(username)}`));

        if (response.status === 404) {
            throw new Error(`User "${username}" not found. They need to log in at least once to be discoverable.`);
        }

        if (!response.ok) {
            throw new Error(`Address resolution failed with status ${response.status}`);
        }

        const data = (await response.json()) as ResolveAddressResponse;
        return {
            utxoAddress: data.utxo_address,
            encryptionPubkey: data.encryption_pubkey,
        };
    }
}

export const addressService = new AddressService();
