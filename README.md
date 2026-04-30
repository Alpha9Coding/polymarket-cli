# Polymarket CLI

Rust CLI for Polymarket. Browse markets, place orders, manage positions, and interact with onchain contracts — from a terminal or as a JSON API for scripts and agents.

> **Fork notice:** This is a fork of [Polymarket/polymarket-cli](https://github.com/Polymarket/polymarket-cli) updated to use the Polymarket V2 CLOB SDK ([`polymarket_client_sdk_v2`](https://github.com/Polymarket/rs-clob-client-v2)). Polymarket V2 went live on 2026-04-28; v1 SDKs no longer work against production. See [docs.polymarket.com/v2-migration](https://docs.polymarket.com/v2-migration) for context.

> **Warning:** This is early, experimental software. Use at your own risk and do not use with large amounts of funds. APIs, commands, and behavior may change without notice. Always verify transactions before confirming.

## Install

### Build from source (recommended)

```bash
git clone https://github.com/Alpha9Coding/polymarket-cli
cd polymarket-cli
cargo install --path .
```

This installs the `polymarket` binary to `~/.cargo/bin/`. Make sure that directory is on your `PATH`.

### Homebrew (macOS / Linux)

> Available after this fork cuts a release.

```bash
brew tap Alpha9Coding/polymarket-cli https://github.com/Alpha9Coding/polymarket-cli
brew install polymarket
```

### Shell script

> Available after this fork cuts a release.

```bash
curl -sSL https://raw.githubusercontent.com/Alpha9Coding/polymarket-cli/main/install.sh | sh
```

## Claude Code skill (for AI agents)

This repo ships a [Claude Code](https://claude.com/claude-code) skill that teaches Claude how to drive the `polymarket` CLI — when to use which subcommand, how to pipe JSON output, the v2-specific gotchas (pUSD, API key re-derivation, etc.). After installing the skill, ask Claude things like *"check my Polymarket positions"*, *"show the order book for the Bitcoin $100K market"*, or *"split $10 pUSD into YES/NO tokens for condition 0x…"* and the skill will fire automatically.

Two install methods — both pull the skill files from this repo, so updates land via `git pull`:

**1. As a Claude Code plugin** (single command, auto-discoverable)

```bash
# Inside Claude Code
/plugin add github:Alpha9Coding/polymarket-cli
```

**2. Manual symlink** (always works)

```bash
git clone https://github.com/Alpha9Coding/polymarket-cli   # if you don't already have it
ln -s "$(pwd)/polymarket-cli/skills/polymarket-cli" ~/.claude/skills/polymarket-cli
```

After install, restart Claude Code (or type `/help` to verify the skill is listed). The `polymarket` binary itself must already be installed via one of the methods above — the skill teaches the agent to use the CLI, it doesn't bundle the CLI.

The skill source is at [skills/polymarket-cli/SKILL.md](skills/polymarket-cli/SKILL.md). Plugin metadata lives at [.claude-plugin/plugin.json](.claude-plugin/plugin.json).

## Quick Start

```bash
# No wallet needed — browse markets immediately
polymarket markets list --limit 5
polymarket markets search "election"
polymarket events list --tag politics

# Check a specific market
polymarket markets get will-trump-win-the-2024-election

# JSON output for scripts
polymarket -o json markets list --limit 3
```

To trade, set up a wallet:

```bash
polymarket setup
# Or manually:
polymarket wallet create
polymarket approve set
```

## Configuration

### Wallet Setup

The CLI needs a private key to sign orders and on-chain transactions. Three ways to provide it (checked in this order):

1. **CLI flag**: `--private-key 0xabc...`
2. **Environment variable**: `POLYMARKET_PRIVATE_KEY=0xabc...`
3. **Config file**: `~/.config/polymarket/config.json`

```bash
# Create a new wallet (generates random key, saves to config)
polymarket wallet create

# Import an existing key
polymarket wallet import 0xabc123...

# Check what's configured
polymarket wallet show
```

The config file (`~/.config/polymarket/config.json`):

```json
{
  "private_key": "0x...",
  "chain_id": 137,
  "signature_type": "proxy"
}
```

### Signature Types

- `proxy` (default) — uses Polymarket's proxy wallet system
- `eoa` — signs directly with your key
- `gnosis-safe` — for multisig wallets

Override per-command with `--signature-type eoa` or via `POLYMARKET_SIGNATURE_TYPE`.

### What Needs a Wallet

Most commands work without a wallet — browsing markets, viewing order books, checking prices. You only need a wallet for:

- Placing and canceling orders (`clob create-order`, `clob market-order`, `clob cancel-*`)
- Checking your balances and trades (`clob balance`, `clob trades`, `clob orders`)
- On-chain operations (`approve set`, `ctf split/merge/redeem`)
- Reward and API key management (`clob rewards`, `clob create-api-key`)

### Trading prerequisites (first-time setup)

Before placing your first order, do these once in this exact order:

```bash
# 1. Wallet config (sets ~/.config/polymarket/config.json)
polymarket setup

# 2. Bootstrap L2 API credentials. NOTE: Polymarket's Cloudflare WAF blocks
#    POST /auth/* from datacenter IPs (AWS, GCP, etc.) with a 403. If you hit
#    this, run create-api-key from a residential IP (your laptop / home) and
#    then scp ~/.config/polymarket/config.json to the server.
polymarket clob create-api-key

# 3. Deposit pUSD to your proxy wallet (address printed by `polymarket setup`).
#    Use polymarket bridge or transfer pUSD directly on Polygon.

# 4. Send on-chain approvals so the V1 + V2 CTF exchanges can move your pUSD
#    and outcome tokens. ~8 transactions, requires MATIC for gas. WITHOUT
#    this step every trade will revert.
polymarket approve set

# 5. Verify
polymarket clob balance --asset-type collateral   # should show your pUSD
polymarket approve check                           # all rows ✓ Approved
```

After step 4, ordinary trading endpoints (`POST /order`, `/cancel`, …) are not WAF-blocked, so trading from a server is fine — only the one-time API key bootstrap (`/auth/*` POSTs) needs to happen from a residential IP.

## Output Formats

Every command supports `--output table` (default) and `--output json`.

```bash
# Human-readable table (default)
polymarket markets list --limit 2
```

```
 Question                            Price (Yes)  Volume   Liquidity  Status
 Will Trump win the 2024 election?   52.00¢       $145.2M  $1.2M      Active
 Will BTC hit $100k by Dec 2024?     67.30¢       $89.4M   $430.5K    Active
```

```bash
# Machine-readable JSON
polymarket -o json markets list --limit 2
```

```json
[
  { "id": "12345", "question": "Will Trump win the 2024 election?", "outcomePrices": ["0.52", "0.48"], ... },
  { "id": "67890", "question": "Will BTC hit $100k by Dec 2024?", ... }
]
```

Short form: `-o json` or `-o table`.

Errors follow the same pattern — table mode prints `Error: ...` to stderr, JSON mode prints `{"error": "..."}` to stdout. Non-zero exit code either way.

## Commands

### Markets

```bash
# List markets with filters
polymarket markets list --limit 10
polymarket markets list --active true --order volume_num
polymarket markets list --closed false --limit 50 --offset 25

# Get a single market by ID or slug
polymarket markets get 12345
polymarket markets get will-trump-win

# Search
polymarket markets search "bitcoin" --limit 5

# Get tags for a market
polymarket markets tags 12345
```

**Flags for `markets list`**: `--limit`, `--offset`, `--order`, `--ascending`, `--active`, `--closed`

### Events

Events group related markets (e.g. "2024 Election" contains multiple yes/no markets).

```bash
polymarket events list --limit 10
polymarket events list --tag politics --active true
polymarket events get 500
polymarket events tags 500
```

**Flags for `events list`**: `--limit`, `--offset`, `--order`, `--ascending`, `--active`, `--closed`, `--tag`

### Tags, Series, Comments, Profiles, Sports

```bash
# Tags
polymarket tags list
polymarket tags get politics
polymarket tags related politics
polymarket tags related-tags politics

# Series (recurring events)
polymarket series list --limit 10
polymarket series get 42

# Comments on an entity
polymarket comments list --entity-type event --entity-id 500
polymarket comments get abc123
polymarket comments by-user 0xf5E6...

# Public profiles
polymarket profiles get 0xf5E6...

# Sports metadata
polymarket sports list
polymarket sports market-types
polymarket sports teams --league NFL --limit 32
```

### Order Book & Prices (CLOB)

All read-only — no wallet needed.

```bash
# Check API health
polymarket clob ok

# Prices
polymarket clob price 48331043336612883... --side buy
polymarket clob midpoint 48331043336612883...
polymarket clob spread 48331043336612883...

# Batch queries (comma-separated token IDs)
polymarket clob batch-prices "TOKEN1,TOKEN2" --side buy
polymarket clob midpoints "TOKEN1,TOKEN2"
polymarket clob spreads "TOKEN1,TOKEN2"

# Order book
polymarket clob book 48331043336612883...
polymarket clob books "TOKEN1,TOKEN2"

# Last trade
polymarket clob last-trade 48331043336612883...

# Market info
polymarket clob market 0xABC123...  # by condition ID
polymarket clob markets             # list all

# Price history
polymarket clob price-history 48331043336612883... --interval 1d --fidelity 30

# Metadata
polymarket clob tick-size 48331043336612883...
polymarket clob fee-rate 48331043336612883...
polymarket clob neg-risk 48331043336612883...
polymarket clob time
polymarket clob geoblock
```

**Interval options for `price-history`**: `1m`, `1h`, `6h`, `1d`, `1w`, `max`

### Trading (CLOB, authenticated)

Requires a configured wallet.

```bash
# Place a limit order (buy 10 shares at $0.50)
polymarket clob create-order \
  --token 48331043336612883... \
  --side buy --price 0.50 --size 10

# Place a market order (buy $5 worth)
polymarket clob market-order \
  --token 48331043336612883... \
  --side buy --amount 5

# Post multiple orders at once
polymarket clob post-orders \
  --tokens "TOKEN1,TOKEN2" \
  --side buy \
  --prices "0.40,0.60" \
  --sizes "10,10"

# Cancel
polymarket clob cancel ORDER_ID
polymarket clob cancel-orders "ORDER1,ORDER2"
polymarket clob cancel-market --market 0xCONDITION...
polymarket clob cancel-all

# View your orders and trades
polymarket clob orders
polymarket clob orders --market 0xCONDITION...
polymarket clob order ORDER_ID
polymarket clob trades

# Check balances
polymarket clob balance --asset-type collateral
polymarket clob balance --asset-type conditional --token 48331043336612883...
polymarket clob update-balance --asset-type collateral
```

**Order types**: `GTC` (default), `FOK`, `GTD`, `FAK`. Add `--post-only` for limit orders.

### Race-test runner (`clob race`, v0.3.0+)

Single-process harness for measuring CLOB submit/cancel timing with no server-roundtrip blocking. All orders are pre-built, signed, and have their on-chain `order_id` (the EIP-712 hash) computed locally **before** the timed loop starts, so a `cancel` step can fire before the prior `submit`'s response has come back.

```bash
YES=0x...        # YES outcome's CLOB token id
NO=0x...         # NO outcome's CLOB token id (complementary, sums to ~$1)

# Place a maker, wait 10ms, cancel — without waiting for the place response.
polymarket clob race \
  --order m1=$YES:buy:0.30:5 \
  --step submit:m1 --step wait:10 --step cancel:m1 \
  --warmup -o json
```

The DSL uses repeatable `--order LABEL=TOKEN:SIDE:PRICE:SIZE[:TYPE]` and `--step submit:LABEL | cancel:LABEL | wait:MS`. Each order gets its own token (so cross-book maker/taker scenarios via Polymarket's complementary mint mechanism are first-class). Add `--dry-run` to validate the plan + sign + derive `order_id` without submitting; add `--repeat N` to run the whole plan N times for distribution stats.

The output is structured JSON with `t0_unix_ms` plus per-action `t_send_us` / `t_recv_us` (monotonic from `Instant::now()`), plus the full server response per submit/cancel. See [skills/polymarket-cli/SKILL.md](skills/polymarket-cli/SKILL.md) for the four canonical race scenarios (cancel-before-place, FAK-vs-cancel, GTC-with-pre-cancel-maker, GTC-with-pre-cancel-taker).

### Rewards & API Keys (CLOB, authenticated)

```bash
polymarket clob rewards --date 2024-06-15
polymarket clob earnings --date 2024-06-15
polymarket clob earnings-markets --date 2024-06-15
polymarket clob reward-percentages
polymarket clob current-rewards
polymarket clob market-reward 0xCONDITION...

# Check if orders are scoring rewards
polymarket clob order-scoring ORDER_ID
polymarket clob orders-scoring "ORDER1,ORDER2"

# API key management
polymarket clob api-keys
polymarket clob create-api-key
polymarket clob delete-api-key

# Account status
polymarket clob account-status
polymarket clob notifications
polymarket clob delete-notifications "NOTIF1,NOTIF2"
```

### On-Chain Data

Public data — no wallet needed.

```bash
# Portfolio
polymarket data positions 0xWALLET_ADDRESS
polymarket data closed-positions 0xWALLET_ADDRESS
polymarket data value 0xWALLET_ADDRESS
polymarket data traded 0xWALLET_ADDRESS

# Trade history
polymarket data trades 0xWALLET_ADDRESS --limit 50

# Activity
polymarket data activity 0xWALLET_ADDRESS

# Market data
polymarket data holders 0xCONDITION_ID
polymarket data open-interest 0xCONDITION_ID
polymarket data volume 12345  # event ID

# Leaderboards
polymarket data leaderboard --period month --order-by pnl --limit 10
polymarket data builder-leaderboard --period week
polymarket data builder-volume --period month
```

### Contract Approvals

Before trading, Polymarket V2 contracts need ERC-20 (pUSD) and ERC-1155 (CTF token) approvals.

```bash
# Check current approvals (read-only)
polymarket approve check
polymarket approve check 0xSOME_ADDRESS

# Approve all contracts (sends 6 on-chain transactions, needs MATIC for gas)
polymarket approve set
```

### CTF Operations

Split, merge, and redeem conditional tokens directly on-chain.

```bash
# Split $10 pUSD into YES/NO tokens
polymarket ctf split --condition 0xCONDITION... --amount 10

# Merge tokens back to pUSD
polymarket ctf merge --condition 0xCONDITION... --amount 10

# Redeem winning tokens after resolution
polymarket ctf redeem --condition 0xCONDITION...

# Redeem neg-risk positions
polymarket ctf redeem-neg-risk --condition 0xCONDITION... --amounts "10,5"

# Calculate IDs (read-only, no wallet needed)
polymarket ctf condition-id --oracle 0xORACLE... --question 0xQUESTION... --outcomes 2
polymarket ctf collection-id --condition 0xCONDITION... --index-set 1
polymarket ctf position-id --collection 0xCOLLECTION...
```

`--amount` is in pUSD (e.g., `10` = $10). The `--partition` flag defaults to binary (`1,2`). On-chain operations require MATIC for gas on Polygon.

### Bridge

Deposit assets from other chains into Polymarket.

```bash
# Get deposit addresses (EVM, Solana, Bitcoin)
polymarket bridge deposit 0xWALLET_ADDRESS

# List supported chains and tokens
polymarket bridge supported-assets

# Check deposit status
polymarket bridge status 0xDEPOSIT_ADDRESS
```

### Wallet Management

```bash
polymarket wallet create               # Generate new random wallet
polymarket wallet create --force       # Overwrite existing
polymarket wallet import 0xKEY...      # Import existing key
polymarket wallet address              # Print wallet address
polymarket wallet show                 # Full wallet info (address, source, config path)
polymarket wallet reset                # Delete config (prompts for confirmation)
polymarket wallet reset --force        # Delete without confirmation
```

### Interactive Shell

```bash
polymarket shell
# polymarket> markets list --limit 3
# polymarket> clob book 48331043336612883...
# polymarket> exit
```

Supports command history. All commands work the same as the CLI, just without the `polymarket` prefix.

### Other

```bash
polymarket status     # API health check
polymarket setup      # Guided first-time setup wizard
polymarket upgrade    # Update to the latest version
polymarket --version
polymarket --help
```

## Common Workflows

### Browse and research markets

```bash
polymarket markets search "bitcoin" --limit 5
polymarket markets get bitcoin-above-100k
polymarket clob book 48331043336612883...
polymarket clob price-history 48331043336612883... --interval 1d
```

### Set up a new wallet and start trading

```bash
polymarket wallet create
polymarket approve set                    # needs MATIC for gas
polymarket clob balance --asset-type collateral
polymarket clob market-order --token TOKEN_ID --side buy --amount 5
```

### Monitor your portfolio

```bash
polymarket data positions 0xYOUR_ADDRESS
polymarket data value 0xYOUR_ADDRESS
polymarket clob orders
polymarket clob trades
```

### Place and manage limit orders

```bash
# Place order
polymarket clob create-order --token TOKEN_ID --side buy --price 0.45 --size 20

# Check it
polymarket clob orders

# Cancel if needed
polymarket clob cancel ORDER_ID

# Or cancel everything
polymarket clob cancel-all
```

### Script with JSON output

```bash
# Pipe market data to jq
polymarket -o json markets list --limit 100 | jq '.[].question'

# Check prices programmatically
polymarket -o json clob midpoint TOKEN_ID | jq '.mid'

# Error handling in scripts
if ! result=$(polymarket -o json clob balance --asset-type collateral 2>/dev/null); then
  echo "Failed to fetch balance"
fi
```

## Architecture

```
src/
  main.rs        -- CLI entry point, clap parsing, error handling
  auth.rs        -- Wallet resolution, RPC provider, CLOB authentication
  config.rs      -- Config file (~/.config/polymarket/config.json)
  shell.rs       -- Interactive REPL
  commands/      -- One module per command group
  output/        -- Table and JSON rendering per command group
```

## Contributing

After cloning, enable the in-repo pre-commit hook so local commits run the same gate as CI (`cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`):

```bash
git config core.hooksPath .githooks
```

The hook only fires when staged changes touch `*.rs` or `Cargo.{toml,lock}`, and you can bypass it with `git commit --no-verify` for WIP commits on a feature branch.

## License

MIT
