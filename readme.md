
# Kamino Liquidator (liquidator)

A lightweight Rust-based tool to collect Kamino lending obligations and enrich them with consolidated Pyth price data.

This project scans a Kamino lending market, finds obligations with active borrows, maps reserves to token mints, starts a Pyth price listener, and periodically writes a JSON snapshot (`obligations_with_pyth_prices.json`) containing live prices and obligation details.

## Key features

- Fetches all obligations for a Kamino lending market (fallbacks to program-account scan when necessary).
- Filters obligations to those with active borrows.
- Resolves reserve -> token mint mappings in batches.
- Starts a consolidated Pyth price listener for all discovered token mints.
- Produces a periodic JSON snapshot of obligations enriched with live Pyth prices.

## Requirements

- Rust (managed with rustup). The repository includes `rust-toolchain.toml` to pin the toolchain.
- Network access to a Solana RPC node.
- (Optional) Node/npm if you intend to inspect or run the TypeScript helper `market.ts` — not required to run the Rust service.

## Environment

This application expects an environment variable `RPC_URL` pointing to a Solana RPC endpoint (mainnet or an appropriate cluster).

Create a `.env` file in the `liquidator_arsen` directory or export the variable in your shell. Example `.env`:

```
RPC_URL=https://api.mainnet-beta.solana.com
```

## Build & run

1. Enter the project folder:

```
cd liquidator_arsen
```

2. Build (optional):

```
cargo build --release
```

3. Run the service (ensure `RPC_URL` is set):

```
cargo run --release
```

When run, the binary logs show startup messages including:

```
Starting Kamino Liquidator with CONSOLIDATED Pyth Price Listener
```

The service waits ~30s after starting the price listener to collect REAL Pyth prices, then continuously updates `obligations_with_pyth_prices.json` every ~20s with the latest obligation and price information.

## Output

- `obligations_with_pyth_prices.json` — a pretty-printed JSON file produced in the working directory. Each entry contains:
	- obligation address and owner
	- deposited and borrowed values
	- lists of deposits and borrows with token mint, symbol, amounts, market values
	- live Pyth price objects (price, confidence, status, last_updated)

## Important files

- `Cargo.toml` — Rust dependencies and metadata.
- `src/main.rs` — Entrypoint: orchestrates obligation discovery, reserve mapping, and price listener.
- `src/utils.rs` — Utility functions (obligation fetch/filter, reserve mapping).
- `src/price_listener.rs` — Consolidated Pyth price listener and helpers.
- `src/kamino.rs`, `listener.rs`, `obligation.rs` — domain-specific parsing/logic for Kamino obligations and accounts.
- `market.ts` — TypeScript helper (inspect-only; not required to run the Rust service).
- `obligations_with_pyth_prices.json` — sample/output file produced by the service.

## Configuration & notes

- The program uses a 30s warm-up to collect Pyth prices before the first enriched snapshot. Afterward, snapshots are written every 20s (these values are currently hard-coded in `main.rs`).
- The code expects certain program and market Pubkeys to be set in `main.rs` (currently hard-coded). If you need different markets, update those Pubkeys in the source.
- If you run against a non-mainnet RPC endpoint, ensure the Pyth feeds you expect are available on that cluster.

## Troubleshooting

- "RPC_URL must be set": export `RPC_URL` or create `.env` as shown above.
- If no obligations are found, the service attempts a fallback program-account scan; ensure your RPC endpoint has sufficient access to historical data.
- If you see many `No Pyth Data` entries, the token mints discovered may not have Pyth feeds available.

## Contributing

Contributions, bug reports and PRs are welcome. Please open issues or PRs with clear reproduction steps and logs.



## Portfolio note — status & retrospective

Status: archived / unfinished

I built this project as a live liquidator prototype for the Kamino lending market. Early on it showed a real opportunity: the system was capable of discovering undercollateralized obligations, mapping reserve accounts to token mints, and enriching positions with real-time Pyth prices — all the building blocks needed for an automated liquidator. At the time it could have been profitable.

Over time the opportunity closed: market conditions, on-chain dynamics, and protocol changes made running a profitable liquidator against Kamino impractical. Because of that, I stopped active development and this repository is provided here as a portfolio piece and technical proof-of-concept rather than a production tool.

What is implemented

- Obligation discovery and fallback scanning for a Kamino lending market.
- Reserve -> token mint mapping with batched RPC lookups.
- Consolidated Pyth price listener and an in-memory price cache.
- Periodic JSON snapshots (`obligations_with_pyth_prices.json`) that combine obligation state and live prices.

What is unfinished / risks if you try to run it now

- No on-chain liquidation execution flow: the code does not attempt to perform liquidations automatically.
- Hard-coded program and market Pubkeys in `src/main.rs`; switching markets requires code changes.
- No production-grade error handling, retry/backoff logic, or rate-limiting for aggressive RPC usage.
- Running against mainnet may be costly and is unlikely to be profitable without further refinements.

Lessons learned

- Timing and market microstructure matter: a technically correct liquidator can still lose money if fees, slippage, and competition erode the edge.
- Reliable, low-latency price feeds are critical — a warm-up time and local aggregation help but are not a complete solution.
- Building on-chain tooling requires careful attention to RPC rate limits and the cost of repeated fetches.

How to present this in a portfolio

- Treat this repository as a systems engineering example: it shows how to wire on-chain account scans, parsing, mapping, and price feed aggregation in Rust.
- Include high-level notes in your portfolio about why the project was archived (honest post-mortem), which makes the case that you can evaluate both technical and economic factors.

Suggested next steps (if you want to continue development)

1. Implement a simulated backtester that can replay historical blocks and test liquidation profitability deterministically.
2. Add a dry-run liquidation planner that estimates gas/fee/slippage before attempting on-chain execution.
3. Parameterize program/market Pubkeys and snapshot timings via config/env.
4. Add CI that builds the project and runs a small unit test battery.

If you'd like, I can add a small `.env.example`, a short backtesting harness skeleton, or a CI workflow that builds the repo; tell me which and I'll add it.
