# Kaizen MAX

Kaizen MAX is a Windows-first operator cockpit for AI-assisted engineering.

- Runtime: Rust (`Kaizen`)
- Primary agent: `Kaizen`
- Sub-agent policy: user-controlled only
- Setup model: UI-first (no manual vault key generation required)

## Operator Quick Start

1. Start the stack:
   - `scripts/start-max.bat`
   - or `scripts/start-max.ps1`
2. The native Rust desktop UI opens as `Kaizen MAX`.
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
- Optional `ADMIN_API_TOKEN` can enforce auth on sensitive settings/gates/secrets endpoints
- Remote bind mode requires explicit security acknowledgement and edge TLS/mTLS/auth

See `contexts/policies/secret_vault_contract.md` for the security implementation contract.

## Docker Vault Daemon (Optional)

If you want vault isolation for multiple local applications, run the standalone containerized vault service:

- `docker compose -f docker-compose.vaultd.yml up -d --build`
- Preferred with local token file: `docker compose --env-file .env.vaultd.local -f docker-compose.vaultd.yml up -d --build`
- Service URL: `http://127.0.0.1:9210`
- Auth: per-app token via `x-vault-app` + `Authorization: Bearer <token>`

Full usage guide: `docs/vaultd.md`.

Vault standalone alignment policy: `contexts/policies/vault_repo_sync_rule.md`.

## Minimal Requirements

- Windows 10/11
- Rust stable toolchain (`cargo`)

The legacy React frontend is retired on the `RustTestBranch` rewrite path.

## Verify Build

- Core tests: run `cargo test` in `core/`
- UI build: run `cargo build` in `ui-dioxus/`

For full architecture and rollout details, see `implementation_plan.md`.
