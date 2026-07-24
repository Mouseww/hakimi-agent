/**
 * Phase 3.4: Cron Hub panel — list / pause / resume / run-now / create / delete.
 */
import { useCallback, useEffect, useState } from 'react';
import { Clock, Play, Pause, Trash2, Plus, RefreshCcw } from 'lucide-react';
import { api, type CronJobInfo } from '../api';
import { Danger } from './dangerConfirm';

type Props = {
  open: boolean;
};

export default function StudioCronPanel({ open }: Props) {
  const [jobs, setJobs] = useState<CronJobInfo[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [showCreate, setShowCreate] = useState(false);
  const [name, setName] = useState('');
  const [schedule, setSchedule] = useState('every 1h');
  const [prompt, setPrompt] = useState('');
  const [busyId, setBusyId] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setLoading(true);
    setErr(null);
    try {
      const body = await api.cronJobs();
      setJobs(body.jobs ?? []);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
      setJobs([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    void reload();
  }, [open, reload]);

  if (!open) return null;

  async function onCreate(e: React.FormEvent) {
    e.preventDefault();
    if (!schedule.trim() || !prompt.trim()) return;
    setBusyId('create');
    try {
      await api.createCronJob({
        name: name.trim() || undefined,
        schedule: schedule.trim(),
        prompt: prompt.trim(),
      });
      setName('');
      setPrompt('');
      setShowCreate(false);
      await reload();
    } catch (err) {
      setErr(err instanceof Error ? err.message : String(err));
    } finally {
      setBusyId(null);
    }
  }

  async function act(id: string, fn: () => Promise<unknown>) {
    setBusyId(id);
    try {
      await fn();
      await reload();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusyId(null);
    }
  }

  return (
    <div className="studio-cron" aria-label="Cron jobs">
      <div className="studio-cron-head">
        <h4>
          <Clock size={12} /> Cron {loading ? '…' : `(${jobs.length})`}
        </h4>
        <div className="studio-cron-actions">
          <button type="button" className="studio-btn ghost compact" onClick={() => void reload()}>
            <RefreshCcw size={12} />
          </button>
          <button
            type="button"
            className="studio-btn ghost compact"
            onClick={() => setShowCreate((v) => !v)}
          >
            <Plus size={12} /> New
          </button>
        </div>
      </div>

      {showCreate && (
        <form className="studio-cron-form" onSubmit={(e) => void onCreate(e)}>
          <input
            className="studio-input"
            placeholder="Name (optional)"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <input
            className="studio-input"
            placeholder="Schedule (e.g. every 1h, 0 9 * * *)"
            value={schedule}
            onChange={(e) => setSchedule(e.target.value)}
            required
          />
          <textarea
            className="studio-input"
            placeholder="Prompt"
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            rows={2}
            required
          />
          <button
            type="submit"
            className="studio-btn compact"
            disabled={busyId === 'create'}
          >
            Create
          </button>
        </form>
      )}

      {jobs.length === 0 && !loading && (
        <div className="studio-empty tiny">No cron jobs</div>
      )}

      <div className="studio-cron-list">
        {jobs.slice(0, 20).map((job) => (
          <div key={job.id} className="studio-cron-row" title={job.prompt}>
            <div className="studio-cron-meta">
              <span className="studio-cron-name">{job.name || job.id}</span>
              <span className="tag">{job.schedule}</span>
              {!job.enabled && <span className="tag">paused</span>}
            </div>
            <div className="studio-cron-ops">
              <button
                type="button"
                className="studio-btn ghost compact"
                title="Run now"
                disabled={busyId === job.id}
                onClick={() => void act(job.id, () => api.runCronJobNow(job.id))}
              >
                <Play size={11} />
              </button>
              {job.enabled ? (
                <button
                  type="button"
                  className="studio-btn ghost compact"
                  title="Pause"
                  disabled={busyId === job.id}
                  onClick={() => void act(job.id, () => api.pauseCronJob(job.id))}
                >
                  <Pause size={11} />
                </button>
              ) : (
                <button
                  type="button"
                  className="studio-btn ghost compact"
                  title="Resume"
                  disabled={busyId === job.id}
                  onClick={() => void act(job.id, () => api.resumeCronJob(job.id))}
                >
                  <Play size={11} />
                </button>
              )}
              <button
                type="button"
                className="studio-btn ghost compact danger"
                title="Delete"
                disabled={busyId === job.id}
                onClick={() => {
                  if (Danger.deleteCron(job.name || job.id)) {
                    void act(job.id, () => api.deleteCronJob(job.id));
                  }
                }}
              >
                <Trash2 size={11} />
              </button>
            </div>
          </div>
        ))}
      </div>
      {err && <div className="studio-empty tiny">{err}</div>}
    </div>
  );
}
