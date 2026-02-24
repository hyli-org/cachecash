import { Barretenberg, Fr } from "@aztec/bb.js";

class Poseidon2Service {
    private bb: Barretenberg | null = null;

    async init(): Promise<void> {
        if (this.bb) return;
        this.bb = await Barretenberg.new();
    }

    /** inputs: 64-char hex field elements; returns 64-char hex result */
    async hash(inputs: string[]): Promise<string> {
        if (!this.bb) await this.init();
        const frs = inputs.map((h) => new Fr(BigInt("0x" + h.replace(/^0x/i, ""))));
        const result = await this.bb!.poseidon2Hash(frs);
        return result.toString().replace(/^0x/, "").padStart(64, "0");
    }
}

export const poseidon2Service = new Poseidon2Service();
