import { Loader2, Save, Trash2, X } from 'lucide-react';
import { useEffect, useMemo, useState, type FormEvent } from 'react';
import { api, type Agent } from './api';

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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fetchedSkills, setFetchedSkills] = useState<string[] | null>(null);

  const idValid = useMemo(() => ID_PATTERN.test(id.trim()), [id]);

  // In edit mode, load the persona's actual skills (from its skills dir) so the
  // chips reflect what is available to this persona, not just the instance set.
  useEffect(() => {
    if (!agent) {
      return;
    }
    const personaId = agent.id;
    const timer = window.setTimeout(() => {
      void api
        .agentSkills(personaId)
        .then((res) => setFetchedSkills(res.available.map((skill) => skill.name)))
        .catch(() => {
          // Keep the instance-wide fallback on failure.
        });
    }, 0);
    return () => window.clearTimeout(timer);
  }, [agent]);

  // Skills to show as chips: the persona's available skills (edit mode) or the
  // instance set (create mode), always including currently-enabled skills.
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
        });
        onSaved(saved);
      } else {
        if (!idValid) {
          setError('Persona id must match [a-z0-9][a-z0-9_-]{0,63}');
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
          <p className="eyebrow">{isEdit ? 'Edit persona' : 'New persona'}</p>
          <h2>{isEdit ? agent?.name || agent?.id : 'Create a persona'}</h2>
        </div>
        <button type="button" className="icon-button" onClick={onCancel} title="Cancel">
          <X size={16} aria-hidden="true" />
        </button>
      </div>

      {error && <div className="notice notice-error">{error}</div>}

      <div className="settings-grid">
        <fieldset className="settings-group">
          <legend>Identity</legend>
          {!isEdit && (
            <label>
              id
              <input value={id} onChange={(e) => setId(e.target.value)} placeholder="coder" />
            </label>
          )}
          <label>
            name
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Coder" />
          </label>
          <label>
            avatar (emoji)
            <input value={avatar} onChange={(e) => setAvatar(e.target.value)} placeholder="🤖" />
          </label>
          <label>
            description
            <input
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Short role summary"
            />
          </label>
        </fieldset>

        <fieldset className="settings-group">
          <legend>Model</legend>
          <label>
            model
            <input
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder="(inherit default)"
            />
          </label>
          <label>
            reasoning effort
            <select value={reasoning ?? ''} onChange={(e) => setReasoning(e.target.value)}>
              <option value="">(default)</option>
              <option value="low">low</option>
              <option value="medium">medium</option>
              <option value="high">high</option>
            </select>
          </label>
          <label className="switch-row">
            <span>Default persona (gateway fallback)</span>
            <input
              type="checkbox"
              checked={isDefault}
              onChange={(e) => setIsDefault(e.target.checked)}
            />
          </label>
        </fieldset>

        <fieldset className="settings-group settings-group-wide">
          <legend>System prompt</legend>
          <label>
            identity prompt
            <textarea
              value={systemPrompt}
              onChange={(e) => setSystemPrompt(e.target.value)}
              placeholder="You are…"
            />
          </label>
        </fieldset>

        <fieldset className="settings-group settings-group-wide">
          <legend>Skills</legend>
          <div className="persona-skill-chips">
            {skillOptions.length === 0 && (
              <span className="panel-empty">No skills available</span>
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
          <legend>Channel bindings</legend>
          <label>
            one platform:bot_id per line (empty = WebUI only)
            <textarea
              className="persona-bindings"
              value={bindingsText}
              onChange={(e) => setBindingsText(e.target.value)}
              placeholder={'telegram:devbot\nslack:support'}
            />
          </label>
        </fieldset>
      </div>

      <div className="persona-form-actions">
        <button className="button button-primary" type="submit" disabled={busy}>
          {busy ? <Loader2 className="spin" size={16} /> : <Save size={16} />}
          <span>Save</span>
        </button>
        {isEdit && !agent?.is_default && (
          <button
            className="button persona-delete"
            type="button"
            onClick={handleDelete}
            disabled={busy}
          >
            <Trash2 size={16} aria-hidden="true" />
            <span>Delete</span>
          </button>
        )}
        <button className="button" type="button" onClick={onCancel} disabled={busy}>
          <span>Cancel</span>
        </button>
      </div>
    </form>
  );
}
