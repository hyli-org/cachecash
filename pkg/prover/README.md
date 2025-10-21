# Prover

Prover is responsible for generating aggregation proofs and submitting them to Ethereum.

## Testing the prover

To test the prover, start a local Ethereum node with `cd eth && npm run node` and deploy the contracts:

```
cd eth
npm run deploy -- --network localhost
```

Copy the `Rollup` contract address and set it as an environment variable `ROLLUP_CONTRACT_ADDR`:

```
export ROLLUP_CONTRACT_ADDR=0xcf7ed3acca5a467e9e704c703e8d87f634fb0fc9
```

Then run the tests:

```
cargo test
```
