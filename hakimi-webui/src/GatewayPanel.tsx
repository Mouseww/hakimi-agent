import { Activity, BadgeCheck, ChevronDown, ChevronRight, Edit3, Loader2, RefreshCcw, Save, ShieldCheck, X } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api, type GatewayConfigResponse, type GatewayPlatformConfig, type GatewayStatusResponse } from './api';
import { useI18n } from './i18n';

const SENSITIVE_FIELDS = new Set([
  'bot_token', 'token', 'client_secret', 'secret', 'api_key', 'password',
]);

const BOOL_FIELDS = new Set(['enabled', 'markdown_support', 'allow_all']);

interface PlatformEditorState {
  platform: string;
  fields: Record<string, unknown>;
}

export default function GatewayPanel() {
  const { t } = useI18n();
  const [status, setStatus] = useState<GatewayStatusResponse | null>(null);
  const [config, setConfig] = useState<GatewayConfigResponse | null>(null);
  const [platforms, setPlatforms] = useState<GatewayPlatformConfig[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);
  const [expandedPlatform, setExpandedPlatform] = useState<string | null>(null);
  const [editor, setEditor] = useState<PlatformEditorState | null>(null);
  const [savingPlatform, setSavingPlatform] = useState(false);

  const fetchData = async () => {
    setLoading(true);
    setError(null);
    try {
      const [statusRes, configRes, platformsRes] = await Promise.all([
        api.getGatewayStatus(),
        api.getGatewayConfig(),
        api.gatewayPlatforms(),
      ]);
      setStatus(statusRes);
      setConfig(configRes);
      setPlatforms(platformsRes);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load gateway data');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    const timer = window.setTimeout(() => { void fetchData(); }, 0);
    return () => window.clearTimeout(timer);
  }, []);

  const handleSaveConfig = async () => {
    if (!config) return;
    setSaving(true);
    setError(null);
    setSuccessMsg(null);
    try {
      await api.updateGatewayConfig(config);
      setSuccessMsg(t('gateway.configSaved'));
      setTimeout(() => setSuccessMsg(null), 3000);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save config');
    } finally {
      setSaving(false);
    }
  };

  const handleRestart = async () => {
    setRestarting(true);
    setError(null);
    setSuccessMsg(null);
    try {
      await api.restartGateway();
      setSuccessMsg(t('gateway.restartSent'));
      setTimeout(() => { setSuccessMsg(null); fetchData(); }, 2000);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to restart gateway');
    } finally {
      setRestarting(false);
    }
  };

  function openPlatformEditor(p: GatewayPlatformConfig) {
    setEditor({ platform: p.platform, fields: { ...p.config } });
  }

  function closePlatformEditor() {
    setEditor(null);
  }

  function updateEditorField(key: string, value: unknown) {
    if (!editor) return;
    setEditor({ ...editor, fields: { ...editor.fields, [key]: value } });
  }

  async function savePlatformConfig() {
    if (!editor) return;
    setSavingPlatform(true);
    setError(null);
    setSuccessMsg(null);
    try {
      const res = await api.updateGatewayPlatform(editor.platform, editor.fields);
      setSuccessMsg(res.message || t('gateway.configSaved'));
      setEditor(null);
      await fetchData();
      if (res.restart_required) {
        setSuccessMsg(t('gateway.savedNeedRestart'));
      }
      setTimeout(() => setSuccessMsg(null), 4000);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save platform config');
    } finally {
      setSavingPlatform(false);
    }
  }

  function toggleExpand(platform: string) {
    setExpandedPlatform(expandedPlatform === platform ? null : platform);
  }

  if (loading) {
    return (
      <div className="gateway-panel-loading">
        <Loader2 className="icon-spin" size={32} />
        <p>{t('gateway.loadingStatus')}</p>
      </div>
    );
  }

  const enabledPlatforms = platforms.filter((p) => p.enabled);
  const disabledPlatforms = platforms.filter((p) => !p.enabled);

  return (
    <div className="gateway-panel">
      <div className="gateway-header">
        <h2>{t('gateway.title')}</h2>
        <button className="btn-refresh" onClick={fetchData} disabled={loading}>
          <RefreshCcw size={16} />
          {t('gateway.refresh')}
        </button>
      </div>

      {error && (
        <div className="alert alert-error">
          <Activity size={16} />
          {error}
        </div>
      )}
      {successMsg && (
        <div className="alert alert-success">
          <BadgeCheck size={16} />
          {successMsg}
        </div>
      )}

      {/* Status Section */}
      <section className="gateway-status">
        <h3>{t('gateway.runningStatus')}</h3>
        <div className="status-grid">
          <div className="status-item">
            <span className="status-label">{t('gateway.status')}</span>
            <div className="status-value">
              {status?.running ? (
                <>
                  <Activity className="status-icon status-running" size={20} />
                  <span className="status-text status-running">{t('gateway.running')}</span>
                </>
              ) : (
                <>
                  <ShieldCheck className="status-icon status-stopped" size={20} />
                  <span className="status-text status-stopped">{t('gateway.stopped')}</span>
                </>
              )}
            </div>
          </div>
          {status?.running && (
            <div className="status-item">
              <span className="status-label">{t('gateway.totalMessages')}</span>
              <span className="status-value status-number">{status.total_messages_sent || 0}</span>
            </div>
          )}
        </div>
        <div style={{ display: 'flex', gap: '8px', marginTop: '8px' }}>
          {status?.running && (
            <button className="btn-restart" onClick={handleRestart} disabled={restarting}>
              {restarting ? <><Loader2 className="icon-spin" size={16} />{t('gateway.restarting')}</> : <><RefreshCcw size={16} />{t('gateway.restart')}</>}
            </button>
          )}
        </div>
      </section>

      {/* Platform Config Editor Modal */}
      {editor && (
        <section className="settings-surface" style={{ border: '1px solid var(--accent)', marginBottom: '12px' }}>
          <div className="settings-header">
            <div>
              <p className="eyebrow">{t(`instance.platform.${editor.platform}` as any)}</p>
              <h3>{t('gateway.platformConfig')}</h3>
            </div>
            <div style={{ display: 'flex', gap: '6px' }}>
              <button className="button button-primary" onClick={() => void savePlatformConfig()} disabled={savingPlatform}>
                <Save size={14} /> {savingPlatform ? t('gateway.saving') : t('gateway.saveConfig')}
              </button>
              <button className="icon-button" onClick={closePlatformEditor}><X size={16} /></button>
            </div>
          </div>
          <div className="config-form" style={{ padding: '0 12px 12px' }}>
            {Object.entries(editor.fields).map(([key, value]) => {
              const isSensitive = SENSITIVE_FIELDS.has(key);
              const isBool = BOOL_FIELDS.has(key) || typeof value === 'boolean';
              const label = key.replace(/_/g, ' ');

              if (isBool) {
                return (
                  <div className="form-group" key={key}>
                    <label>
                      <input
                        type="checkbox"
                        checked={!!value}
                        onChange={(e) => updateEditorField(key, e.target.checked)}
                      />
                      {label}
                    </label>
                  </div>
                );
              }

              if (typeof value === 'number') {
                return (
                  <div className="form-group" key={key}>
                    <label>{label}</label>
                    <input
                      type="number"
                      value={value}
                      onChange={(e) => updateEditorField(key, Number(e.target.value))}
                    />
                  </div>
                );
              }

              if (Array.isArray(value)) {
                return (
                  <div className="form-group" key={key}>
                    <label>{label}</label>
                    <textarea
                      rows={2}
                      value={(value as string[]).join('\n')}
                      onChange={(e) =>
                        updateEditorField(
                          key,
                          e.target.value.split('\n').map((s) => s.trim()).filter(Boolean),
                        )
                      }
                      placeholder={t('gateway.onePerLine')}
                    />
                  </div>
                );
              }

              return (
                <div className="form-group" key={key}>
                  <label>{label}</label>
                  <input
                    type={isSensitive ? 'password' : 'text'}
                    value={String(value ?? '')}
                    onChange={(e) => updateEditorField(key, e.target.value)}
                    placeholder={isSensitive ? t('gateway.enterSecret') : ''}
                    autoComplete={isSensitive ? 'new-password' : 'off'}
                  />
                  {isSensitive && (
                    <small className="form-hint">{t('gateway.secretHint')}</small>
                  )}
                </div>
              );
            })}
          </div>
        </section>
      )}

      {/* Enabled Platforms */}
      <section className="gateway-platforms">
        <h3>{t('gateway.enabledPlatforms')}</h3>
        {enabledPlatforms.length === 0 && (
          <div className="panel-empty">{t('gateway.noEnabled')}</div>
        )}
        {enabledPlatforms.map((p) => (
          <div className="platform-card" key={p.platform}>
            <div className="platform-card-header" onClick={() => toggleExpand(p.platform)}>
              {expandedPlatform === p.platform ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
              <span className="bindings-platform-tag">{p.platform}</span>
              <span className="platform-bot-id">{p.bot_id}</span>
              <span style={{ flex: 1 }} />
              <button
                className="icon-button"
                onClick={(e) => { e.stopPropagation(); openPlatformEditor(p); }}
                title={t('gateway.editPlatform')}
              >
                <Edit3 size={13} />
              </button>
            </div>
            {expandedPlatform === p.platform && (
              <div className="platform-card-details">
                {Object.entries(p.config)
                  .filter(([k]) => k !== 'enabled')
                  .map(([k, v]) => (
                    <div className="platform-detail-row" key={k}>
                      <span className="platform-detail-key">{k.replace(/_/g, ' ')}</span>
                      <span className="platform-detail-value">
                        {typeof v === 'boolean' ? (v ? 'Yes' : 'No') : String(v ?? '')}
                      </span>
                    </div>
                  ))}
              </div>
            )}
          </div>
        ))}
      </section>

      {/* Disabled Platforms */}
      <section className="gateway-platforms">
        <h3>{t('gateway.disabledPlatforms')}</h3>
        <div className="disabled-platforms-grid">
          {disabledPlatforms.map((p) => (
            <div className="platform-chip-disabled" key={p.platform}>
              <span>{t(`instance.platform.${p.platform}` as any)}</span>
              <button
                className="icon-button"
                onClick={() => openPlatformEditor(p)}
                title={t('gateway.editPlatform')}
              >
                <Edit3 size={12} />
              </button>
            </div>
          ))}
        </div>
      </section>

      {/* Global Config Section */}
      {config && (
        <section className="gateway-config">
          <h3>{t('gateway.config')}</h3>
          <div className="config-form">
            <div className="form-group">
              <label htmlFor="busy-mode">{t('gateway.busyMode')}</label>
              <select
                id="busy-mode"
                value={config.busy_input_mode}
                onChange={(e) => setConfig({ ...config, busy_input_mode: e.target.value })}
              >
                <option value="parallel">{t('gateway.parallel')}</option>
                <option value="queue">{t('gateway.queue')}</option>
                <option value="interrupt">{t('gateway.interrupt')}</option>
              </select>
              <small className="form-hint">{t('gateway.busyHint')}</small>
            </div>

            <div className="form-group">
              <label>
                <input
                  type="checkbox"
                  checked={config.allow_all}
                  onChange={(e) => setConfig({ ...config, allow_all: e.target.checked })}
                />
                {t('gateway.allowAll')}
              </label>
              <small className="form-hint">{t('gateway.allowAllHint')}</small>
            </div>

            {!config.allow_all && (
              <div className="form-group">
                <label htmlFor="allowed-users">{t('gateway.whitelist')}</label>
                <textarea
                  id="allowed-users"
                  rows={3}
                  placeholder={t('gateway.whitelistPlaceholder')}
                  value={config.allowed_users.join('\n')}
                  onChange={(e) =>
                    setConfig({
                      ...config,
                      allowed_users: e.target.value.split('\n').map((s) => s.trim()).filter(Boolean),
                    })
                  }
                />
                <small className="form-hint">{t('gateway.whitelistHint')}</small>
              </div>
            )}

            <div className="form-group">
              <label>
                <input
                  type="checkbox"
                  checked={config.filter_silence_narration}
                  onChange={(e) => setConfig({ ...config, filter_silence_narration: e.target.checked })}
                />
                {t('gateway.filterNarration')}
              </label>
              <small className="form-hint">{t('gateway.filterNarrationHint')}</small>
            </div>

            <button className="btn-save" onClick={handleSaveConfig} disabled={saving}>
              {saving ? <><Loader2 className="icon-spin" size={16} />{t('gateway.saving')}</> : <><BadgeCheck size={16} />{t('gateway.saveConfig')}</>}
            </button>
          </div>
        </section>
      )}
    </div>
  );
}
