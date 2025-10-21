# UTXO Mint Example

This crate demonstrates how to drive the Noir UTXO circuit from Rust and reproduce a mint
operation using the existing fixtures shipped with the repository.

## Scenario

- Two inputs are spent (values 15 and 5) that belong to the same address.
- A mint message increases the total output value by 10, resulting in outputs with values 1 and 29.
- The mint hash is derived exactly as in the Noir program (Poseidon2 over the outputs' `psi`s).

Running the binary serialises the inputs, executes the circuit via `barretenberg`, and prints the
public messages together with the size of the generated proof.

## Running

```bash
# Optionally re-use the cached protoc binary if network access is restricted.
PROTOC_ROOT=$(find target/debug/build -path '*protoc-29.3-linux-x86_64/bin/protoc' -print -quit)
if [ -n "$PROTOC_ROOT" ]; then
  export PROTOC_PREBUILT_FORCE_PROTOC_PATH="$PROTOC_ROOT"
  export PROTOC_PREBUILT_FORCE_INCLUDE_PATH="$(dirname "$PROTOC_ROOT")/../include"
fi

cargo run -p utxo-mint-example
```

If the cached `protoc` is not present yet, run any build once (e.g. `cargo run`) so that Rust pulls
the prebuilt binary into `target/debug/build`.

> **Note:** The example stops after proving. The CLI `bb verify` command expects a combined proof
> format and currently rejects the raw proof bytes emitted by `bb_cli`; wire verification is
> therefore not invoked automatically. The generated proof can still be inspected via
> `UtxoProof::to_bytes()` if needed.
