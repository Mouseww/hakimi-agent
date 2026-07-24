/**
 * Phase 3 skeleton: Fleet / Skills / MCP / Cron hub panels.
 * Reads existing REST APIs (no new backend required for v1 shell).
 */
import { useEffect, useState } from 'react';
import { Bot, Server, Sparkles } from 'lucide-react';
import {
  api,
  type Agent,
  type AgentsListResponse,
  type McpServersResponse,
  type SkillInfo,
  type SkillsResponse,
} from '../api';
import StudioCronPanel from './StudioCronPanel';

type Props = {
  open: boolean;
};

export default function StudioEcosystemPanel({ open }: Props) {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [mcpNames, setMcpNames] = useState<string[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setLoading(true);
    setErr(null);
    Promise.allSettled([api.agents(), api.skills(), api.mcpServers()])
      .then((results) => {
        if (cancelled) return;
        const [a, s, m] = results;
        if (a.status === 'fulfilled') {
          const body = a.value as AgentsListResponse | Agent[];
          setAgents(Array.isArray(body) ? body : body.agents ?? []);
        }
        if (s.status === 'fulfilled') {
          const body = s.value as SkillsResponse | SkillInfo[];
          if (Array.isArray(body)) setSkills(body);
          else setSkills(body.data ?? []);
        }
        if (m.status === 'fulfilled') {
          const body = m.value as McpServersResponse;
          setMcpNames((body.servers ?? []).map((x) => x.name));
        }
        if (
          a.status === 'rejected' &&
          s.status === 'rejected' &&
          m.status === 'rejected'
        ) {
          setErr('Could not load Fleet/Skills/MCP (API offline?)');
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  if (!open) return null;

  return (
    <div className="studio-ecosystem" aria-label="Agent fleet and ecosystem">
      <div className="studio-eco-card">
        <h4>
          <Bot size={12} /> Fleet {loading ? '…' : `(${agents.length})`}
        </h4>
        {agents.length === 0 && (
          <div className="studio-empty tiny">No sub-agents listed</div>
        )}
        {agents.slice(0, 12).map((ag) => (
          <div key={ag.id} className="studio-eco-item" title={ag.id}>
            {ag.name || ag.id}
            {ag.model ? <span className="tag">{ag.model}</span> : null}
          </div>
        ))}
      </div>
      <div className="studio-eco-card">
        <h4>
          <Sparkles size={12} /> Skills ({skills.length})
        </h4>
        {skills.length === 0 && (
          <div className="studio-empty tiny">No skills loaded</div>
        )}
        {skills.slice(0, 16).map((sk) => (
          <div key={sk.name} className="studio-eco-item" title={sk.description}>
            {sk.name}
            {!sk.active ? <span className="tag">off</span> : null}
          </div>
        ))}
      </div>
      <div className="studio-eco-card">
        <h4>
          <Server size={12} /> MCP ({mcpNames.length})
        </h4>
        {mcpNames.length === 0 && (
          <div className="studio-empty tiny">No MCP servers</div>
        )}
        {mcpNames.slice(0, 12).map((name) => (
          <div key={name} className="studio-eco-item">
            {name}
          </div>
        ))}
      </div>
      <div className="studio-eco-card studio-eco-cron">
        <StudioCronPanel open={open} />
      </div>
      {err && <div className="studio-empty tiny">{err}</div>}
    </div>
  );
}
