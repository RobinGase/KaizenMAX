# Kaizen MAX

Kaizen MAX is a Windows-first operator cockpit for AI-assisted engineering.

- Runtime: Rust (`Kaizen`)
- Primary agent: `Kaizen`
- Sub-agent policy: user-controlled only
- Setup model: UI-first

## Operator Quick Start

1. Start the stack:
   - `scripts/start-max.bat`
   - or `scripts/start-max.ps1`
2. The native desktop Mission Control UI opens (`Tauri v2 + SolidJS`).
3. Open `Settings -> Providers & Auth`
4. Configure inference:
    - `zeroclaw` routes through the configured `inference_provider`
    - default local path: `codex-cli` with ChatGPT OAuth (`codex login`)
    - OpenAI / Anthropic / NVIDIA: `*_API_KEY`
    - Gemini: `GEMINI_API_KEY` / `GOOGLE_API_KEY`, or set `GOOGLE_OAUTH_CLIENT_ID` + `GOOGLE_CLOUD_PROJECT` and click `Connect OAuth`
    - Gemini CLI: install `gemini` and complete its local login once
    - Codex CLI: install `codex` and complete `codex login` once
5. Send a message in Kaizen chat

## Release Updates

- The desktop app treats `origin/main` as the release channel for repo installs.
- On launch and every 15 minutes, Mission Control checks whether the local checkout is behind `origin/main`.
- If `main` has new commits, the app shows an in-app update notification and an `Apply Update` action.
- Applying the update runs `scripts/update-kaizen-max.ps1`, which:
  - fetches `origin/main`
  - performs a fast-forward pull
  - rebuilds the backend and desktop app
  - relaunches Kaizen MAX
- Auto-apply is intentionally blocked when the checkout is dirty or not on `main`.

## Crystal Ball Bridge (Optional)

All setup is in `Settings -> Providers & Auth`:

1. Set Mattermost URL and Channel ID
2. Set `MATTERMOST_TOKEN` in the environment
3. Toggle `Enable Crystal Ball Bridge`
4. Run `Validate` and `Smoke`

## Security Notes

- Crystal Ball events are redacted before archive and bridge publication
- Optional `ADMIN_API_TOKEN` can enforce auth on sensitive settings/gates/secrets endpoints
- Remote bind mode requires explicit security acknowledgement and edge TLS/mTLS/auth

## Local Auth (Current)

Vault has been extracted to standalone repo: `D:\KaizenInnovations\Kai-Vault`

Current build runs without vault:
- OpenAI / Anthropic / NVIDIA use env vars (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `NVIDIA_API_KEY`)
- Gemini supports:
  - API key env vars (`GEMINI_API_KEY` or `GOOGLE_API_KEY`)
  - app-managed Google OAuth (`GOOGLE_OAUTH_CLIENT_ID`, `GOOGLE_CLOUD_PROJECT`, optional `GOOGLE_OAUTH_CLIENT_SECRET`)
  - Google ADC OAuth fallback (`gcloud auth application-default login` + `GOOGLE_CLOUD_PROJECT`)
- Codex CLI supports local login state, including ChatGPT OAuth (`codex login`, stored in `~/.codex/auth.json`)
- App-managed Gemini OAuth stores tokens locally at `data/oauth/gemini_tokens.json` by default
- `zeroclaw` is the provider/auth control plane and routes to the configured provider
- Default zeroclaw route is `codex-cli` (`gpt-5.4`) so the local app works without vault or API keys on a logged-in Codex CLI setup

## Minimal Requirements

- Windows 10/11
- Rust stable toolchain (`cargo`)
- Node.js + npm (for Mission Control UI dev/build)

The legacy React frontend is retired on the `RustTestBranch` rewrite path.

## Verify Build

- Core tests: run `cargo test` in `core/`
- UI checks: run `npm install && npm run check` in `ui-tauri-solid/`
- UI dev launch: run `npm run tauri:dev` in `ui-tauri-solid/`

For full architecture and rollout details, see `implementation_plan.md`.
