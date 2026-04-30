---
name: polymarket-cli
description: Use the `polymarket` CLI (Alpha9Coding/polymarket-cli fork, v2 SDK) to browse Polymarket markets/events/tags, query CLOB v2 prices/books/midpoints, place/cancel/inspect orders, check balances and approvals, manage wallet config, and run CTF split/merge/redeem operations from the terminal. Trigger this skill whenever the user asks to "browse markets", "check the order book", "place a trade on Polymarket", "see my Polymarket positions/orders/balances", "approve contracts", "set up a Polymarket wallet", "split/merge/redeem CTF tokens", or directly mentions the `polymarket` command. For raw on-chain data API queries (no CLI involved) prefer the `polymarket-data` skill instead.
metadata:
  { "openclaw": { "emoji": "📈", "os": ["darwin", "linux"], "requires": { "bins": ["polymarket"] } } }
---

# polymarket — Polymarket V2 CLI

CLI installed at `~/.cargo/bin/polymarket` (v0.2.0+, built from `Alpha9Coding/polymarket-cli`, depends on `polymarket_client_sdk_v2`). Talks to **Polymarket V2** (live since 2026-04-28) — uses pUSD as collateral, not USDC.e.

## Output Format

Every command supports `--output table` (default) and `--output json` (or `-o json`). Prefer JSON when piping to `jq`/`python` — table mode includes ANSI box drawing and truncation.

```bash
polymarket -o json markets list --limit 5 | jq '.[].question'
```

Errors: table mode prints `Error: ...` to stderr; JSON mode prints `{"error": "..."}` to stdout. Non-zero exit either way.

## Wallet / Auth

Three sources for the private key, checked in order: `--private-key 0x...` flag, `POLYMARKET_PRIVATE_KEY` env var, `~/.config/polymarket/config.json`.

```bash
polymarket setup                  # interactive guided setup
polymarket wallet create          # generate new key, save to config
polymarket wallet import 0xKEY    # import existing key
polymarket wallet show            # print address + config path
```

