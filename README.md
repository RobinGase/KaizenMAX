# Kaizen MAX

Kaizen MAX is a Windows-first operator cockpit for AI-assisted engineering.

- Runtime: Rust (`ZeroClaw`)
- Primary agent: `Kaizen`
- Sub-agent policy: user-controlled only
- Setup model: UI-first (no manual vault key generation required)

## Operator Quick Start

1. Start the stack:
   - `scripts/start-max.bat`
   - or `scripts/start-max.ps1`
2. Open the UI: `http://localhost:3000`
3. Open `Settings -> Providers`
4. Configure inference:
   - choose provider/model
   - store provider API key in `Provider Credentials`
5. Send a message in Kaizen chat

## Crystal Ball Bridge (Optional)

All setup is in `Settings -> Providers`:

1. Set Mattermost URL and Channel ID
2. Store `Mattermost Bot` token in encrypted credentials
3. Toggle `Enable Crystal Ball Bridge`
4. Run `Validate` and `Smoke`

## Security Notes

- Secrets are encrypted at rest with AES-256-GCM
- Plaintext secrets are not returned by API endpoints
- Vault key is auto-bootstrapped if `ADMIN_VAULT_KEY` is not set
- Crystal Ball events are redacted before archive and bridge publication

## Minimal Requirements

- Windows 10/11
- Rust stable toolchain (`cargo`)
- Node.js 20+ and `npm`

## Verify Build

- Core tests: run `cargo test` in `core/`
- UI build: run `npm run build` in `ui/`

For full architecture and rollout details, see `implementation_plan.md`.
