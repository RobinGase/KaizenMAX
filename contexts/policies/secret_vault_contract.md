# Secret Vault Contract (Release Gate)

This document is the implementation contract for credential handling in Kaizen MAX.

## API Contract

- Write-only set/replace:
  - `PUT /api/secrets/{provider}`
- Metadata-only read:
  - `GET /api/secrets`
- Revoke:
  - `DELETE /api/secrets/{provider}`
- Integrity test:
  - `POST /api/secrets/{provider}/test`

The API never returns plaintext secret values.

## Storage Format

Vault records persist only encrypted material and metadata:

- `ciphertext` (base64 AES-256-GCM output)
- `nonce` (base64 12-byte nonce)
- `provider`
- `key_id`
- `created_at`
- `last_updated`
- `last4`
- `secret_type`

No raw secret may be written to `.env`, logs, events, archives, or API responses.

## Decryption Policy

- Decrypt only for immediate provider use.
- Keep plaintext in memory only for the shortest required scope.
- Wipe transient plaintext buffers in best-effort fashion after provider call setup.
- Do not persist decrypted values.

## Network + Access Policy

- Local mode (`ZEROCLAW_MODE=native`) must bind loopback host only.
- Remote mode requires explicit acknowledgement:
  - `ZEROCLAW_REMOTE_SECURITY_ACK=I_UNDERSTAND_REMOTE_REQUIRES_TLS_MTLS_AUTH`
- CORS must use explicit allowlist (`KAIZEN_CORS_ORIGINS`), no wildcard.
- If `ADMIN_API_TOKEN` is set, sensitive endpoints require:
  - `Authorization: Bearer <token>` or
  - `x-admin-token: <token>`

## Release Validation Checklist

- At-rest ciphertext inspection passes (no plaintext in vault file).
- API metadata responses do not leak plaintext.
- Event/archive/log redaction checks pass.
- Save flow network capture confirms plaintext only in request body to local gateway.
- Unauthorized requests fail when `ADMIN_API_TOKEN` is configured.
- Regression tests pass for chat, agents, gates, settings, and crystal-ball workflows.
