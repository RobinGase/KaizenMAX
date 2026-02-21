# Kaizen Vault Daemon (Docker)

`kaizen-vaultd` is a standalone local vault service for multi-application secret storage.

## Why this mode

- Keeps vault runtime isolated in a container
- Lets only explicit apps use vault (`x-vault-app` + token)
- Does not force unrelated apps to depend on vault availability

## Start with Docker

From repo root:

```bash
docker compose -f docker-compose.vaultd.yml up -d --build
```

Health check:

```bash
curl http://127.0.0.1:9210/health
```

## Auth model

Each request requires:

- `x-vault-app: <app_id>`
- either `Authorization: Bearer <token>` or `x-vault-token: <token>`

Token sources:

- Preferred: `KAIZEN_VAULTD_APP_TOKENS_FILE` and `KAIZEN_VAULTD_ADMIN_TOKEN_FILE`
- Fallback: `KAIZEN_VAULTD_APP_TOKENS` and `KAIZEN_VAULTD_ADMIN_TOKEN`
- `KAIZEN_VAULTD_ADMIN_CROSS_APP_BYPASS` defaults to `false` (recommended)

`KAIZEN_VAULTD_APP_TOKENS` format:

```text
kaizenmax=<token>,crm=<token>,nexus=<token>
```

Secrets are app-scoped internally (`app/<app_id>/<provider>`), so one app cannot read another app's secrets.
Admin cross-app access is disabled unless `KAIZEN_VAULTD_ADMIN_CROSS_APP_BYPASS=true`.

## API

- `GET /health`
- `GET /v1/secrets`
- `PUT /v1/secrets/{provider}` with JSON body:
  - `value` (string)
  - `secret_type` (optional, default `api_key`)
- `POST /v1/secrets/{provider}/test`
- `GET /v1/secrets/{provider}/use`
- `DELETE /v1/secrets/{provider}`

Security behavior:

- Failed auth attempts are rate-limited per app id.
- After too many failed attempts, API returns `429` for a short cooldown window.

Example store:

```bash
curl -X PUT "http://127.0.0.1:9210/v1/secrets/openai" \
  -H "x-vault-app: kaizenmax" \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"value":"sk-...","secret_type":"api_key"}'
```

Example retrieve for internal app use:

```bash
curl "http://127.0.0.1:9210/v1/secrets/openai/use" \
  -H "x-vault-app: kaizenmax" \
  -H "Authorization: Bearer <token>"
```

## Operational policy

- Bind to loopback only (`127.0.0.1:9210`) unless you deliberately secure remote access.
- Use long random tokens per app.
- Rotate app tokens periodically.
- Keep container volume backups for `/data/vault.json` and `/data/vault.key`.
- For clients, set `NO_PROXY=127.0.0.1,localhost,::1` to avoid accidental proxy routing.
- `/v1` responses include no-store cache headers; keep client-side logging disabled for `/use` payloads.
