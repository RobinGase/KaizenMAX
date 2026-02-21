# Vault Standalone Sync Rule

This rule defines how KaizenMAX and the standalone Vault repository stay aligned.

## Intent

- Vault runs as its own standalone service/repository for release/runtime usage.
- KaizenMAX remains the active development workspace where vault changes can be authored.
- Any vault change in KaizenMAX must be mirrored to the standalone Vault repo immediately.

## Source Of Truth Model

- `Development source`: `KaizenMAX` (`DevMaster` -> `main` flow)
- `Runtime packaging source`: standalone Vault repo (Docker image and release tags)

Both must remain functionally equivalent for vault API + crypto behavior.

## Mandatory Trigger Files

If a change touches any of these paths, the sync rule is triggered:

- `core/src/vault.rs`
- `core/src/bin/kaizen-vaultd.rs`
- `docker/vaultd/Dockerfile`
- `docker-compose.vaultd.yml`
- `.env.example` (vault/vaultd keys)
- `docs/vaultd.md`
- `contexts/policies/secret_vault_contract.md`

## Required Actions (No Exceptions)

1. Implement and validate in KaizenMAX first.
2. Merge KaizenMAX change through `DevMaster` -> `main`.
3. Create matching sync commit/PR in standalone Vault repo.
4. Update Vault Docker image/tag in standalone repo release pipeline.
5. Record cross-reference in commit/PR notes:
   - `KaizenMAX commit <sha>`
   - `Vault repo commit/PR <ref>`

## Merge Gate

- A vault-triggering PR is **not complete** unless a matching standalone Vault sync reference exists.
- If standalone sync is intentionally deferred, PR must include explicit reason and owner.

## Agent Operating Rule

Any coding agent modifying trigger files must:

- state: `Vault sync required: YES`
- include a short "Standalone Vault Sync" checklist in final handoff

This keeps the process automatic by policy, without heavy scripting.
