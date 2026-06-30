import { Bot, Building2, FolderTree, Plus, Settings } from 'lucide-react';
import type { Agent } from './api';

interface PersonaRailProps {
  agents: Agent[];
  activeId: string | null;
  view: 'chat' | 'config' | 'instance' | 'workspace' | 'office';
  onSelect: (id: string) => void;
  onEdit: (id: string) => void;
  onCreate: () => void;
  onInstance: () => void;
  onWorkspace: () => void;
  onOffice: () => void;
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
  onWorkspace,
  onOffice,
}: PersonaRailProps) {
  return (
    <nav className="persona-rail" aria-label="Personas">
      <div className="persona-rail-list">
        {agents.map((agent) => {
          const active = agent.id === activeId && view === 'chat';
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
          className={`persona-instance ${view === 'office' ? 'is-active' : ''}`}
          title="Office"
          onClick={onOffice}
        >
          <Building2 size={18} aria-hidden="true" />
        </button>
        <button
          type="button"
          className={`persona-instance ${view === 'workspace' ? 'is-active' : ''}`}
          title="Workspace files"
          onClick={onWorkspace}
        >
          <FolderTree size={18} aria-hidden="true" />
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
