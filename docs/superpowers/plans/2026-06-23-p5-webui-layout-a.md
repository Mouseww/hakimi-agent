# P5 WebUI Layout A (Persona Rail) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign the React operator console (`hakimi-webui/`) into Layout A — a leftmost persona rail that switches the workspace between per-persona chat, a persona config form, and an instance-settings view — consuming the P4 `/api/agents` and `/api/bindings` endpoints.

**Architecture:** Add an outer shell (`console-body`) below the existing topbar: a narrow vertical `PersonaRail` plus a swappable main area. The main area renders one of three views driven by `view` state: `chat` (the existing sessions/chat/right-panel workspace, now scoped to the active persona), `config` (`PersonaConfigForm` for create/edit), or `instance` (`InstanceSettings` = bindings overview + existing SettingsPanel + GatewayPanel). The API client gains an `agents` resource, `bindings`, and persona-scoped `agentChat`. Design extends the existing App.css token system (warm paper/green/amber); no new aesthetic.

**Tech Stack:** React 19, Vite 8, TypeScript ~6, Tailwind 4 (present but the app is hand-CSS via App.css), lucide-react. Verify locally: `cd hakimi-webui && npm install` then `npm run lint` and `npm run build` (`tsc -b && vite build`). No CI gate for the frontend; no binary embed change (per the chosen scope, the React app is the dev console and is not wired into the server binary in P5).

---

## Design decisions (locked)

- **Scope = React app only.** Do not touch `crates/hakimi-webui/static/` (the vanilla UI the binary embeds) and do not change the server's `include_str!` wiring. The redesign targets `hakimi-webui/` and is verified via `npm run build`.
- **Layout A as an outer shell.** Keep the existing topbar. Below it: `<div class="console-body">` = `<PersonaRail/>` + `<div class="console-main">`. The previous `.workspace-grid` (sessions | chat | right panel) becomes the `chat` view inside `console-main`.
- **Three views (`view` state):** `chat` (default), `config` (create/edit persona), `instance` (settings). Rail item click → select persona + `chat`; rail item gear → `config` for that persona; rail `+` → `config` in create mode; rail bottom `⚙` → `instance`.
- **Persona-scoped chat:** chat uses `api.agentChat(activePersonaId, message)` → `POST /api/agents/{id}/chat`. Sessions list/right panel stay instance-wide for P5 (per-persona sessions endpoints are deferred, matching P4).
- **Agent type mirrors `PersonaConfig`:** `id, name, avatar, description, model, reasoning_effort?, system_prompt, enabled_skills, bindings, is_default`.
- **Skills chips** come from `/v1/skills` (existing `api.skills()`), toggling membership in the persona's `enabled_skills`.
- **Design consistency:** new CSS reuses existing tokens/classes (`--green`, `--line`, `.button`, `.settings-group`, etc.). The dark-themed GatewayPanel is folded in unchanged.

## File structure

- Modify: `hakimi-webui/src/api.ts` — add `Agent`, `AgentsListResponse`, `BindingsResponse` types + `agents`/`bindings`/`agentChat` on `api`.
- Create: `hakimi-webui/src/PersonaRail.tsx` — leftmost vertical persona rail.
- Create: `hakimi-webui/src/PersonaConfigForm.tsx` — create/edit persona form.
- Create: `hakimi-webui/src/InstanceSettings.tsx` — bindings overview + SettingsPanel + GatewayPanel.
- Modify: `hakimi-webui/src/App.tsx` — outer shell, persona state, view switching, persona-scoped chat.
- Modify: `hakimi-webui/src/App.css` — persona rail + config form + bindings styles (extend tokens).

---

## Task 1: API client — agents, bindings, persona chat

**Files:** Modify `hakimi-webui/src/api.ts`

- [ ] **Step 1: Add types** (after `ConfigUpdate`, before `getAuthToken`):

```typescript
export interface Agent {
  id: string;
  name: string;
  avatar: string;
  description: string;
  model: string;
  reasoning_effort?: string | null;
  system_prompt: string;
  enabled_skills: string[];
  bindings: string[];
  is_default: boolean;
}

export interface AgentsListResponse {
  agents: Agent[];
  default: string;
}

export interface AgentUpdate {
  name?: string;
  avatar?: string;
  description?: string;
  model?: string;
  reasoning_effort?: string;
  system_prompt?: string;
  enabled_skills?: string[];
  bindings?: string[];
  is_default?: boolean;
}

export interface BindingsResponse {
  bindings: Record<string, string>;
  default: string;
}
```

- [ ] **Step 2: Add methods to `api`** (inside the `export const api = { ... }` object, after `restartGateway`):