Signature types: `proxy` (default — uses Polymarket's proxy wallet system), `eoa` (sign directly with EOA), `gnosis-safe`. Override per-command with `--signature-type eoa` or env var `POLYMARKET_SIGNATURE_TYPE`.

**V2 caveat:** L1 API keys do NOT carry over from v1. After `setup`, run `polymarket clob create-api-key` to mint fresh L2 credentials before any authenticated CLOB call.

**Datacenter IP gotcha (verified 2026-04-30 on AWS / pm1)**: Polymarket's Cloudflare WAF blocks POST `/auth/*` from cloud-egress IPs (curl/Python/reqwest all return CF 403, regardless of UA). It's a JA3/IP-fingerprint block, not application-level. **Workaround**: run `polymarket clob create-api-key` once from a **residential IP** (laptop/home), then copy `~/.config/polymarket/config.json` (which now caches the L2 credentials) to the server. Trading endpoints (`POST /order`, `POST /cancel`, etc.) are NOT WAF-blocked — only `/auth/*` is. So once the API key is bootstrapped, server-side trading works fine.

The CLI's `create-api-key` command (v0.3.2+) splits the create + derive calls and surfaces both errors verbosely so this case is diagnosable; older versions silently swallow the create error and report only "Could not derive api key".

**Trading prerequisites (do these BEFORE first order placement)**:
1. `polymarket setup` — wallet config
2. `polymarket clob create-api-key` (from residential IP, see above) — L2 creds
3. Bridge / deposit pUSD to your proxy wallet (`polymarket setup` prints the address)
4. `polymarket approve set` — sends 8 on-chain txs (pUSD + CTF approvals for v1 + v2 exchanges). Requires MATIC for gas. **Without this, every trade reverts.**
5. Now you can `polymarket clob create-order` etc.

## Read-only Commands (no wallet needed)

### Markets / Events / Tags / Series (Gamma API)

```bash
polymarket markets list --limit 10 --active true --order volume_num
polymarket markets get <id-or-slug>          # by numeric ID or slug
polymarket markets search "bitcoin"
polymarket markets tags <market-id>

polymarket events list --tag politics --active true
polymarket events get <id>
polymarket tags list
polymarket tags related politics
polymarket series list
polymarket comments list --entity-type event --entity-id <id>
polymarket profiles get 0xADDR
polymarket sports list
polymarket sports teams --league NFL
```

### CLOB Read (prices, books, history)

```bash
polymarket clob ok                            # health check (hits clob-v2)
polymarket clob price <token-id> --side buy
polymarket clob midpoint <token-id>
polymarket clob spread <token-id>
polymarket clob book <token-id>               # full order book
polymarket clob last-trade <token-id>

polymarket clob batch-prices "TOKEN1,TOKEN2" --side buy
polymarket clob midpoints   "TOKEN1,TOKEN2"
polymarket clob spreads     "TOKEN1,TOKEN2"
polymarket clob books       "TOKEN1,TOKEN2"

polymarket clob market <condition-id>         # by 0x... condition
polymarket clob markets                       # list all v2 CLOB markets
polymarket clob simplified-markets

polymarket clob price-history <token-id> --interval 1d --fidelity 30
# interval values: 1m, 1h, 6h, 1d, 1w, max

polymarket clob tick-size <token-id>
polymarket clob fee-rate  <token-id>
polymarket clob neg-risk  <token-id>
polymarket clob time
polymarket clob geoblock
```

### On-chain Data (no auth, just an address)

```bash
polymarket data positions       0xWALLET
polymarket data closed-positions 0xWALLET
polymarket data value           0xWALLET
polymarket data trades          0xWALLET --limit 50
polymarket data activity        0xWALLET

# Sort flags (v0.2.1+) on positions / closed-positions:
#   positions       --sort-by  tokens (default) | current | initial | cash-pnl | percent-pnl
#   closed-positions --sort-by realized-pnl (default) | timestamp | title | price | avg-price
#   --sort-direction asc | desc (default desc)
#
# IMPORTANT: closed-positions defaults to REALIZED-PNL DESC, NOT timestamp.
# Without --sort-by timestamp the first page is the wallet's BIGGEST WINNERS
# all-time, which gives a wildly skewed PnL summary if you only fetch 50.
# For a true accounting, page through ALL closed positions OR sort by timestamp
# and bound by date.
polymarket data closed-positions 0xWALLET --sort-by timestamp --sort-direction desc --limit 50
polymarket data holders         0xCONDITION
polymarket data open-interest   0xCONDITION
polymarket data volume          <event-id>

polymarket data leaderboard --period month --order-by pnl --limit 10
polymarket data builder-leaderboard --period week
polymarket data builder-volume --period month
```

For richer on-chain analytics (raw HTTP API, custom aggregations), see the sibling `polymarket-data` skill.

## Authenticated Commands (wallet required)

### Trading

```bash
# Limit order (buy 10 shares at $0.50)
polymarket clob create-order --token <id> --side buy --price 0.50 --size 10

# Market order ($5 worth)
polymarket clob market-order --token <id> --side buy --amount 5

# Batch — same side, paired prices/sizes
polymarket clob post-orders --tokens "T1,T2" --side buy --prices "0.40,0.60" --sizes "10,10"

polymarket clob cancel <order-id>
polymarket clob cancel-orders "ID1,ID2"
polymarket clob cancel-market --market <condition-id>
polymarket clob cancel-all

polymarket clob orders                        # open orders
polymarket clob orders --market <condition-id>
polymarket clob order <order-id>
polymarket clob trades                        # trade history
```

Order types: `GTC` (default), `FOK`, `GTD`, `FAK`. Add `--post-only` to a limit order. Market orders default to `FOK`.

`--amount` for `market-order buy` is **pUSD** (not USDC). For `market-order sell` it's shares.

### Race-test runner (`clob race`, v0.3.0+; multi-signer v0.5.0+)

Single-process, single-tokio-runtime test harness for measuring submit/cancel timing without server roundtrip blocking. All orders are pre-signed and their EIP-712 hashes (the `order_id`) computed locally BEFORE timing starts, so a `cancel` step can fire by-id even before the prior `submit`'s response has come back.

```bash
polymarket clob race \
  [--signer LABEL=PRIVATE_KEY[:SIG_TYPE]] \      # repeatable; needed for cross-wallet matching
  --order LABEL=TOKEN:SIDE:PRICE:SIZE[:TYPE][@SIGNER] \   # repeatable
  --step submit:LABEL | cancel:LABEL | wait:MS \  # repeatable, ordered
  [--repeat N] [--warmup] [--dry-run] \
  [-o json]
```

**Order spec**: `LABEL=TOKEN:SIDE:PRICE:SIZE[:TYPE][@SIGNER]`
- LABEL: any string (e.g. `m1`, `t1`)
- TOKEN: full CLOB token id (`0x…` hex or large decimal). Per-order so cross-book / multi-market scenarios work.
- SIDE: `buy` | `sell`
- PRICE: decimal (e.g. `0.30`)
- SIZE: shares (e.g. `5`)
- TYPE: `GTC` (default) | `FOK` | `GTD` | `FAK`
- SIGNER (optional): `@LABEL` references a `--signer` definition. Required when `--signer` is used; forbidden when it isn't.

**Signer spec** (v0.5.0+): `LABEL=PRIVATE_KEY[:SIG_TYPE]`
- LABEL: any string (e.g. `alice`, `bob`)
- PRIVATE_KEY: `0x…` hex private key
- SIG_TYPE: `proxy` (default) | `eoa` | `gnosis-safe`. Per-signer, so one wallet can be EOA while another is proxy.

**Why multi-signer matters**: Polymarket forbids self-matching, so any test where a maker and a taker need to actually collide must use two different wallets. Single-signer mode (no `--signer`) is fine for scenario 1 (place → cancel one wallet); scenarios 2/3/4 require `--signer alice=… --signer bob=…` plus `@alice` / `@bob` on the orders.

**Step spec**: `submit:LABEL` | `cancel:LABEL` | `wait:MS`
- submit fires `tokio::spawn(post_order)` on the order's signer's authenticated client — does NOT block; the loop moves on
- cancel fires `tokio::spawn(cancel_order)` on the SAME order's signer (cancel is L2-authenticated so server checks ownership) — does NOT block
- wait blocks the step loop with `tokio::time::sleep` (precise to scheduler ~1ms)

After all steps the runner joins all spawned handles and emits a single JSON record with `t0_unix_ms`, the orders block (each tagged with `signer` + `maker` address + `order_id`), and an `actions` array with `t_send_us` / `t_recv_us` / `signer` per submit/cancel.

**Polymarket matching reminder**: to match a maker `BUY YES @ p`, a taker can either be `SELL YES @ ≤p` (same token, opposite side) OR `BUY NO @ ≥(1-p)` (different token, complementary mint match). For race tests of this complementary path, the maker and taker orders MUST come from different wallets (Polymarket rejects self-matching) and use **different** token ids.

**4 canonical scenarios**:

Single-signer (scenario 1 only):

```bash
YES=0x...   # YES token id

# 1. place → 10ms → cancel — tests "can we cancel before server acks our place?"
polymarket clob race \
  --order m1=$YES:buy:0.30:5 \
  --step submit:m1 --step wait:10 --step cancel:m1 \
  -o json
```

Multi-signer (scenarios 2/3/4 — alice = maker wallet, bob = taker wallet):

```bash
ALICE=0x...   # maker private key (must be pre-registered on Polymarket: web login + approve set)
BOB=0x...     # taker private key (same pre-reqs; bob's wallet needs the relevant tokens)
YES=0x...
NO=0x...

# 2. place maker → 10ms → place FAK taker → cancel maker
#    tests "does FAK still match a maker the user is racing to cancel?"
polymarket clob race \
  --signer alice=$ALICE \
  --signer bob=$BOB \
  --order maker=$YES:buy:0.30:5@alice \
  --order fak=$NO:buy:0.70:5:FAK@bob \
  --step submit:maker --step wait:10 --step submit:fak --step cancel:maker \
  --warmup -o json

# 3. place maker → 10ms → place GTC taker → cancel maker → 10ms → cancel GTC
polymarket clob race \
  --signer alice=$ALICE --signer bob=$BOB \
  --order maker=$YES:buy:0.30:5@alice \
  --order taker=$NO:buy:0.70:5@bob \
  --step submit:maker --step wait:10 --step submit:taker \
  --step cancel:maker --step wait:10 --step cancel:taker \
  --warmup -o json

# 4. same as 3 but cancel taker first, then maker
polymarket clob race \
  --signer alice=$ALICE --signer bob=$BOB \
  --order maker=$YES:buy:0.30:5@alice \
  --order taker=$NO:buy:0.70:5@bob \
  --step submit:maker --step wait:10 --step submit:taker \
  --step cancel:taker --step wait:10 --step cancel:maker \
  --warmup -o json
```

**Multi-signer prerequisites**: each `--signer` wallet needs to have already been bootstrapped (web login → API key registered on server, plus `polymarket approve set` from that wallet). The race command will L1+L2 authenticate each signer once during setup; if a wallet's API key isn't on the server, you'll get a 400 from `derive-api-key` per the v0.3.2 verbose error report.

**Useful flags**:
- `--dry-run` — build + sign + compute order_ids; print the plan; no server-side submit. Use to validate the spec end-to-end before live trading.
- `--warmup` — fires a cheap `clob ok` before timing starts to warm the TCP/TLS connection pool. Recommended for any precise measurement.
- `--repeat N` — runs the whole plan N times with fresh salts (new `order_id` per run). Outputs `{"runs": [...]}` for distribution analysis.

**What's pre-computed (NOT counted in `t_send`/`t_recv`)**:
- L1 + L2 auth (full credentials cache before t0)
- For each `--order`: build → sign → derive `order_id` from `OrderV2.eip712_signing_hash()`
- HTTP connection pool warmed (with `--warmup`)

**What's irreducible inside `t_send`→`t_recv`**:
- HMAC L2 signature (depends on per-request timestamp; ~100μs)
- JSON body serialization (~10μs)
- Network RTT (~150-300ms for taker, ~20ms for cancel per upstream measurements)

### Balances / Account

```bash
polymarket clob balance --asset-type collateral
polymarket clob balance --asset-type conditional --token <id>
polymarket clob update-balance --asset-type collateral

polymarket clob api-keys
polymarket clob create-api-key
polymarket clob delete-api-key
polymarket clob account-status
polymarket clob notifications
```

### Rewards

```bash
polymarket clob rewards --date 2026-04-30
polymarket clob earnings --date 2026-04-30
polymarket clob earnings-markets --date 2026-04-30
polymarket clob reward-percentages
polymarket clob current-rewards
polymarket clob market-reward <condition-id>
polymarket clob order-scoring <order-id>
polymarket clob orders-scoring "ID1,ID2"
```

### Contract Approvals

V2 needs separate approvals for both **v1** (legacy redemption) and **v2** (new trading) exchanges. `polymarket approve set` covers all of them in one go (sends 8 txs — pUSD + CTF for each of: CTF Exchange v1, Neg Risk Exchange v1, CTF Exchange v2, Neg Risk Exchange v2; plus optionally Neg Risk Adapter).

```bash
polymarket approve check                      # read-only, list per-contract status
polymarket approve check 0xADDR
polymarket approve set                        # send the txs (needs MATIC for gas)
```

### CTF (split / merge / redeem)

```bash
# Split $10 pUSD -> YES + NO tokens
polymarket ctf split  --condition 0xCOND --amount 10
polymarket ctf merge  --condition 0xCOND --amount 10
polymarket ctf redeem --condition 0xCOND
polymarket ctf redeem-neg-risk --condition 0xCOND --amounts "10,5"

# Read-only ID calculators
polymarket ctf condition-id  --oracle 0xORC --question 0xQ --outcomes 2
polymarket ctf collection-id --condition 0xCOND --index-set 1
polymarket ctf position-id   --collection 0xCOLL
```

`--amount` is in **pUSD** (e.g. `10` = $10). `--partition` defaults to binary `1,2`. Add `--collateral 0xADDR` to override the default pUSD address.

### Bridge (deposit from other chains)

```bash
polymarket bridge deposit 0xWALLET            # get EVM/Solana/BTC deposit addrs
polymarket bridge supported-assets
polymarket bridge status 0xDEPOSIT_ADDR
```

## Interactive Shell

```bash
polymarket shell
# polymarket> markets list --limit 3
# polymarket> clob book <token>
# polymarket> exit
```

Has command history. All commands work the same as the CLI, just without the `polymarket` prefix.

## Common Patterns

### Resolve a Polymarket URL to a CLI command

Users often paste `https://polymarket.com/...` URLs. Strip the path and route by segment:

| URL form | Slug source | Command |
|---|---|---|
| `polymarket.com/event/<slug>` | last segment | `polymarket events get <slug>` |
| `polymarket.com/event/<event-slug>/<market-slug>` | second-to-last segment | `polymarket markets get <market-slug>` |
| `polymarket.com/market/<slug>` | last segment | `polymarket markets get <slug>` |
| `polymarket.com/markets/<digits>` | last segment (numeric) | `polymarket markets get <id>` |

Drop trailing query strings (`?utm=...`, `#tvl`) before passing the slug. If unsure, try `events get` first — it returns the parent event and all child markets in one call.

### Resolve a slug to a CLOB token ID (for clob commands)

`clob` commands take CLOB token IDs (long hex strings or large decimals), but humans usually have slugs/IDs. Pull tokens from `markets get`:

```bash
polymarket -o json markets get <slug-or-id> | jq -r '.clobTokenIds | fromjson | .[0]'
# returns the YES (Up) token; .[1] is NO (Down)
```

For an event with many markets, fan out:

```bash
polymarket -o json events get <event-slug> | jq -r '.markets[] | "\(.question)\t\(.clobTokenIds | fromjson | .[0])"'
```

### JSON shapes worth knowing

**Field-naming convention — read this first.** The CLI emits **snake_case** (`realized_pnl`, `avg_price`, `total_bought`, `cur_price`, `percent_pnl`, `event_slug`, …) even when the upstream Polymarket data API uses **camelCase** (`realizedPnl`, `avgPrice`, ...). If you copy field names from the [data API docs](https://data-api.polymarket.com) or paste a `curl … | jq` query into the CLI flow, the lookups silently return `null`. Always inspect a single record first (`polymarket -o json data closed-positions <addr> --limit 1 | jq '.[0]'`) before writing aggregations.

Other shapes:

```bash
# .midpoint  — string decimal, NOT .mid
polymarket -o json clob midpoint <token>     # → {"midpoint": "0.4565"}

# .bids / .asks — arrays of {price, size} (price strings, ascending in .bids, ascending in .asks too — best bid is LAST in .bids, best ask is FIRST in .asks)
polymarket -o json clob book <token>         # → {bids:[{price,size},...], asks:[...], lastTradePrice, market, asset}

# Use these to grab top-of-book in one shot:
polymarket -o json clob book <token> | jq '{
  best_bid: (.bids | sort_by(.price | tonumber) | last),
  best_ask: (.asks | sort_by(.price | tonumber) | first),
  spread: ((.asks | min_by(.price | tonumber) | .price | tonumber) - (.bids | max_by(.price | tonumber) | .price | tonumber))
}'
```

Batch endpoints (`clob midpoints`, `clob batch-prices`) take comma-separated tokens but **only accept decimal token IDs**, not the hex form. If you have hex tokens, loop over `clob midpoint` one at a time instead — slower but works.

### Watch a market

```bash
TOKEN=$(polymarket -o json markets get <slug> | jq -r '.clobTokenIds | fromjson | .[0]')
watch -n 5 "polymarket clob midpoint $TOKEN"
```

### Show top-of-book across N buckets of a multi-outcome event

Pattern from event-with-N-buckets analysis (e.g. "Elon tweet count 140-159 / 160-179 / ..."). Builds a TSV of `bucket_label \t token_id` then loops `clob book` for top-of-book per bucket:

```bash
polymarket -o json events get <event-slug> | jq -r '.markets[] |
  "\(.question | capture("(?<b>\\d+(-\\d+|\\+))").b)\t\(.clobTokenIds | fromjson | .[0])"' \
  | sort -V > /tmp/buckets.tsv

while IFS=$'\t' read -r bucket token; do
  J=$(polymarket -o json clob book "$token")
  bid=$(echo "$J" | jq -r '(.bids // []) | sort_by(.price | tonumber) | last | "\(.price)@\(.size)"')
  ask=$(echo "$J" | jq -r '(.asks // []) | sort_by(.price | tonumber) | first | "\(.price)@\(.size)"')
  printf "%-12s  bid=%-15s  ask=%-15s\n" "$bucket" "$bid" "$ask"
done < /tmp/buckets.tsv
```

### Daily P&L from closed positions (CLI version of polymarket-data skill)

```bash
polymarket -o json data closed-positions 0xWALLET | jq '
  group_by(.endDate[:10])
  | map({date: .[0].endDate[:10], pnl: (map(.realizedPnl | tonumber) | add)})
  | sort_by(.date)
'
```

### Place a "scaled" set of bids

```bash
TOKEN=...; SIDE=buy
polymarket clob post-orders \
  --tokens  "$TOKEN,$TOKEN,$TOKEN" \
  --side    "$SIDE" \
  --prices  "0.40,0.42,0.44" \
  --sizes   "20,20,20"
```

## V2 Gotchas

- **Collateral is pUSD**, not USDC.e. Address `0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB`. Old USDC.e balance does not work for trading.
- **Pre-cutover open orders were wiped** — `clob orders` returns `[]` until you place new ones.
- **L2 API keys must be re-derived.** First call after migration: `polymarket clob create-api-key`. v1 keys return 401.
- **Both v1 and v2 contracts get approved** by `approve set`. v1 is for redeeming positions that predate the cutover; v2 is required for new trading.
- **Builder attribution** moved from HMAC headers (v1) to a per-order `builderCode` field (v2). The CLI handles this transparently.
- Endpoints: clob v2 is at `https://clob-v2.polymarket.com` (the SDK uses this by default); gamma/data/bridge URLs unchanged.

## Self-update

```bash
polymarket upgrade                            # checks fork's GitHub releases
```

The `upgrade` command currently points at `Alpha9Coding/polymarket-cli` (this fork). Will report "no release found" until releases are cut.

## Repo / Source

Source: [Alpha9Coding/polymarket-cli](https://github.com/Alpha9Coding/polymarket-cli). Build from source: `git clone … && cd polymarket-cli && cargo install --path .`. SDK: [Polymarket/rs-clob-client-v2](https://github.com/Polymarket/rs-clob-client-v2) (`polymarket_client_sdk_v2 = "0.5"`).
