import { Layers3, Save, Shield, SlidersHorizontal, Terminal } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api, type ConfigUpdate, type ModelTiersDto, type SanitizedConfig, type TierConfigDto } from './api';

type Notice = {
  text: string;
  tone: 'success' | 'error';
};

function toNumber(value: string, fallback: number): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : fallback;
}

export default function SettingsPanel() {
  const [config, setConfig] = useState<SanitizedConfig | null>(null);
  const [draft, setDraft] = useState<Partial<SanitizedConfig>>({});
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);

  useEffect(() => {
    let ignore = false;

    async function loadConfig() {
      try {
        setLoading(true);
        const nextConfig = await api.config();
        if (!ignore) {
          setConfig(nextConfig);
          setDraft(nextConfig);
        }
      } catch (error) {
        if (!ignore) {
          setNotice({ text: error instanceof Error ? error.message : String(error), tone: 'error' });
        }
      } finally {
        if (!ignore) {
          setLoading(false);
        }
      }
    }

    void loadConfig();

    return () => {
      ignore = true;
    };
  }, []);

  async function saveConfig() {
    if (!config) {
      return;
    }

    const payload: ConfigUpdate = {
      model_default: draft.model_default,
      model_provider: draft.model_provider,
      model_tiers: draft.model_tiers,
      auto_dispatch_enabled: draft.auto_dispatch_enabled,
      auto_dispatch_show_decision: draft.auto_dispatch_show_decision,
      auto_dispatch_two_stage_enabled: draft.auto_dispatch_two_stage_enabled,
      agent_max_turns: toNumber(String(draft.agent_max_turns ?? config.agent_max_turns), config.agent_max_turns),
      agent_verbose: draft.agent_verbose,
      agent_system_prompt: draft.agent_system_prompt,
      agent_reasoning_effort: draft.agent_reasoning_effort,
      agent_save_trajectories: draft.agent_save_trajectories,
      agent_trajectory_dir: draft.agent_trajectory_dir,
      terminal_env_type: draft.terminal_env_type,
      terminal_cwd: draft.terminal_cwd,
      terminal_timeout: toNumber(String(draft.terminal_timeout ?? config.terminal_timeout), config.terminal_timeout),
      terminal_docker_image: draft.terminal_docker_image,
      compression_enabled: draft.compression_enabled,
      compression_engine: draft.compression_engine,
      compression_model: draft.compression_model,
      compression_context_length: toNumber(
        String(draft.compression_context_length ?? config.compression_context_length),
        config.compression_context_length,
      ),
      display_streaming: draft.display_streaming,
      display_skin: draft.display_skin,
      embedding_enabled: draft.embedding_enabled,
      embedding_provider: draft.embedding_provider,
      embedding_model: draft.embedding_model,
      embedding_dimension: toNumber(
        String(draft.embedding_dimension ?? config.embedding_dimension),
        config.embedding_dimension,
      ),
      embedding_batch_size: toNumber(
        String(draft.embedding_batch_size ?? config.embedding_batch_size),
        config.embedding_batch_size,
      ),
      embedding_normalize: draft.embedding_normalize,
    };

    try {
      setSaving(true);
      setNotice(null);
      const nextConfig = await api.updateConfig(payload);
      setConfig(nextConfig);
      setDraft(nextConfig);
      setNotice({ text: 'Runtime config updated', tone: 'success' });
    } catch (error) {
      setNotice({ text: error instanceof Error ? error.message : String(error), tone: 'error' });
    } finally {
      setSaving(false);
    }
  }

  if (loading) {
    return <div className="panel-empty">Loading config</div>;
  }

  if (!config) {
    return (
      <div className="panel-empty panel-empty-error">
        {notice?.text ?? 'Config unavailable'}
      </div>
    );
  }

  return (
    <section className="settings-surface" aria-label="Control center">
      <header className="settings-header">
        <div>
          <p className="eyebrow">Control Center</p>
          <h2>Runtime Configuration</h2>
        </div>
        <button className="button button-primary" type="button" onClick={saveConfig} disabled={saving}>
          <Save size={16} aria-hidden="true" />
          <span>{saving ? 'Saving' : 'Save'}</span>
        </button>
      </header>

      {notice && (
        <div className={`notice notice-${notice.tone}`} role="status">
          {notice.text}
        </div>
      )}

      <div className="settings-grid">
        <fieldset className="settings-group">
          <legend>
            <SlidersHorizontal size={16} aria-hidden="true" />
            Model
          </legend>
          <label>
            <span>Provider</span>
            <select
              value={draft.model_provider ?? ''}
              onChange={(event) => setDraft((current) => ({ ...current, model_provider: event.target.value }))}
            >
              <option value="auto">auto</option>
              <option value="openai">openai</option>
              <option value="anthropic">anthropic</option>
              <option value="openrouter">openrouter</option>
              <option value="gemini">gemini</option>
              <option value="bedrock">bedrock</option>
            </select>
          </label>
          <label>
            <span>Default model</span>
            <input
              value={draft.model_default ?? ''}
              onChange={(event) => setDraft((current) => ({ ...current, model_default: event.target.value }))}
              placeholder="hakimi-agent"
            />
          </label>
          <label>
            <span>Reasoning effort</span>
            <select
              value={draft.agent_reasoning_effort ?? ''}
              onChange={(event) =>
                setDraft((current) => ({ ...current, agent_reasoning_effort: event.target.value }))
              }
            >
              <option value="">default</option>
              <option value="low">low</option>
              <option value="medium">medium</option>
              <option value="high">high</option>
            </select>
          </label>
        </fieldset>

        <fieldset className="settings-group settings-group-wide">
          <legend>
            <Layers3 size={16} aria-hidden="true" />
            Model Tiers & Auto-Dispatch
          </legend>
          <p style={{ fontSize: '0.875rem', color: 'var(--color-text-secondary)', marginBottom: '1rem' }}>
            Configure model tiers for intelligent workload distribution
          </p>
          
          <label className="switch-row">
            <span>Enable Auto-Dispatch</span>
            <input
              type="checkbox"
              checked={draft.auto_dispatch_enabled ?? false}
              onChange={(event) =>
                setDraft((current) => ({ ...current, auto_dispatch_enabled: event.target.checked }))
              }
            />
          </label>
          <label className="switch-row">
            <span>Show dispatch decisions</span>
            <input
              type="checkbox"
              checked={draft.auto_dispatch_show_decision ?? false}
              onChange={(event) =>
                setDraft((current) => ({ ...current, auto_dispatch_show_decision: event.target.checked }))
              }
            />
          </label>
          <label className="switch-row">
            <span>Two-stage execution</span>
            <input
              type="checkbox"
              checked={draft.auto_dispatch_two_stage_enabled ?? false}
              onChange={(event) =>
                setDraft((current) => ({ ...current, auto_dispatch_two_stage_enabled: event.target.checked }))
              }
            />
          </label>

          <div style={{ marginTop: '1.5rem', display: 'grid', gap: '1rem' }}>
            {/* Primary Tier */}
            <details open>
              <summary style={{ fontWeight: '500', cursor: 'pointer', marginBottom: '0.75rem' }}>
                Primary Tier (Required)
              </summary>
              <div style={{ display: 'grid', gap: '0.75rem', paddingLeft: '1rem' }}>
                <label>
                  <span>Provider</span>
                  <input
                    value={draft.model_tiers?.primary?.provider ?? ''}
                    onChange={(event) =>
                      setDraft((current) => {
                        const currentPrimary: TierConfigDto = current.model_tiers?.primary ?? { provider: '', model: '', base_url: '' };
                        return {
                          ...current,
                          model_tiers: {
                            primary: {
                              ...currentPrimary,
                              provider: event.target.value,
                            },
                            light: current.model_tiers?.light,
                            reasoning: current.model_tiers?.reasoning,
                          } as ModelTiersDto,
                        };
                      })
                    }
                    placeholder="openai"
                  />
                </label>
                <label>
                  <span>Model</span>
                  <input
                    value={draft.model_tiers?.primary?.model ?? ''}
                    onChange={(event) =>
                      setDraft((current) => {
                        const currentPrimary: TierConfigDto = current.model_tiers?.primary ?? { provider: '', model: '', base_url: '' };
                        return {
                          ...current,
                          model_tiers: {
                            primary: {
                              ...currentPrimary,
                              model: event.target.value,
                            },
                            light: current.model_tiers?.light,
                            reasoning: current.model_tiers?.reasoning,
                          } as ModelTiersDto,
                        };
                      })
                    }
                    placeholder="gpt-4"
                  />
                </label>
                <label>
                  <span>Base URL (optional)</span>
                  <input
                    value={draft.model_tiers?.primary?.base_url ?? ''}
                    onChange={(event) =>
                      setDraft((current) => {
                        const currentPrimary: TierConfigDto = current.model_tiers?.primary ?? { provider: '', model: '', base_url: '' };
                        return {
                          ...current,
                          model_tiers: {
                            primary: {
                              ...currentPrimary,
                              base_url: event.target.value,
                            },
                            light: current.model_tiers?.light,
                            reasoning: current.model_tiers?.reasoning,
                          } as ModelTiersDto,
                        };
                      })
                    }
                    placeholder="https://api.openai.com/v1"
                  />
                </label>
                <label>
                  <span>API Key (leave empty to use default)</span>
                  <input
                    type="password"
                    value={draft.model_tiers?.primary?.api_key ?? ''}
                    onChange={(event) =>
                      setDraft((current) => {
                        const currentPrimary: TierConfigDto = current.model_tiers?.primary ?? { provider: '', model: '', base_url: '' };
                        return {
                          ...current,
                          model_tiers: {
                            primary: {
                              ...currentPrimary,
                              api_key: event.target.value,
                            },
                            light: current.model_tiers?.light,
                            reasoning: current.model_tiers?.reasoning,
                          } as ModelTiersDto,
                        };
                      })
                    }
                    placeholder="sk-..."
                  />
                </label>
              </div>
            </details>

            {/* Light Tier */}
            <details>
              <summary style={{ fontWeight: '500', cursor: 'pointer', marginBottom: '0.75rem' }}>
                Light Tier (Optional)
              </summary>
              <div style={{ display: 'grid', gap: '0.75rem', paddingLeft: '1rem' }}>
                <label>
                  <span>Provider</span>
                  <input
                    value={draft.model_tiers?.light?.provider ?? ''}
                    onChange={(event) =>
                      setDraft((current) => {
                        const currentPrimary: TierConfigDto = current.model_tiers?.primary ?? { provider: '', model: '', base_url: '' };
                        return {
                          ...current,
                          model_tiers: {
                            primary: currentPrimary,
                            light: event.target.value
                              ? {
                                  ...current.model_tiers?.light,
                                  provider: event.target.value,
                                  model: current.model_tiers?.light?.model ?? '',
                                  base_url: current.model_tiers?.light?.base_url ?? '',
                                }
                              : undefined,
                            reasoning: current.model_tiers?.reasoning,
                          } as ModelTiersDto,
                        };
                      })
                    }
                    placeholder="openai"
                  />
                </label>
                <label>
                  <span>Model</span>
                  <input
                    value={draft.model_tiers?.light?.model ?? ''}
                    onChange={(event) =>
                      setDraft((current) => {
                        const currentPrimary: TierConfigDto = current.model_tiers?.primary ?? { provider: '', model: '', base_url: '' };
                        return {
                          ...current,
                          model_tiers: {
                            primary: currentPrimary,
                            light: event.target.value || current.model_tiers?.light?.provider
                              ? {
                                  ...current.model_tiers?.light,
                                  provider: current.model_tiers?.light?.provider ?? '',
                                  model: event.target.value,
                                  base_url: current.model_tiers?.light?.base_url ?? '',
                                }
                              : undefined,
                            reasoning: current.model_tiers?.reasoning,
                          } as ModelTiersDto,
                        };
                      })
                    }
                    placeholder="gpt-3.5-turbo"
                  />
                </label>
                <label>
                  <span>Base URL (optional)</span>
                  <input
                    value={draft.model_tiers?.light?.base_url ?? ''}
                    onChange={(event) =>
                      setDraft((current) => {
                        const currentPrimary: TierConfigDto = current.model_tiers?.primary ?? { provider: '', model: '', base_url: '' };
                        return {
                          ...current,
                          model_tiers: {
                            primary: currentPrimary,
                            light: current.model_tiers?.light
                              ? {
                                  ...current.model_tiers.light,
                                  base_url: event.target.value,
                                }
                              : undefined,
                            reasoning: current.model_tiers?.reasoning,
                          } as ModelTiersDto,
                        };
                      })
                    }
                    placeholder="https://api.openai.com/v1"
                  />
                </label>
                <label>
                  <span>API Key (leave empty to use default)</span>
                  <input
                    type="password"
                    value={draft.model_tiers?.light?.api_key ?? ''}
                    onChange={(event) =>
                      setDraft((current) => {
                        const currentPrimary: TierConfigDto = current.model_tiers?.primary ?? { provider: '', model: '', base_url: '' };
                        return {
                          ...current,
                          model_tiers: {
                            primary: currentPrimary,
                            light: current.model_tiers?.light
                              ? {
                                  ...current.model_tiers.light,
                                  api_key: event.target.value,
                                }
                              : undefined,
                            reasoning: current.model_tiers?.reasoning,
                          } as ModelTiersDto,
                        };
                      })
                    }
                    placeholder="sk-..."
                  />
                </label>
              </div>
            </details>

            {/* Reasoning Tier */}
            <details>
              <summary style={{ fontWeight: '500', cursor: 'pointer', marginBottom: '0.75rem' }}>
                Reasoning Tier (Optional)
              </summary>
              <div style={{ display: 'grid', gap: '0.75rem', paddingLeft: '1rem' }}>
                <label>
                  <span>Provider</span>
                  <input
                    value={draft.model_tiers?.reasoning?.provider ?? ''}
                    onChange={(event) =>
                      setDraft((current) => {
                        const currentPrimary: TierConfigDto = current.model_tiers?.primary ?? { provider: '', model: '', base_url: '' };
                        return {
                          ...current,
                          model_tiers: {
                            primary: currentPrimary,
                            light: current.model_tiers?.light,
                            reasoning: event.target.value
                              ? {
                                  ...current.model_tiers?.reasoning,
                                  provider: event.target.value,
                                  model: current.model_tiers?.reasoning?.model ?? '',
                                  base_url: current.model_tiers?.reasoning?.base_url ?? '',
                                }
                              : undefined,
                          } as ModelTiersDto,
                        };
                      })
                    }
                    placeholder="openai"
                  />
                </label>
                <label>
                  <span>Model</span>
                  <input
                    value={draft.model_tiers?.reasoning?.model ?? ''}
                    onChange={(event) =>
                      setDraft((current) => {
                        const currentPrimary: TierConfigDto = current.model_tiers?.primary ?? { provider: '', model: '', base_url: '' };
                        return {
                          ...current,
                          model_tiers: {
                            primary: currentPrimary,
                            light: current.model_tiers?.light,
                            reasoning: event.target.value || current.model_tiers?.reasoning?.provider
                              ? {
                                  ...current.model_tiers?.reasoning,
                                  provider: current.model_tiers?.reasoning?.provider ?? '',
                                  model: event.target.value,
                                  base_url: current.model_tiers?.reasoning?.base_url ?? '',
                                }
                              : undefined,
                          } as ModelTiersDto,
                        };
                      })
                    }
                    placeholder="o1-preview"
                  />
                </label>
                <label>
                  <span>Base URL (optional)</span>
                  <input
                    value={draft.model_tiers?.reasoning?.base_url ?? ''}
                    onChange={(event) =>
                      setDraft((current) => {
                        const currentPrimary: TierConfigDto = current.model_tiers?.primary ?? { provider: '', model: '', base_url: '' };
                        return {
                          ...current,
                          model_tiers: {
                            primary: currentPrimary,
                            light: current.model_tiers?.light,
                            reasoning: current.model_tiers?.reasoning
                              ? {
                                  ...current.model_tiers.reasoning,
                                  base_url: event.target.value,
                                }
                              : undefined,
                          } as ModelTiersDto,
                        };
                      })
                    }
                    placeholder="https://api.openai.com/v1"
                  />
                </label>
                <label>
                  <span>API Key (leave empty to use default)</span>
                  <input
                    type="password"
                    value={draft.model_tiers?.reasoning?.api_key ?? ''}
                    onChange={(event) =>
                      setDraft((current) => {
                        const currentPrimary: TierConfigDto = current.model_tiers?.primary ?? { provider: '', model: '', base_url: '' };
                        return {
                          ...current,
                          model_tiers: {
                            primary: currentPrimary,
                            light: current.model_tiers?.light,
                            reasoning: current.model_tiers?.reasoning
                              ? {
                                  ...current.model_tiers.reasoning,
                                  api_key: event.target.value,
                                }
                              : undefined,
                          } as ModelTiersDto,
                        };
                      })
                    }
                    placeholder="sk-..."
                  />
                </label>
              </div>
            </details>
          </div>
        </fieldset>

        <fieldset className="settings-group">
          <legend>
            <Shield size={16} aria-hidden="true" />
            Agent
          </legend>
          <label>
            <span>Max turns</span>
            <input
              type="number"
              min={1}
              value={draft.agent_max_turns ?? config.agent_max_turns}
              onChange={(event) =>
                setDraft((current) => ({ ...current, agent_max_turns: Number(event.target.value) }))
              }
            />
          </label>
          <label className="switch-row">
            <span>Verbose</span>
            <input
              type="checkbox"
              checked={draft.agent_verbose ?? false}
              onChange={(event) => setDraft((current) => ({ ...current, agent_verbose: event.target.checked }))}
            />
          </label>
          <label className="switch-row">
            <span>Save trajectories</span>
            <input
              type="checkbox"
              checked={draft.agent_save_trajectories ?? false}
              onChange={(event) =>
                setDraft((current) => ({ ...current, agent_save_trajectories: event.target.checked }))
              }
            />
          </label>
          <label>
            <span>Trajectory directory</span>
            <input
              value={draft.agent_trajectory_dir ?? ''}
              onChange={(event) =>
                setDraft((current) => ({ ...current, agent_trajectory_dir: event.target.value }))
              }
            />
          </label>
        </fieldset>

        <fieldset className="settings-group">
          <legend>
            <Terminal size={16} aria-hidden="true" />
            Terminal
          </legend>
          <label>
            <span>Environment</span>
            <select
              value={draft.terminal_env_type ?? ''}
              onChange={(event) => setDraft((current) => ({ ...current, terminal_env_type: event.target.value }))}
            >
              <option value="host">host</option>
              <option value="docker">docker</option>
            </select>
          </label>
          <label>
            <span>Working directory</span>
            <input
              value={draft.terminal_cwd ?? ''}
              onChange={(event) => setDraft((current) => ({ ...current, terminal_cwd: event.target.value }))}
            />
          </label>
          <label>
            <span>Timeout seconds</span>
            <input
              type="number"
              min={1}
              value={draft.terminal_timeout ?? config.terminal_timeout}
              onChange={(event) =>
                setDraft((current) => ({ ...current, terminal_timeout: Number(event.target.value) }))
              }
            />
          </label>
          <label>
            <span>Docker image</span>
            <input
              value={draft.terminal_docker_image ?? ''}
              onChange={(event) =>
                setDraft((current) => ({ ...current, terminal_docker_image: event.target.value }))
              }
            />
          </label>
        </fieldset>

        <fieldset className="settings-group settings-group-wide">
          <legend>System Prompt</legend>
          <textarea
            value={draft.agent_system_prompt ?? ''}
            onChange={(event) => setDraft((current) => ({ ...current, agent_system_prompt: event.target.value }))}
            spellCheck={false}
          />
        </fieldset>

        <fieldset className="settings-group">
          <legend>Compression</legend>
          <label className="switch-row">
            <span>Enabled</span>
            <input
              type="checkbox"
              checked={draft.compression_enabled ?? false}
              onChange={(event) =>
                setDraft((current) => ({ ...current, compression_enabled: event.target.checked }))
              }
            />
          </label>
          <label>
            <span>Engine</span>
            <select
              value={draft.compression_engine ?? ''}
              onChange={(event) => setDraft((current) => ({ ...current, compression_engine: event.target.value }))}
            >
              <option value="simple">simple</option>
              <option value="smart">smart</option>
            </select>
          </label>
          <label>
            <span>Model</span>
            <input
              value={draft.compression_model ?? ''}
              onChange={(event) => setDraft((current) => ({ ...current, compression_model: event.target.value }))}
            />
          </label>
          <label>
            <span>Context length</span>
            <input
              type="number"
              min={1}
              value={draft.compression_context_length ?? config.compression_context_length}
              onChange={(event) =>
                setDraft((current) => ({ ...current, compression_context_length: Number(event.target.value) }))
              }
            />
          </label>
        </fieldset>

        <fieldset className="settings-group">
          <legend>Display</legend>
          <label className="switch-row">
            <span>Streaming</span>
            <input
              type="checkbox"
              checked={draft.display_streaming ?? false}
              onChange={(event) => setDraft((current) => ({ ...current, display_streaming: event.target.checked }))}
            />
          </label>
          <label>
            <span>Skin</span>
            <input
              value={draft.display_skin ?? ''}
              onChange={(event) => setDraft((current) => ({ ...current, display_skin: event.target.value }))}
            />
          </label>
          <label>
            <span>MCP servers</span>
            <input value={config.mcp_server_count} readOnly />
          </label>
        </fieldset>
      </div>
    </section>
  );
}
