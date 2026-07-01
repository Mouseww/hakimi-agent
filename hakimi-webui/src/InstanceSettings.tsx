import { Edit3, Loader2, Plus, RefreshCcw, Share2, Trash2, X } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api, type Agent, type BindingsResponse } from './api';
import GatewayPanel from './GatewayPanel';
import SettingsPanel from './SettingsPanel';
import { useI18n } from './i18n';

interface BindingEditorState {
  mode: 'add' | 'edit';
  channel: string;
  personaId: string;
  originalChannel?: string;
}

export default function InstanceSettings() {
  const { t } = useI18n();
  const [bindings, setBindings] = useState<BindingsResponse | null>(null);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [editor, setEditor] = useState<BindingEditorState | null>(null);
  const [saving, setSaving] = useState(false);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const [bindingsRes, agentsRes] = await Promise.all([
        api.bindings(),
        api.agents(),
      ]);
      setBindings(bindingsRes);
      setAgents(agentsRes.agents);
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

  function openAdd() {
    setEditor({ mode: 'add', channel: '', personaId: agents[0]?.id ?? '' });
  }

  function openEdit(channel: string, personaId: string) {
    setEditor({ mode: 'edit', channel, personaId, originalChannel: channel });
  }

  function closeEditor() {
    setEditor(null);
  }

  async function saveBinding() {
    if (!editor || saving) return;
    const channel = editor.channel.trim();
    const personaId = editor.personaId.trim();
    if (!channel || !personaId) return;

    setSaving(true);
    setError(null);
    try {
      const targetAgent = agents.find((a) => a.id === personaId);
      if (!targetAgent) {
        setError('Agent not found');
        setSaving(false);
        return;
      }

      const currentBindings = new Set(targetAgent.bindings);

      if (editor.mode === 'edit' && editor.originalChannel && editor.originalChannel !== channel) {
        const oldAgent = agents.find((a) => a.bindings.includes(editor.originalChannel!));
        if (oldAgent && oldAgent.id !== personaId) {
          const oldBindings = oldAgent.bindings.filter((b) => b !== editor.originalChannel);
          await api.updateAgent(oldAgent.id, { bindings: oldBindings });
        } else if (oldAgent && oldAgent.id === personaId) {
          currentBindings.delete(editor.originalChannel);
        }
      }

      if (editor.mode === 'edit' && editor.originalChannel) {
        const oldOwner = agents.find((a) => a.bindings.includes(editor.originalChannel!) && a.id !== personaId);
        if (oldOwner) {
          const cleaned = oldOwner.bindings.filter((b) => b !== editor.originalChannel);
          await api.updateAgent(oldOwner.id, { bindings: cleaned });
        }
        if (targetAgent.bindings.includes(editor.originalChannel)) {
          currentBindings.delete(editor.originalChannel);
        }
      }

      currentBindings.add(channel);
      await api.updateAgent(personaId, { bindings: Array.from(currentBindings) });
      setEditor(null);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  }

  async function deleteBinding(channel: string) {
    if (!window.confirm(t('instance.deleteConfirm'))) return;
    setSaving(true);
    setError(null);
    try {
      const ownerAgent = agents.find((a) => a.bindings.includes(channel));
      if (ownerAgent) {
        const newBindings = ownerAgent.bindings.filter((b) => b !== channel);
        await api.updateAgent(ownerAgent.id, { bindings: newBindings });
      }
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="instance-settings">
      <section className="settings-surface">
        <div className="settings-header">
          <div>
            <p className="eyebrow">{t('instance.routing')}</p>
            <h2>{t('instance.channelBindings')}</h2>
          </div>
          <div style={{ display: 'flex', gap: '6px' }}>
            <button
              className="button button-primary"
              type="button"
              onClick={openAdd}
              title={t('instance.addBinding')}
              disabled={agents.length === 0}
            >
              <Plus size={14} aria-hidden="true" />
              <span>{t('instance.addBinding')}</span>
            </button>
            <button
              className="icon-button"
              type="button"
              onClick={() => void load()}
              title={t('topbar.refresh')}
            >
              {loading ? <Loader2 className="spin" size={16} /> : <RefreshCcw size={16} />}
            </button>
          </div>
        </div>
        {error && <div className="notice notice-error">{error}</div>}

        {editor && (
          <div className="bindings-editor">
            <div className="bindings-editor-row">
              <label>
                {t('instance.channel')}
                <input
                  value={editor.channel}
                  onChange={(e) => setEditor({ ...editor, channel: e.target.value })}
                  placeholder="telegram:devbot"
                  autoFocus
                />
              </label>
              <label>
                {t('instance.selectPersona')}
                <select
                  value={editor.personaId}
                  onChange={(e) => setEditor({ ...editor, personaId: e.target.value })}
                >
                  {agents.map((a) => (
                    <option key={a.id} value={a.id}>
                      {a.avatar} {a.name || a.id}
                    </option>
                  ))}
                </select>
              </label>
              <div className="bindings-editor-actions">
                <button
                  className="button button-primary"
                  type="button"
                  onClick={() => void saveBinding()}
                  disabled={saving || !editor.channel.trim() || !editor.personaId.trim()}
                >
                  {t('instance.saveBinding')}
                </button>
                <button className="icon-button" type="button" onClick={closeEditor} title={t('instance.cancelBinding')}>
                  <X size={16} />
                </button>
              </div>
            </div>
          </div>
        )}

        <div className="bindings-table">
          <div className="bindings-row bindings-head">
            <span>
              <Share2 size={13} aria-hidden="true" /> {t('instance.platformBotId')}
            </span>
            <span>{t('instance.persona')}</span>
            <span>{t('instance.actions')}</span>
          </div>
          {entries.map(([channel, persona]) => (
            <div className="bindings-row" key={channel}>
              <span className="bindings-channel">{channel}</span>
              <span className="bindings-persona">{persona}</span>
              <span className="bindings-actions">
                <button
                  type="button"
                  className="icon-button"
                  title={t('instance.editBinding')}
                  onClick={() => openEdit(channel, agents.find((a) => a.bindings.includes(channel))?.id ?? '')}
                >
                  <Edit3 size={13} />
                </button>
                <button
                  type="button"
                  className="icon-button"
                  title={t('instance.deleteBinding')}
                  onClick={() => void deleteBinding(channel)}
                  disabled={saving}
                >
                  <Trash2 size={13} />
                </button>
              </span>
            </div>
          ))}
          {!loading && entries.length === 0 && (
            <div className="panel-empty">
              {t('instance.noBindings')}
            </div>
          )}
        </div>
        {bindings && (
          <p className="bindings-default">
            {t('instance.defaultPersona')}: <strong>{bindings.default}</strong>
          </p>
        )}
      </section>

      <SettingsPanel />
      <GatewayPanel />
    </div>
  );
}
