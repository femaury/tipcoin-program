# Tipcoin Program

An Anchor-based Solana program that powers Tipbot’s delegated escrow flow. The
program lets Discord users deposit SPL tokens into deterministic vault PDAs,
assign spend allowances, tip other users instantly through an off-chain
relayer, and settle everything on-chain with auditable events.

## Highlights

- **Deterministic vaults & allowances** – Each Discord account hash maps to a
  vault PDA plus an allowance PDA that caps spend for relayed tips.
- **Event-driven design** – Every deposit, allowance change, tip, and withdraw
  emits structured events so indexers (e.g., `escrow-log-processor`) can stay in
  sync without replaying instructions manually.
- **Configurable fees & relayer** – The upgrade authority can rotate the
  relayer pubkey and adjust fee bps (capped at 100 bps) without redeploying.
- **Scripts & tests included** – TypeScript scripts to bootstrap config plus a
  Mocha test suite that exercises the full flow end-to-end.

## Repository layout

```
Anchor.toml                 Anchor workspace definition
Cargo.toml                  Workspace that declares `programs/tipcoin`
programs/tipcoin/src/       Rust program sources
programs/tipcoin/tests/     Mocha e2e suite (runs via `pnpm run test:local`)
scripts/                    TS helpers (init config, update fees, etc.)
tsconfig.json               Shared TypeScript config for scripts/tests
target/idl|types/           Generated artifacts after `anchor build`
```

## Prerequisites

- Rust toolchain (1.75+ recommended) and `cargo`
- Solana CLI (`solana --version` ≥ 1.18)
- Anchor CLI 0.32.x (`anchor --version`)
- Node 18+ with `pnpm` (tests/scripts use ES modules)
- Access to a funded keypair for deploying and running tests

## Getting started

```bash
git clone git@github.com:YOURORG/tipcoin.git
cd tipcoin
pnpm install           # installs script + test deps
anchor build           # compiles the program, emits IDL + types
```

`anchor build` writes artifacts to `target/idl/tipcoin.json`,
`target/types/tipcoin.ts`, and `target/deploy/tipcoin.so`. Downstream projects
(for example `apps/indexer`) import the IDL via the `#tipcoin-idl` alias, so make
sure the generated files stay current. If you need to ship a pinned snapshot,
copy the output into a release bundle or publish it as an npm package.

## Running tests

The suite spins up a local validator via Anchor and runs through the full user
journey.

```bash
pnpm run test:local    # runs programs/tipcoin/tests/tipcoin.ts
```

Override the wallet or cluster by exporting `ANCHOR_WALLET` /
`ANCHOR_PROVIDER_URL` as usual.

## Deployment workflow

1. Point `Anchor.toml` `[programs.<cluster>]` entries at the new program ID.
2. Ensure the deployer keypair (upgrade authority) is funded on the target
   cluster.
3. Build and deploy:
   ```bash
   anchor build
   anchor deploy --provider.cluster devnet
   ```
4. Record the new ID in both `Anchor.toml` and any downstream consumers (bots,
   indexers, config scripts).

## Reproducible / verified builds

To supply Solana with a verified build:

```bash
anchor build -- --features cpi        # ensure the exact feature set
shasum -a 256 target/deploy/tipcoin.so
```

Submit the resulting hash alongside the program ID. Auditors can reproduce the
same binary by cloning this repo, running `anchor build` with the same Anchor
version, and comparing the SHA-256 signature. The IDL (`idl/tipcoin.json`)
should match what the program emits at runtime; CI can enforce this by
diff-checking `target/idl` against the committed `idl/` directory.

## Key accounts & events

- `Config` PDA stores upgrade authority, relayer, SPL mint, and fee settings.
- `Vault` PDAs hold user funds (seed: `["vault", hashed_user_id]`).
- `Allowance` PDAs control per-user delegated spend caps.
- `FeeVault` PDA escrows protocol fees (seed: `["fee_vault", config]`).
- Events: `DepositEvent`, `AllowanceUpdated`, `TipEvent`, `WithdrawEvent`, plus
  logs from fee withdrawals & admin actions. Downstream services should listen
  for these events rather than parsing instructions manually.

## Scripts & downstream usage

- `scripts/init-config.ts` – initializes the config PDA with relayer, mint, and
  fee settings.
- `scripts/set-fees.ts` – upgrades fee bps post-deployment.

Both require `ANCHOR_CONFIG` or CLI flags pointing at the correct cluster and
keypair; see the script sources for argument details. After each build,
downstream services can read the fresh IDL/types from `target/idl` and
`target/types` via the shared alias (`#tipcoin-idl` / `#tipcoin-types`).

## Contributing

1. Fork & branch.
2. Run `anchor fmt` / `cargo fmt`, `pnpm test:local`, and `anchor build`.
3. Ensure `idl/tipcoin.json` matches `target/idl/tipcoin.json`.
4. Open a PR describing the change, expected behavior, and any migration steps.

Please open a GitHub issue for questions about verification, deployments, or
additional event hooks you need for integrations.
