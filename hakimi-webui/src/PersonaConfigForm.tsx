import { Loader2, Save, Trash2, X } from 'lucide-react';
import { useEffect, useMemo, useState, type FormEvent } from 'react';
import { api, type Agent, type AgentMemoryResponse } from './api';
import { useI18n } from './i18n';

interface PersonaConfigFormProps {
  agent: Agent | null;
  availableSkills: string[];
  onSaved: (agent: Agent) => void;
  onDeleted: (id: string) => void;
  onCancel: () => void;
}

const ID_PATTERN = /^[a-z0-9][a-z0-9_-]{0,63}$/;

export default function PersonaConfigForm({
  agent,
  availableSkills,
  onSaved,
  onDeleted,
  onCancel,
}: PersonaConfigFormProps) {
  const { t } = useI18n();
  const isEdit = agent !== null;
  const [id, setId] = useState(agent?.id ?? '');
  const [name, setName] = useState(agent?.name ?? '');
  const [avatar, setAvatar] = useState(agent?.avatar ?? '');
  const [description, setDescription] = useState(agent?.description ?? '');
  const [model, setModel] = useState(agent?.model ?? '');
  const [reasoning, setReasoning] = useState(agent?.reasoning_effort ?? '');
  const [systemPrompt, setSystemPrompt] = useState(agent?.system_prompt ?? '');
  const [skills, setSkills] = useState<string[]>(agent?.enabled_skills ?? []);
  const [bindingsText, setBindingsText] = useState((agent?.bindings ?? []).join('\n'));
  const [isDefault, setIsDefault] = useState(agent?.is_default ?? false);
  const [addressable, setAddressable] = useState(agent?.addressable ?? true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fetchedSkills, setFetchedSkills] = useState<string[] | null>(null);
  const [memory, setMemory] = useState<AgentMemoryResponse | null>(null);
  const [memoryLoading, setMemoryLoading] = useState(false);

  const idValid = useMemo(() => ID_PATTERN.test(id.trim()), [id]);

  useEffect(() => {
    if (!agent) {
      return;
    }
    const personaId = agent.id;
    const timer = window.setTimeout(() => {
      void api
        .agentSkills(personaId)
        .then((res) => setFetchedSkills(res.available.map((skill) => skill.name)))
        .catch(() => {});
    }, 0);
    return () => window.clearTimeout(timer);
  }, [agent]);

  useEffect(() => {
    if (!agent) {
      setMemory(null);
      return;
    }
    const personaId = agent.id;
    setMemoryLoading(true);
    const timer = window.setTimeout(() => {
      void api
        .agentMemory(personaId)
        .then((res) => setMemory(res))
        .catch(() => setMemory(null))
        .finally(() => setMemoryLoading(false));
    }, 0);
    return () => window.clearTimeout(timer);
  }, [agent]);

  // Re-sync form fields when the agent prop changes (e.g. switching persona)
  useEffect(() => {
    setId(agent?.id ?? '');
    setName(agent?.name ?? '');
    setAvatar(agent?.avatar ?? '');
    setDescription(agent?.description ?? '');
    setModel(agent?.model ?? '');
    setReasoning(agent?.reasoning_effort ?? '');
    setSystemPrompt(agent?.system_prompt ?? '');
    setSkills(agent?.enabled_skills ?? []);
    setBindingsText((agent?.bindings ?? []).join('\n'));
    setIsDefault(agent?.is_default ?? false);
    setAddressable(agent?.addressable ?? true);
  }, [agent]);

  const skillOptions = useMemo(() => {
    const base = new Set(fetchedSkills ?? availableSkills);
    for (const skill of skills) {
      base.add(skill);
    }
    return Array.from(base).sort();
  }, [fetchedSkills, availableSkills, skills]);

  function toggleSkill(skill: string) {
    setSkills((current) =>
      current.includes(skill) ? current.filter((s) => s !== skill) : [...current, skill],
    );
  }

  function parsedBindings(): string[] {
    return bindingsText
      .split('\n')
      .map((line) => line.trim())
      .filter(Boolean);
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const reasoningValue = (reasoning ?? '').trim();
      if (isEdit && agent) {
        const saved = await api.updateAgent(agent.id, {
          name,
          avatar,
          description,
          model,
          reasoning_effort: reasoningValue,
          system_prompt: systemPrompt,
          enabled_skills: skills,
          bindings: parsedBindings(),
          is_default: isDefault,
          addressable,
        });
        onSaved(saved);
      } else {
        if (!idValid) {
          setError(t('persona.idError'));
          setBusy(false);
          return;
        }
        const saved = await api.createAgent({
          id: id.trim(),
          name,
          avatar,
          description,
          model,
          reasoning_effort: reasoningValue || undefined,
          system_prompt: systemPrompt,
          enabled_skills: skills,
          bindings: parsedBindings(),
          is_default: isDefault,
          addressable,
        });
        onSaved(saved);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleDelete() {
    if (!isEdit || !agent || busy) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.deleteAgent(agent.id);
      onDeleted(agent.id);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    }
  }

  return (
    <form className="persona-form settings-surface" onSubmit={handleSubmit}>
      <div className="settings-header">
        <div>
          <p className="eyebrow">{isEdit ? t('persona.edit') : t('persona.new')}</p>
          <h2>{isEdit ? agent?.name || agent?.id : t('persona.create')}</h2>
        </div>
        <button type="button" className="icon-button" onClick={onCancel} title={t('persona.cancel')}>
          <X size={16} aria-hidden="true" />
        </button>
      </div>

      {error && <div className="notice notice-error">{error}</div>}

      <div className="settings-grid">
        <fieldset className="settings-group">
          <legend>{t('persona.identity')}</legend>
          {!isEdit && (
            <label>
              {t('persona.id')}
              <input value={id} onChange={(e) => setId(e.target.value)} placeholder="coder" />
            </label>
          )}
          <label>
            {t('persona.name')}
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Coder" />
          </label>
          <label>
            {t('persona.avatarEmoji')}
            <input value={avatar} onChange={(e) => setAvatar(e.target.value)} placeholder="🤖" />
          </label>
          <label>
            {t('persona.description')}
            <input
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Short role summary"
            />
          </label>
        </fieldset>

        <fieldset className="settings-group">
          <legend>{t('persona.model')}</legend>
          <label>
            {t('persona.modelField')}
            <input
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder={t('persona.inheritDefault')}
            />
          </label>
          <label>
            {t('persona.reasoningEffort')}
            <select value={reasoning ?? ''} onChange={(e) => setReasoning(e.target.value)}>
              <option value="">{t('persona.default')}</option>
              <option value="low">low</option>
              <option value="medium">medium</option>
              <option value="high">high</option>
            </select>
          </label>
          <label className="switch-row">
            <span>{t('persona.isDefault')}</span>
            <input
              type="checkbox"
              checked={isDefault}
              onChange={(e) => setIsDefault(e.target.checked)}
            />
          </label>
          <label className="switch-row">
            <span>{t('persona.addressable')}</span>
            <input
              type="checkbox"
              checked={addressable}
              onChange={(e) => setAddressable(e.target.checked)}
            />
          </label>
        </fieldset>

        <fieldset className="settings-group settings-group-wide">
          <legend>{t('persona.systemPrompt')}</legend>
          <label>
            {t('persona.identityPrompt')}
            <textarea
              value={systemPrompt}
              onChange={(e) => setSystemPrompt(e.target.value)}
              placeholder="You are..."
            />
          </label>
        </fieldset>

        <fieldset className="settings-group settings-group-wide">
          <legend>{t('persona.skills')}</legend>
          <div className="persona-skill-chips">
            {skillOptions.length === 0 && (
              <span className="panel-empty">{t('persona.noSkills')}</span>
            )}
            {skillOptions.map((skill) => (
              <button
                type="button"
                key={skill}
                className={`persona-skill-chip ${skills.includes(skill) ? 'is-on' : ''}`}
                onClick={() => toggleSkill(skill)}
              >
                {skill}
              </button>
            ))}
          </div>
        </fieldset>

        <fieldset className="settings-group settings-group-wide">
          <legend>{t('persona.channelBindings')}</legend>
          <label>
            {t('persona.bindingsHint')}
            <textarea
              className="persona-bindings"
              value={bindingsText}
              onChange={(e) => setBindingsText(e.target.value)}
              placeholder={'telegram:devbot\nslack:support'}
            />
          </label>
        </fieldset>

        {isEdit && (
          <fieldset className="settings-group settings-group-wide">
            <legend>{t('persona.memory')}</legend>
            {memoryLoading ? (
              <div className="panel-empty">
                <Loader2 className="spin" size={16} aria-hidden="true" />
              </div>
            ) : memory ? (
              <div className="persona-memory">
                <div className="persona-memory-dir">
                  <small>{t('persona.memoryDir')}: <code>{memory.dir}</code></small>
                </div>
                {memory.memory_md && (
                  <div className="persona-memory-index">
                    <small>{t('persona.memoryIndex')}:</small>
                    <pre className="persona-memory-content">{memory.memory_md}</pre>
                  </div>
                )}
                {memory.files.length > 0 && (
                  <div className="persona-memory-files">
                    {memory.files.map((f) => (
                      <span key={f} className="persona-memory-file">{f}</span>
                    ))}
                  </div>
                )}
                {!memory.memory_md && memory.files.length === 0 && (
                  <div className="panel-empty">{t('persona.noMemory')}</div>
                )}
              </div>
            ) : (
              <div className="panel-empty">{t('persona.noMemory')}</div>
            )}
          </fieldset>
        )}
      </div>

      <div className="persona-form-actions">
        <button className="button button-primary" type="submit" disabled={busy}>
          {busy ? <Loader2 className="spin" size={16} /> : <Save size={16} />}
          <span>{t('persona.save')}</span>
        </button>
        {isEdit && !agent?.is_default && (
          <button
            className="button persona-delete"
            type="button"
            onClick={handleDelete}
            disabled={busy}
          >
            <Trash2 size={16} aria-hidden="true" />
            <span>{t('persona.deleteBtn')}</span>
          </button>
        )}
        <button className="button" type="button" onClick={onCancel} disabled={busy}>
          <span>{t('persona.cancel')}</span>
        </button>
      </div>
    </form>
  );
}
