import { Loader2, RefreshCcw, Share2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api, type BindingsResponse } from './api';
import GatewayPanel from './GatewayPanel';
import SettingsPanel from './SettingsPanel';
import { useI18n } from './i18n';

export default function InstanceSettings() {
  const { t } = useI18n();
  const [bindings, setBindings] = useState<BindingsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      setBindings(await api.bindings());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void load();
    }, 0);
    return () => window.clearTimeout(timer);
  }, []);

  const entries = bindings ? Object.entries(bindings.bindings) : [];

  return (
    <div className="instance-settings">
      <section className="settings-surface">
        <div className="settings-header">
          <div>
            <p className="eyebrow">{t('instance.routing')}</p>
            <h2>{t('instance.bindings')}</h2>
          </div>
          <button
            className="icon-button"
            type="button"
            onClick={() => void load()}
            title={t('common.refresh')}
          >
            {loading ? <Loader2 className="spin" size={16} /> : <RefreshCcw size={16} />}
          </button>
        </div>
        {error && <div className="notice notice-error">{error}</div>}
        <div className="bindings-table">
          <div className="bindings-row bindings-head">
            <span>
              <Share2 size={13} aria-hidden="true" /> {t('instance.channel')}
            </span>
            <span>{t('instance.persona')}</span>
          </div>
          {entries.map(([channel, persona]) => (
            <div className="bindings-row" key={channel}>
              <span className="bindings-channel">{channel}</span>
              <span className="bindings-persona">{persona}</span>
            </div>
          ))}
          {!loading && entries.length === 0 && (
            <div className="panel-empty">{t('instance.noBindings')}</div>
          )}
        </div>
        {bindings && (
          <p className="bindings-default">
            {t('instance.defaultFallback')} <strong>{bindings.default}</strong>
          </p>
        )}
      </section>

      <SettingsPanel />
      <GatewayPanel />
    </div>
  );
}
