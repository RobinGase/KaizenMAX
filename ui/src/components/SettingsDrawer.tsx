import { useEffect, useState } from "react";
import { fetchVaultStatus } from "../api";
import type {
  ArchiveIntegrityReport,
  CrystalBallHealth,
  CrystalBallSmokeResponse,
  CrystalBallValidateResponse,
  KaizenSettings,
  KaizenSettingsPatch,
  VaultStatus,
} from "../types";
import CredentialsPanel from "./CredentialsPanel";

type SettingsTab = "general" | "providers" | "crystal_ball";

interface SettingsDrawerProps {
  isOpen: boolean;
  settings: KaizenSettings | null;
  crystalBallHealth: CrystalBallHealth | null;
  crystalBallAudit: ArchiveIntegrityReport | null;
  crystalBallValidation: CrystalBallValidateResponse | null;
  crystalBallSmoke: CrystalBallSmokeResponse | null;
  onClose: () => void;
  onUpdate: (patch: KaizenSettingsPatch) => Promise<void>;
  onRefreshCrystalBallAudit: () => Promise<void>;
  onValidateCrystalBall: () => Promise<void>;
  onRunCrystalBallSmoke: () => Promise<void>;
}

function SettingsDrawer({
  isOpen,
  settings,
  crystalBallHealth,
  crystalBallAudit,
  crystalBallValidation,
  crystalBallSmoke,
  onClose,
  onUpdate,
  onRefreshCrystalBallAudit,
  onValidateCrystalBall,
  onRunCrystalBallSmoke,
}: SettingsDrawerProps) {
  const [activeTab, setActiveTab] = useState<SettingsTab>("general");
  const [saving, setSaving] = useState(false);
  const [refreshingAudit, setRefreshingAudit] = useState(false);
  const [validatingBridge, setValidatingBridge] = useState(false);
  const [runningSmoke, setRunningSmoke] = useState(false);
  const [vaultStatus, setVaultStatus] = useState<VaultStatus | null>(null);
  const [loadingVaultStatus, setLoadingVaultStatus] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isOpen) return;
    setActiveTab("general");
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen || activeTab !== "providers") {
      return;
    }

    setLoadingVaultStatus(true);
    fetchVaultStatus()
      .then((status) => {
        setVaultStatus(status);
      })
      .catch((loadError) => {
        setError(
          loadError instanceof Error
            ? loadError.message
            : "Failed to load vault status"
        );
      })
      .finally(() => {
        setLoadingVaultStatus(false);
      });
  }, [isOpen, activeTab]);

  if (!isOpen || !settings) {
    return null;
  }

  const save = async (patch: KaizenSettingsPatch) => {
    setSaving(true);
    setError(null);
    try {
      await onUpdate(patch);
    } catch (updateError) {
      setError(
        updateError instanceof Error
          ? updateError.message
          : "Failed to update settings"
      );
    } finally {
      setSaving(false);
    }
  };

  const refreshAudit = async () => {
    setRefreshingAudit(true);
    setError(null);
    try {
      await onRefreshCrystalBallAudit();
    } catch (refreshError) {
      setError(
        refreshError instanceof Error
          ? refreshError.message
          : "Failed to refresh Crystal Ball audit"
      );
    } finally {
      setRefreshingAudit(false);
    }
  };

  const validateBridge = async () => {
    setValidatingBridge(true);
    setError(null);
    try {
      await onValidateCrystalBall();
    } catch (validateError) {
      setError(
        validateError instanceof Error
          ? validateError.message
          : "Failed to validate Crystal Ball bridge"
      );
    } finally {
      setValidatingBridge(false);
    }
  };

  const runSmoke = async () => {
    setRunningSmoke(true);
    setError(null);
    try {
      await onRunCrystalBallSmoke();
    } catch (smokeError) {
      setError(
        smokeError instanceof Error
          ? smokeError.message
          : "Failed to run Crystal Ball smoke test"
      );
    } finally {
      setRunningSmoke(false);
    }
  };

  return (
    <div className="settings-overlay" onClick={onClose}>
      <aside className="settings-drawer" onClick={(event) => event.stopPropagation()}>
        <header className="settings-header">
          <h3>Settings</h3>
          <button type="button" onClick={onClose}>
            Close
          </button>
        </header>

        <div className="settings-tabs">
          <button
            type="button"
            className={activeTab === "general" ? "settings-tab active" : "settings-tab"}
            onClick={() => setActiveTab("general")}
          >
            General
          </button>
          <button
            type="button"
            className={activeTab === "providers" ? "settings-tab active" : "settings-tab"}
            onClick={() => setActiveTab("providers")}
          >
            Providers
          </button>
          <button
            type="button"
            className={activeTab === "crystal_ball" ? "settings-tab active" : "settings-tab"}
            onClick={() => setActiveTab("crystal_ball")}
          >
            Crystal Ball
          </button>
        </div>

        <div className="settings-content">
          {activeTab === "general" && (
            <>
              <label>
                Runtime Engine
                <select
                  value={settings.runtime_engine}
                  disabled={saving}
                  onChange={(event) =>
                    void save({
                      runtime_engine: event.target.value as "zeroclaw" | "openclaw_compat",
                    })
                  }
                >
                  <option value="zeroclaw">ZeroClaw (default)</option>
                  <option value="openclaw_compat">OpenClaw Compatibility</option>
                </select>
              </label>

              <label>
                Max Sub-Agents
                <input
                  type="number"
                  min={0}
                  max={20}
                  value={settings.max_subagents}
                  disabled={saving}
                  onChange={(event) =>
                    void save({
                      max_subagents: Math.min(
                        20,
                        Math.max(0, Number(event.target.value) || 0)
                      ),
                    })
                  }
                />
              </label>

              <label>
                New Agent Chat Default
                <select
                  value={settings.new_agent_chat_default_state}
                  disabled={saving}
                  onChange={(event) =>
                    void save({
                      new_agent_chat_default_state: event.target.value as "open" | "closed",
                    })
                  }
                >
                  <option value="closed">Closed</option>
                  <option value="open">Open</option>
                </select>
              </label>

              <label className="settings-toggle">
                <input
                  type="checkbox"
                  checked={settings.auto_spawn_subagents}
                  disabled={saving}
                  onChange={(event) =>
                    void save({ auto_spawn_subagents: event.target.checked })
                  }
                />
                Allow Auto Spawn
              </label>

              <label className="settings-toggle">
                <input
                  type="checkbox"
                  checked={settings.allow_direct_user_to_subagent_chat}
                  disabled={saving}
                  onChange={(event) =>
                    void save({
                      allow_direct_user_to_subagent_chat: event.target.checked,
                    })
                  }
                />
                Allow Direct User to Sub-Agent Chat
              </label>

              <label className="settings-toggle">
                <input
                  type="checkbox"
                  checked={settings.hard_gates_enabled}
                  disabled={saving}
                  onChange={(event) =>
                    void save({ hard_gates_enabled: event.target.checked })
                  }
                />
                Hard Gates Enabled
              </label>

              <label className="settings-toggle">
                <input
                  type="checkbox"
                  checked={settings.require_human_smoke_test_before_deploy}
                  disabled={saving}
                  onChange={(event) =>
                    void save({
                      require_human_smoke_test_before_deploy: event.target.checked,
                    })
                  }
                />
                Require Human Smoke Test Before Deploy
              </label>

              <label className="settings-toggle">
                <input
                  type="checkbox"
                  checked={settings.provider_inference_only}
                  disabled={saving}
                  onChange={(event) =>
                    void save({ provider_inference_only: event.target.checked })
                  }
                />
                Provider Inference Only
              </label>
            </>
          )}

          {activeTab === "providers" && (
            <>
              <div className="inference-settings">
                <h4>Inference Provider</h4>

                <div className="inference-field">
                  <label>Provider</label>
                  <select
                    value={settings.inference_provider}
                    disabled={saving}
                    onChange={(event) =>
                      void save({ inference_provider: event.target.value })
                    }
                  >
                    <option value="anthropic">Anthropic (Claude)</option>
                    <option value="openai">OpenAI (GPT)</option>
                  </select>
                </div>

                <div className="inference-field">
                  <label>Model</label>
                  <input
                    type="text"
                    value={settings.inference_model}
                    disabled={saving}
                    onChange={(event) =>
                      void save({ inference_model: event.target.value })
                    }
                    placeholder={
                      settings.inference_provider === "anthropic"
                        ? "claude-sonnet-4-20250514"
                        : "gpt-4o"
                    }
                  />
                </div>

                <div className="inference-field">
                  <label>
                    Max Tokens
                    <span className="range-value">{settings.inference_max_tokens}</span>
                  </label>
                  <input
                    type="number"
                    min={256}
                    max={32768}
                    step={256}
                    value={settings.inference_max_tokens}
                    disabled={saving}
                    onChange={(event) =>
                      void save({
                        inference_max_tokens: Math.min(
                          32768,
                          Math.max(256, Number(event.target.value) || 4096)
                        ),
                      })
                    }
                  />
                </div>

                <div className="inference-field">
                  <label>
                    Temperature
                    <span className="range-value">
                      {settings.inference_temperature.toFixed(2)}
                    </span>
                  </label>
                  <input
                    type="range"
                    min={0}
                    max={1}
                    step={0.05}
                    value={settings.inference_temperature}
                    disabled={saving}
                    onChange={(event) =>
                      void save({
                        inference_temperature: parseFloat(event.target.value),
                      })
                    }
                  />
                </div>

                <label className="settings-toggle">
                  <input
                    type="checkbox"
                    checked={settings.credentials_ui_enabled}
                    disabled={saving}
                    onChange={(event) =>
                      void save({ credentials_ui_enabled: event.target.checked })
                    }
                  />
                  Show Credentials Panel
                </label>
              </div>

              {loadingVaultStatus && <p className="settings-note">Loading vault status...</p>}

              <CredentialsPanel
                enabled={settings.credentials_ui_enabled}
                vaultStatus={vaultStatus}
              />
            </>
          )}

          {activeTab === "crystal_ball" && (
            <>
              <label className="settings-toggle">
                <input
                  type="checkbox"
                  checked={settings.crystal_ball_enabled}
                  disabled={saving}
                  onChange={(event) =>
                    void save({ crystal_ball_enabled: event.target.checked })
                  }
                />
                Crystal Ball Enabled
              </label>

              <label className="settings-toggle">
                <input
                  type="checkbox"
                  checked={settings.crystal_ball_default_open}
                  disabled={saving}
                  onChange={(event) =>
                    void save({ crystal_ball_default_open: event.target.checked })
                  }
                />
                Crystal Ball Open on Startup
              </label>

              <section className="audit-card">
                <div className="audit-card-header">
                  <h4>Crystal Ball Audit</h4>
                  <div className="audit-actions">
                    <button
                      type="button"
                      onClick={() => void refreshAudit()}
                      disabled={refreshingAudit}
                    >
                      {refreshingAudit ? "Refreshing..." : "Refresh"}
                    </button>
                    <button
                      type="button"
                      onClick={() => void validateBridge()}
                      disabled={validatingBridge}
                    >
                      {validatingBridge ? "Validating..." : "Validate"}
                    </button>
                    <button
                      type="button"
                      onClick={() => void runSmoke()}
                      disabled={runningSmoke}
                    >
                      {runningSmoke ? "Running..." : "Smoke"}
                    </button>
                  </div>
                </div>

                <div className="audit-grid">
                  <span>Mode</span>
                  <strong>{crystalBallHealth?.mode ?? "unknown"}</strong>

                  <span>Mattermost</span>
                  <strong>
                    {crystalBallHealth?.mattermost_configured
                      ? crystalBallHealth?.mattermost_connected
                        ? "connected"
                        : "configured (offline)"
                      : "local-only"}
                  </strong>

                  <span>Archive Integrity</span>
                  <strong>{crystalBallHealth?.archive_integrity_valid ? "valid" : "invalid"}</strong>

                  <span>HMAC</span>
                  <strong>
                    {crystalBallHealth?.archive_hmac_configured ? "configured" : "not configured"}
                  </strong>

                  <span>Signed Records</span>
                  <strong>{crystalBallHealth?.archive_signed_records ?? 0}</strong>

                  <span>Legacy Records</span>
                  <strong>{crystalBallHealth?.archive_legacy_unsigned_records ?? 0}</strong>

                  <span>MAC Verified</span>
                  <strong>{crystalBallHealth?.archive_mac_verified_records ?? 0}</strong>

                  <span>MAC Missing</span>
                  <strong>{crystalBallHealth?.archive_mac_missing_records ?? 0}</strong>
                </div>

                {crystalBallValidation && (
                  <div className="audit-summary">
                    <span>Bridge Validation</span>
                    <strong>
                      {crystalBallValidation.error
                        ? `failed (${crystalBallValidation.error})`
                        : "ok"}
                    </strong>
                  </div>
                )}

                {crystalBallSmoke && (
                  <div className="audit-summary">
                    <span>Smoke Test</span>
                    <strong>
                      {crystalBallSmoke.success
                        ? `ok (${crystalBallSmoke.smoke?.marker ?? "no marker"})`
                        : `failed (${crystalBallSmoke.error ?? "unknown"})`}
                    </strong>
                  </div>
                )}

                {crystalBallAudit && !crystalBallAudit.valid && (
                  <p className="settings-error">
                    Audit issue at line {crystalBallAudit.first_invalid_line ?? "?"}: {" "}
                    {crystalBallAudit.reason ?? "unknown"}
                  </p>
                )}
              </section>
            </>
          )}

          {error && <p className="settings-error">{error}</p>}
        </div>
      </aside>
    </div>
  );
}

export default SettingsDrawer;
