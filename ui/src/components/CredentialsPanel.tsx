import { useEffect, useState } from "react";
import type { SecretMetadata, SecretTestResult } from "../types";
import {
  fetchSecrets,
  storeSecret,
  revokeSecret,
  testSecret,
} from "../api";

interface CredentialsPanelProps {
  enabled: boolean;
}

const KNOWN_PROVIDERS = [
  { id: "anthropic", label: "Anthropic" },
  { id: "openai", label: "OpenAI" },
  { id: "google", label: "Google AI" },
];

function CredentialsPanel({ enabled }: CredentialsPanelProps) {
  const [secrets, setSecrets] = useState<SecretMetadata[]>([]);
  const [inputs, setInputs] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState<Record<string, boolean>>({});
  const [testing, setTesting] = useState<Record<string, boolean>>({});
  const [testResults, setTestResults] = useState<
    Record<string, SecretTestResult>
  >({});
  const [error, setError] = useState<string | null>(null);

  const loadSecrets = async () => {
    try {
      const list = await fetchSecrets();
      setSecrets(list);
    } catch (e) {
      setError(
        e instanceof Error ? e.message : "Failed to load credentials"
      );
    }
  };

  useEffect(() => {
    if (enabled) {
      void loadSecrets();
    }
  }, [enabled]);

  if (!enabled) {
    return null;
  }

  const getSecretForProvider = (providerId: string) =>
    secrets.find((s) => s.provider === providerId);

  const handleStore = async (providerId: string) => {
    const value = inputs[providerId]?.trim();
    if (!value) return;

    setSaving((prev) => ({ ...prev, [providerId]: true }));
    setError(null);
    try {
      await storeSecret(providerId, value);
      setInputs((prev) => ({ ...prev, [providerId]: "" }));
      await loadSecrets();
    } catch (e) {
      setError(
        e instanceof Error ? e.message : "Failed to store credential"
      );
    } finally {
      setSaving((prev) => ({ ...prev, [providerId]: false }));
    }
  };

  const handleRevoke = async (providerId: string) => {
    setError(null);
    try {
      await revokeSecret(providerId);
      setTestResults((prev) => {
        const next = { ...prev };
        delete next[providerId];
        return next;
      });
      await loadSecrets();
    } catch (e) {
      setError(
        e instanceof Error ? e.message : "Failed to revoke credential"
      );
    }
  };

  const handleTest = async (providerId: string) => {
    setTesting((prev) => ({ ...prev, [providerId]: true }));
    setError(null);
    try {
      const result = await testSecret(providerId);
      setTestResults((prev) => ({ ...prev, [providerId]: result }));
    } catch (e) {
      setError(
        e instanceof Error ? e.message : "Failed to test credential"
      );
    } finally {
      setTesting((prev) => ({ ...prev, [providerId]: false }));
    }
  };

  return (
    <section className="credentials-panel">
      <h4>Provider Credentials</h4>
      <p className="credentials-info">
        Keys are encrypted at rest. Only masked metadata is shown.
      </p>

      {KNOWN_PROVIDERS.map((provider) => {
        const meta = getSecretForProvider(provider.id);
        const isSaving = saving[provider.id] ?? false;
        const isTesting = testing[provider.id] ?? false;
        const result = testResults[provider.id];

        return (
          <div className="credential-row" key={provider.id}>
            <div className="credential-header">
              <strong>{provider.label}</strong>
              {meta ? (
                <span className="credential-status credential-configured">
                  Configured (****{meta.last4})
                </span>
              ) : (
                <span className="credential-status credential-missing">
                  Not configured
                </span>
              )}
            </div>

            <div className="credential-actions">
              <input
                type="password"
                value={inputs[provider.id] ?? ""}
                onChange={(e) =>
                  setInputs((prev) => ({
                    ...prev,
                    [provider.id]: e.target.value,
                  }))
                }
                placeholder={
                  meta ? "Replace key..." : "Enter API key..."
                }
                autoComplete="off"
              />
              <button
                onClick={() => void handleStore(provider.id)}
                disabled={
                  isSaving || !(inputs[provider.id]?.trim())
                }
              >
                {isSaving ? "Saving..." : meta ? "Update" : "Save"}
              </button>
              {meta && (
                <>
                  <button
                    onClick={() => void handleTest(provider.id)}
                    disabled={isTesting}
                  >
                    {isTesting ? "Testing..." : "Test"}
                  </button>
                  <button
                    onClick={() => void handleRevoke(provider.id)}
                    className="credential-revoke"
                  >
                    Revoke
                  </button>
                </>
              )}
            </div>

            {result && (
              <div
                className={`credential-test-result ${
                  result.test_passed ? "test-pass" : "test-fail"
                }`}
              >
                {result.test_passed
                  ? "Test passed"
                  : `Test failed: ${result.error ?? "unknown"}`}
              </div>
            )}

            {meta && (
              <div className="credential-meta">
                Updated: {new Date(meta.last_updated).toLocaleString()}
              </div>
            )}
          </div>
        );
      })}

      {error && <p className="settings-error">{error}</p>}
    </section>
  );
}

export default CredentialsPanel;
