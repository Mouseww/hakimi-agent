/**
 * Checkpoint panel — list / create / restore (Phase 5 rewind primitive).
 */
import { History, Plus, RefreshCcw, RotateCcw } from 'lucide-react';
import type { CheckpointView } from './studioProtocol';
import { Danger } from './dangerConfirm';

type Props = {
  open: boolean;
  checkpoints: CheckpointView[];
  canMutate: boolean;
  onRefresh: () => void;
  onCreate: (label?: string) => void;
  onRestore: (id: string) => void;
};

export default function StudioCheckpointPanel({
  open,
  checkpoints,
  canMutate,
  onRefresh,
  onCreate,
  onRestore,
}: Props) {
  if (!open) return null;

  return (
    <div className="studio-cp-panel" aria-label="Checkpoints">
      <div className="studio-cp-head">
        <h4>
          <History size={12} /> Checkpoints ({checkpoints.length})
        </h4>
        <div className="studio-cp-ops">
          <button
            type="button"
            className="studio-btn ghost compact"
            onClick={onRefresh}
            title="Refresh"
          >
            <RefreshCcw size={12} />
          </button>
          <button
            type="button"
            className="studio-btn ghost compact"
            disabled={!canMutate}
            onClick={() => {
              const label = window.prompt('Checkpoint label (optional)', '') ?? undefined;
              onCreate(label || undefined);
            }}
            title="Create checkpoint (current file or top-level)"
          >
            <Plus size={12} />
          </button>
        </div>
      </div>
      <div className="studio-cp-list">
        {checkpoints.length === 0 && (
          <div className="studio-empty tiny">No checkpoints yet</div>
        )}
        {checkpoints.slice(0, 24).map((cp) => (
          <div key={cp.id} className="studio-cp-row" title={cp.path}>
            <div className="studio-cp-meta">
              <span className="studio-cp-id mono">{cp.id}</span>
              {cp.label ? <span className="tag">{cp.label}</span> : null}
              <span className="studio-cp-files">{cp.files?.length ?? 0}f</span>
            </div>
            <button
              type="button"
              className="studio-btn ghost compact"
              disabled={!canMutate}
              title="Restore"
              onClick={() => {
                if (Danger.restoreCheckpoint(cp.id)) {
                  onRestore(cp.id);
                }
              }}
            >
              <RotateCcw size={12} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