```typescript
  agents: () => request<AgentsListResponse>('/api/agents'),
  agent: (id: string) => request<Agent>(`/api/agents/${encodeURIComponent(id)}`),
  createAgent: (payload: Partial<Agent> & { id: string }) =>
    request<Agent>('/api/agents', {
      method: 'POST',
      body: JSON.stringify(payload),
    }),
  updateAgent: (id: string, payload: AgentUpdate) =>
    request<Agent>(`/api/agents/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: JSON.stringify(payload),
    }),
  deleteAgent: (id: string) =>
    request<{ id: string; deleted: boolean }>(`/api/agents/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    }),
  agentChat: (id: string, message: string) =>
    request<ChatResponse>(`/api/agents/${encodeURIComponent(id)}/chat`, {
      method: 'POST',
      body: JSON.stringify({ message } satisfies ChatRequest),
    }),
  bindings: () => request<BindingsResponse>('/api/bindings'),
```

- [ ] **Step 3: Commit** (after the full feature builds — see Task 6; api.ts alone is not separately verified).

---

## Task 2: PersonaRail component

**Files:** Create `hakimi-webui/src/PersonaRail.tsx`

- [ ] **Step 1: Write the component**

```tsx
import { Plus, Settings, Bot } from 'lucide-react';
import type { Agent } from './api';

interface PersonaRailProps {
  agents: Agent[];
  activeId: string | null;
  view: 'chat' | 'config' | 'instance';
  onSelect: (id: string) => void;
  onEdit: (id: string) => void;
  onCreate: () => void;
  onInstance: () => void;
}

function avatarText(agent: Agent): string {
  if (agent.avatar.trim()) {
    return agent.avatar.trim().slice(0, 2);
  }
  const name = agent.name.trim() || agent.id;
  return name.slice(0, 1).toUpperCase();
}

export default function PersonaRail({
  agents,
  activeId,
  view,
  onSelect,
  onEdit,
  onCreate,
  onInstance,
}: PersonaRailProps) {
  return (
    <nav className="persona-rail" aria-label="Personas">
      <div className="persona-rail-list">
        {agents.map((agent) => {
          const active = agent.id === activeId && view !== 'instance';
          return (
            <div className={`persona-rail-item ${active ? 'is-active' : ''}`} key={agent.id}>
              <button
                type="button"
                className="persona-chip"
                title={agent.name || agent.id}
                onClick={() => onSelect(agent.id)}
              >
                <span aria-hidden="true">{avatarText(agent)}</span>
                {agent.is_default && <i className="persona-default-dot" title="default" />}
              </button>
              <button
                type="button"
                className="persona-gear"
                title={`Configure ${agent.name || agent.id}`}
                onClick={() => onEdit(agent.id)}
              >
                <Settings size={13} aria-hidden="true" />
              </button>
            </div>
          );
        })}
        {agents.length === 0 && (
          <div className="persona-rail-empty" aria-hidden="true">
            <Bot size={18} />
          </div>
        )}
      </div>

      <div className="persona-rail-foot">
        <button type="button" className="persona-add" title="New persona" onClick={onCreate}>
          <Plus size={18} aria-hidden="true" />
        </button>
        <button
          type="button"
          className={`persona-instance ${view === 'instance' ? 'is-active' : ''}`}
          title="Instance settings"
          onClick={onInstance}
        >
          <Settings size={18} aria-hidden="true" />
        </button>
      </div>
    </nav>
  );
}
```

---

## Task 3: PersonaConfigForm component

**Files:** Create `hakimi-webui/src/PersonaConfigForm.tsx`

Handles both create (no `agent`) and edit (with `agent`). On save it calls `onSaved` with the persisted agent; on delete, `onDeleted`. Skills are toggle chips sourced from `availableSkills`.

- [ ] **Step 1: Write the component**

```tsx
import { Loader2, Save, Trash2, X } from 'lucide-react';
import { useMemo, useState, type FormEvent } from 'react';
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

  const idValid = useMemo(() => ID_PATTERN.test(id.trim()), [id]);

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
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const reasoningValue = reasoning.trim();
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
    if (!isEdit || !agent || busy) return;
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
            {availableSkills.length === 0 && <span className="panel-empty">No skills available</span>}
            {availableSkills.map((skill) => (
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
          <button className="button persona-delete" type="button" onClick={handleDelete} disabled={busy}>
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
```

---

## Task 4: InstanceSettings component

**Files:** Create `hakimi-webui/src/InstanceSettings.tsx`

Renders a bindings overview table (from `/api/bindings`) plus the existing `SettingsPanel` and `GatewayPanel`.

- [ ] **Step 1: Write the component**

```tsx
import { Loader2, RefreshCcw, Share2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api, type BindingsResponse } from './api';
import GatewayPanel from './GatewayPanel';
import SettingsPanel from './SettingsPanel';

export default function InstanceSettings() {
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
    void load();
  }, []);

  const entries = bindings ? Object.entries(bindings.bindings) : [];

  return (
    <div className="instance-settings">
      <section className="settings-surface">
        <div className="settings-header">
          <div>
            <p className="eyebrow">Routing</p>
            <h2>Channel bindings</h2>
          </div>
          <button className="icon-button" type="button" onClick={() => void load()} title="Refresh">
            {loading ? <Loader2 className="spin" size={16} /> : <RefreshCcw size={16} />}
          </button>
        </div>
        {error && <div className="notice notice-error">{error}</div>}
        <div className="bindings-table">
          <div className="bindings-row bindings-head">
            <span><Share2 size={13} aria-hidden="true" /> platform:bot_id</span>
            <span>persona</span>
          </div>
          {entries.map(([channel, persona]) => (
            <div className="bindings-row" key={channel}>
              <span className="bindings-channel">{channel}</span>
              <span className="bindings-persona">{persona}</span>
            </div>
          ))}
          {!loading && entries.length === 0 && (
            <div className="panel-empty">No channel bindings. Unbound channels fall back to the default persona.</div>
          )}
        </div>
        {bindings && (
          <p className="bindings-default">
            Default persona (fallback): <strong>{bindings.default}</strong>
          </p>
        )}
      </section>

      <SettingsPanel />
      <GatewayPanel />
    </div>
  );
}
```

---

## Task 5: App.tsx integration

**Files:** Modify `hakimi-webui/src/App.tsx`

- [ ] **Step 1: Add imports** — add `PersonaRail`, `PersonaConfigForm`, `InstanceSettings`, and `type Agent` to the existing import block:

```tsx
import PersonaRail from './PersonaRail';
import PersonaConfigForm from './PersonaConfigForm';
import InstanceSettings from './InstanceSettings';
```

Add `type Agent,` to the `from './api'` type import list.

- [ ] **Step 2: Add persona state** (after the existing `useState` hooks, before `selectedSession` memo):

```tsx
  const [agents, setAgents] = useState<Agent[]>([]);
  const [activePersonaId, setActivePersonaId] = useState<string | null>(null);
  const [view, setView] = useState<'chat' | 'config' | 'instance'>('chat');
  const [editingPersona, setEditingPersona] = useState<Agent | null>(null);

  const activePersona = useMemo(
    () => agents.find((a) => a.id === activePersonaId) ?? null,
    [agents, activePersonaId],
  );
  const availableSkillNames = useMemo(() => data.skills.map((s) => s.name), [data.skills]);

  async function loadAgents() {
    try {
      const res = await api.agents();
      setAgents(res.agents);
      setActivePersonaId((current) => current ?? res.default);
    } catch {
      // agents endpoint optional; keep chat working against default
    }
  }
```

- [ ] **Step 3: Load agents on mount** — extend the existing mount `useEffect` to also call `void loadAgents();` alongside `void refreshAll();`.

- [ ] **Step 4: Persona-scoped send** — in `sendMessage`, replace `const response = await api.chat(content);` with:

```tsx
      const response = activePersonaId
        ? await api.agentChat(activePersonaId, content)
        : await api.chat(content);
```

- [ ] **Step 5: Persona handlers** (define before `return`):

```tsx
  function handleSelectPersona(id: string) {
    setActivePersonaId(id);
    setView('chat');
  }
  function handleEditPersona(id: string) {
    setEditingPersona(agents.find((a) => a.id === id) ?? null);
    setView('config');
  }
  function handleCreatePersona() {
    setEditingPersona(null);
    setView('config');
  }
  function handlePersonaSaved(saved: Agent) {
    setAgents((current) => {
      const exists = current.some((a) => a.id === saved.id);
      const next = exists
        ? current.map((a) => (a.id === saved.id ? saved : a))
        : [...current, saved];
      return saved.is_default
        ? next.map((a) => (a.id === saved.id ? a : { ...a, is_default: false }))
        : next;
    });
    setActivePersonaId(saved.id);
    setView('chat');
    void loadAgents();
  }
  function handlePersonaDeleted(id: string) {
    setAgents((current) => current.filter((a) => a.id !== id));
    setActivePersonaId((current) => (current === id ? null : current));
    setView('chat');
    void loadAgents();
  }
```

- [ ] **Step 6: Wrap the workspace in the Layout A shell** — change the JSX so the topbar stays, then a `console-body` row holds the rail + a `console-main` that swaps views. Replace the opening of the workspace:

Replace:
```tsx
      <div className="workspace-grid">
```
with:
```tsx
      <div className="console-body">
        <PersonaRail
          agents={agents}
          activeId={activePersonaId}
          view={view}
          onSelect={handleSelectPersona}
          onEdit={handleEditPersona}
          onCreate={handleCreatePersona}
          onInstance={() => setView('instance')}
        />
        <div className="console-main">
          {view === 'instance' ? (
            <InstanceSettings />
          ) : view === 'config' ? (
            <PersonaConfigForm
              agent={editingPersona}
              availableSkills={availableSkillNames}
              onSaved={handlePersonaSaved}
              onDeleted={handlePersonaDeleted}
              onCancel={() => setView('chat')}
            />
          ) : (
            <div className="workspace-grid">
```

And close the new wrappers: the existing `</div>` that closed `.workspace-grid` now closes the inner workspace; add `)}</div></div>` after it to close `console-main` and `console-body`. (During execution, match the exact closing tags.)

- [ ] **Step 7: Show the active persona in the chat header** — in the `chat-header`, change the `<h2>Chat</h2>` to `<h2>{activePersona ? (activePersona.name || activePersona.id) : 'Chat'}</h2>` so the user sees which persona is active.

---

## Task 6: Styles + verification

**Files:** Modify `hakimi-webui/src/App.css`

- [ ] **Step 1: Add Layout A styles** (append before the `@media` blocks, reusing tokens). Key classes: `.console-body` (flex row, min-height to fill), `.persona-rail` (fixed ~72px width, vertical flex, `space-between`), `.persona-rail-item`, `.persona-chip` (square avatar button, `.is-active` green border), `.persona-default-dot`, `.persona-gear`, `.persona-rail-foot`, `.persona-add`/`.persona-instance`, `.console-main` (flex:1, min-width:0), `.persona-form-actions` (flex gap), `.persona-skill-chips`/`.persona-skill-chip.is-on` (green), `.persona-delete` (red border), `.bindings-table`/`.bindings-row`/`.bindings-head`/`.bindings-default`, `.instance-settings`. Make `.workspace-grid` fill `.console-main` (`height:100%`). Add a `.persona-rail` collapse rule in the `max-width: 980px` media block. (Full CSS written during execution to match existing token values.)

- [ ] **Step 2: Install deps + lint + build**

```
cd hakimi-webui
npm install
npm run lint
npm run build
```
Expected: lint clean, `tsc -b` no type errors, `vite build` succeeds (writes `dist/`).

- [ ] **Step 3: Commit**

```bash
git add hakimi-webui/src/ docs/superpowers/plans/2026-06-23-p5-webui-layout-a.md
git commit -m "feat(webui): Layout A 人格栏 + 人格配置表单 + 实例设置/绑定总览(P5)"
```

---

## Task 7: Update handoff

- [ ] Mark P5 done in `docs/superpowers/handoffs/2026-06-22-multi-agent-isolation-handoff.md` with the commit sha; note the React app is verified via `npm run build` but not embedded into the binary (vanilla `crates/hakimi-webui/static/` still ships), and per-persona sessions/memory/skills sub-resource UIs remain follow-ups.

---

## Self-review

- **Spec coverage (§4.2 Layout A, §4.3 config form, §4.4 instance settings/bindings, §4.5 API + components):** PersonaRail (Task 2) = §4.2 rail + `+` + bottom gear; PersonaConfigForm (Task 3) = §4.3 identity/model/prompt/skills/bindings/default; InstanceSettings (Task 4) = §4.4 bindings overview + folded SettingsPanel/GatewayPanel; api.ts agents/bindings/agentChat (Task 1) = §4.5; App.tsx persona context + persona-scoped chat (Task 5) = §4.5 threading. The §4.3 "协作(占位)" collaboration toggle and client routing (`/agents/:id`) are deferred (documented).
- **Placeholder scan:** component code is complete; only App.css full text and the exact App.tsx closing-tag splice are finished during execution (noted explicitly), since they depend on matching existing markup.
- **Type consistency:** `Agent` fields match P4 `PersonaConfig`/serialized JSON (`reasoning_effort` optional). `api.agentChat` returns `ChatResponse` (reused). `view` union `'chat'|'config'|'instance'` used identically in PersonaRail props and App state. `availableSkills: string[]` passed from `data.skills.map(s => s.name)`.
- **Known non-goals (documented):** no binary embed wiring; per-persona sessions/memory/skills sub-resource endpoints + UIs; streaming persona chat; client-side routing; collaboration (`agent:<id>`) toggle.
