import { Activity, BadgeCheck, Loader2, RefreshCcw, ShieldCheck } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api, type GatewayConfigResponse, type GatewayStatusResponse } from './api';
import { useI18n } from './i18n';

export default function GatewayPanel() {
  const { t } = useI18n();
  const [status, setStatus] = useState<GatewayStatusResponse | null>(null);
  const [config, setConfig] = useState<GatewayConfigResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);

  const fetchData = async () => {
    setLoading(true);
    setError(null);
    try {
      const [statusRes, configRes] = await Promise.all([
        api.getGatewayStatus(),
        api.getGatewayConfig(),
      ]);
      setStatus(statusRes);
      setConfig(configRes);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load gateway data');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void fetchData();
    }, 0);
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
      setTimeout(() => {
        setSuccessMsg(null);
        fetchData();
      }, 2000);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to restart gateway');
    } finally {
      setRestarting(false);
    }
  };

  if (loading) {
    return (
      <div className="gateway-panel-loading">
        <Loader2 className="icon-spin" size={32} />
        <p>{t('gateway.loadingStatus')}</p>
      </div>
    );
  }

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
            <>
              <div className="status-item">
                <span className="status-label">{t('gateway.connectedPlatforms')}</span>
                <span className="status-value">
                  {status.platforms.length > 0 ? (
                    <div className="platform-list">
                      {status.platforms.map((p, i) => (
                        <div key={i} className="platform-item">
                          <BadgeCheck size={14} className="platform-icon" />
                          <span>{p.platform || p.name}</span>
                          {p.bot_id && <code className="bot-id">{p.bot_id}</code>}
                        </div>
                      ))}
                    </div>
                  ) : (
                    <span className="text-muted">{t('gateway.none')}</span>
                  )}
                </span>
              </div>

              <div className="status-item">
                <span className="status-label">{t('gateway.totalMessages')}</span>
                <span className="status-value status-number">
                  {status.total_messages_sent || 0}
                </span>
              </div>
            </>
          )}
        </div>

        {status?.running && (
          <button
            className="btn-restart"
            onClick={handleRestart}
            disabled={restarting}
          >
            {restarting ? (
              <>
                <Loader2 className="icon-spin" size={16} />
                {t('gateway.restarting')}
              </>
            ) : (
              <>
                <RefreshCcw size={16} />
                {t('gateway.restart')}
              </>
            )}
          </button>
        )}
      </section>

      {/* Config Section */}
      {config && (
        <section className="gateway-config">
          <h3>{t('gateway.config')}</h3>
          <div className="config-form">
            <div className="form-group">
              <label htmlFor="busy-mode">{t('gateway.busyMode')}</label>
              <select
                id="busy-mode"
                value={config.busy_input_mode}
                onChange={(e) =>
                  setConfig({ ...config, busy_input_mode: e.target.value })
                }
              >
                <option value="queue">{t('gateway.queue')}</option>
                <option value="interrupt">{t('gateway.interrupt')}</option>
              </select>
              <small className="form-hint">
                {t('gateway.busyHint')}
              </small>
            </div>

            <div className="form-group">
              <label>
                <input
                  type="checkbox"
                  checked={config.allow_all}
                  onChange={(e) =>
                    setConfig({ ...config, allow_all: e.target.checked })
                  }
                />
                {t('gateway.allowAll')}
              </label>
              <small className="form-hint">
                {t('gateway.allowAllHint')}
              </small>
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
                      allowed_users: e.target.value
                        .split('\n')
                        .map((s) => s.trim())
                        .filter((s) => s.length > 0),
                    })
                  }
                />
                <small className="form-hint">
                  {t('gateway.whitelistHint')}
                </small>
              </div>
            )}

            <div className="form-group">
              <label>
                <input
                  type="checkbox"
                  checked={config.filter_silence_narration}
                  onChange={(e) =>
                    setConfig({
                      ...config,
                      filter_silence_narration: e.target.checked,
                    })
                  }
                />
                {t('gateway.filterNarration')}
              </label>
              <small className="form-hint">
                {t('gateway.filterNarrationHint')}
              </small>
            </div>

            <button
              className="btn-save"
              onClick={handleSaveConfig}
              disabled={saving}
            >
              {saving ? (
                <>
                  <Loader2 className="icon-spin" size={16} />
                  {t('gateway.saving')}
                </>
              ) : (
                <>
                  <BadgeCheck size={16} />
                  {t('gateway.saveConfig')}
                </>
              )}
            </button>
          </div>
        </section>
      )}
    </div>
  );
}
