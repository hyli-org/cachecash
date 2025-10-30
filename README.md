# CacheCash

<div align="center">

  <a href="https://hyli.org/">
    <img src="https://github.com/hyli-org/hyli-assets/blob/main/Logos/Logo/HYLI_WORDMARK_ORANGE.png?raw=true" width="320" alt="Hyli">
  </a>

_**CacheCash is a high‑TPS private transfer minigame built on [Hyli](https://hyli.org)**._

_**[Play CacheCash now!](https://cachecash.testnet.hyli.org/)**_

[![Telegram Chat][tg-badge]][tg-url]
[![Twitter][twitter-badge]][twitter-url]
</div>

## Overview

CacheCash showcases **private, high-speed payments** powered by proofs on Hyli.

## Key features

- 🕶️ Private balances & transfers powered by zero-knowledge proofs
- ⚡ Instant local interactions with onchain finality
- 🧱 Composes Noir for privacy, SP1 for speed
- 💧 Fully private faucet: claim and spend without revealing balances
- 🎲 Simple, fast, and fully private gameplay
- 🔒 All data stays on your device

## Links

- 🎮 [Play the game](https://cachecash.testnet.hyli.org)
- 📘 [Read the deep dive](https://blog.hyli.org/launching-cachecash)
- 🧰 [Learn about Hyli](https://docs.hyli.org)

## How to run

Prerequisites:

- Node.js ≥ 18
- Rust ≥ 1.75
- [Noir](https://noir-lang.org/docs/getting_started/installation/)
- [Running Hyli devnet](https://docs.hyli.org/quickstart/run/)

### Local development

1. Start the Hyli devnet locally (or point to a remote devnet) so the node RPC is reachable at `http://127.0.0.1:4321` and the DA reader at `127.0.0.1:4141`.
2. (Optional) Copy the default server configuration if you want to tweak ports or node URLs:

   ```bash
   cp server/src/conf_defaults.toml config.toml
   ```

   You can also override individual keys at runtime with environment variables such as `CACHECASH__NODE_URL=http://devnet-host:4321`.
3. Run the CacheCash server from the repository root:

   ```bash
   cargo run --release --manifest-path server/Cargo.toml
   ```

   The first boot generates and caches the SP1 proving key under `data/hyli_utxo_state_pk.bin`, so expect an extra minute the very first time.
4. In a second terminal, install frontend dependencies (uses Bun to stay in sync with `bun.lockb`) and start the Vite dev server:

   ```bash
   cd front
   bun install
   bun run dev
   ```

   If you prefer npm, run `npm install` followed by `npm run dev`; the values in `front/.env` control which endpoints the UI talks to.
5. Open <http://localhost:5173> in your browser and start playing.

### Docker

- Backend:

  ```bash
  docker build -f Dockerfile.server -t cachecash-server .
  docker run --network host cachecash-server
  ```

  Supply a custom `config.toml` via a bind mount (e.g. `-v $(pwd)/config.toml:/app/config.toml`) or environment variables if the devnet is not on localhost.
- Frontend:

  ```bash
  docker build -f Dockerfile.ui -t cachecash-ui .
  docker run -p 5173:80 cachecash-ui
  ```

  Set `VITE_*` variables with `-e` flags when you need the UI to target non-default endpoints.

## Inspired by

- [Payy](https://docs.payy.network/payy-network/whitepaper)
- [ZCash](https://z.cash/)

[twitter-badge]: https://img.shields.io/twitter/follow/hyli_org
[twitter-url]: https://x.com/hyli_org
[tg-badge]: https://img.shields.io/endpoint?url=https%3A%2F%2Ftg.sumanjay.workers.dev%2Fhyli_org%2F&logo=telegram&label=chat&color=neon
[tg-url]: https://t.me/hyli_org
