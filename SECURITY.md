# Security policy

jevmarket signs on-chain approvals and places real Polymarket orders with a private key from its config file or environment. We treat key handling, order placement and the exposure caps as security-sensitive.

## Using it safely

- Use a **dedicated wallet** holding only funds you are willing to lose. The caps in the config are the only brake.
- The config file is written with mode `0600`. Keep it that way, never commit or share it, and prefer the `POLYMARKET_PRIVATE_KEY` and `OPENROUTER_API_KEY` environment variables on shared machines.
- The SQLite database contains your market activity and order history. Treat it as private.
- The OpenRouter key is sent only to `openrouter_base_url`. The private key never leaves the machine: it signs EIP-712 orders locally and, for `setup`, approval transactions sent through `polygon_rpc_url`.

## Supported versions

Security fixes land on the latest release only.

| Release | Status |
|---|---|
| Latest | Supported |
| Older | Not supported |

## Reporting a vulnerability

**Do not open a public GitHub issue for an unpatched vulnerability.**

1. **GitHub Security Advisories** (preferred): [submit a private advisory](../../security/advisories/new)
2. **Email**: `security@coderscantina.com`, subject `jevmarket security report`

Please include the jevmarket version (`jevmarket --version`), your OS, the affected component (config handling, order placement, caps, approvals, OpenRouter client), reproduction steps and the impact.

## What to expect

- Acknowledgement within **72 hours**.
- A status update within **14 days** if a fix is not ready yet.
- Coordinated disclosure after a fix is released, with credit if you want it.

## What counts

Examples: a way to exceed the configured caps, orders placed during `--dry-run`, key material written to logs or the database, the key or config sent anywhere other than the documented endpoints, config file permissions widened, or a crafted API response that makes the bot trade outside its gates.

Usually not a security issue: losing money on a trade the caps allowed, OpenRouter or Polymarket outages, and misconfiguration of documented settings.

There is no bug bounty.
