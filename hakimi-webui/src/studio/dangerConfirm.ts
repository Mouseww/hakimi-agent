/**
 * Dangerous-operation confirmation helpers for Studio UI.
 * Policy: delete / restore / recursive wipe require explicit confirm.
 */

export type DangerKind =
  | 'workspace_delete'
  | 'checkpoint_restore'
  | 'cron_delete'
  | 'handoff';

export type DangerRequest = {
  kind: DangerKind;
  title: string;
  detail?: string;
  /** When true, user must type the word "DELETE" or the resource name. */
  requireTypedConfirm?: boolean;
  confirmWord?: string;
};

/**
 * Browser confirm with optional typed phrase. Returns true if user accepts.
 */
export function confirmDanger(req: DangerRequest): boolean {
  const body = [
    req.title,
    req.detail ?? '',
    req.requireTypedConfirm
      ? `\nType ${req.confirmWord || 'DELETE'} to confirm.`
      : '\nContinue?',
  ]
    .filter(Boolean)
    .join('\n');

  if (req.requireTypedConfirm) {
    const word = req.confirmWord || 'DELETE';
    const typed = window.prompt(body, '');
    return typed === word;
  }
  return window.confirm(body);
}

/** Prebuilt prompts for common Studio actions. */
export const Danger = {
  deleteFile(path: string): boolean {
    return confirmDanger({
      kind: 'workspace_delete',
      title: `Delete file?`,
      detail: path,
      requireTypedConfirm: false,
    });
  },
  deleteRecursive(path: string): boolean {
    return confirmDanger({
      kind: 'workspace_delete',
      title: `Delete folder recursively?`,
      detail: path,
      requireTypedConfirm: true,
      confirmWord: 'DELETE',
    });
  },
  restoreCheckpoint(id: string): boolean {
    return confirmDanger({
      kind: 'checkpoint_restore',
      title: `Restore checkpoint ${id}?`,
      detail: 'Current files will be overwritten from the snapshot.',
      requireTypedConfirm: true,
      confirmWord: 'RESTORE',
    });
  },
  deleteCron(name: string): boolean {
    return confirmDanger({
      kind: 'cron_delete',
      title: `Delete cron job?`,
      detail: name,
    });
  },
  handoff(toDevice: string): boolean {
    return confirmDanger({
      kind: 'handoff',
      title: `Hand off Active Runner?`,
      detail: `Target device: ${toDevice}`,
    });
  },
};
