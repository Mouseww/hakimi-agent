import { ChevronRight, FileText, Folder, Loader2, RefreshCcw } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { api, type WorkspaceEntry } from './api';
import hljs from './highlighter';
import { useI18n } from './i18n';

const EXT_LANG: Record<string, string> = {
  ts: 'typescript',
  tsx: 'typescript',
  js: 'javascript',
  jsx: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  py: 'python',
  rs: 'rust',
  json: 'json',
  md: 'markdown',
  markdown: 'markdown',
  css: 'css',
  scss: 'scss',
  html: 'xml',
  htm: 'xml',
  xml: 'xml',
  svg: 'xml',
  sh: 'bash',
  bash: 'bash',
  zsh: 'bash',
  yml: 'yaml',
  yaml: 'yaml',
  toml: 'ini',
  ini: 'ini',
  sql: 'sql',
  go: 'go',
  java: 'java',
  kt: 'kotlin',
  c: 'c',
  h: 'cpp',
  cpp: 'cpp',
  cc: 'cpp',
  hpp: 'cpp',
  rb: 'ruby',
  php: 'php',
  swift: 'swift',
  lua: 'lua',
  r: 'r',
  diff: 'diff',
  patch: 'diff',
};

function detectLanguage(name: string): string | null {
  const ext = name.split('.').pop()?.toLowerCase() ?? '';
  return EXT_LANG[ext] ?? null;
}

function joinPath(dir: string, name: string): string {
  return dir ? `${dir}/${name}` : name;
}

/**
 * Read-only workspace file browser + code viewer. Lists the agent's workspace
 * directory (`/api/workspace/list`), reads files (`/api/workspace/read`), and
 * renders content with highlight.js syntax highlighting.
 */
export default function WorkspacePanel() {
  const { t } = useI18n();
  const [path, setPath] = useState('');
  const [entries, setEntries] = useState<WorkspaceEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [content, setContent] = useState('');
  const [fileLoading, setFileLoading] = useState(false);

  async function loadDir(target: string) {
    setLoading(true);
    setError(null);
    try {
      const res = await api.workspaceList(target);
      setEntries(res.entries);
      setPath(target);
    } catch (loadError) {
      setError(loadError instanceof Error ? loadError.message : String(loadError));
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void loadDir('');
    }, 0);
    return () => window.clearTimeout(timer);
  }, []);

  async function openFile(name: string) {
    const filePath = joinPath(path, name);
    setSelectedFile(filePath);
    setFileLoading(true);
    setError(null);
    try {
      const res = await api.workspaceRead(filePath);
      setContent(res.content);
    } catch (readError) {
      setError(readError instanceof Error ? readError.message : String(readError));
      setContent('');
    } finally {
      setFileLoading(false);
    }
  }

  const segments = useMemo(() => (path ? path.split('/') : []), [path]);

  // Highlight off the main thread of layout by memoizing; skip very large files.
  const highlighted = useMemo(() => {
    if (!selectedFile || content.length > 300_000) {
      return '';
    }
    const lang = detectLanguage(selectedFile);
    try {
      if (lang && hljs.getLanguage(lang)) {
        return hljs.highlight(content, { language: lang }).value;
      }
      return hljs.highlightAuto(content).value;
    } catch {
      return '';
    }
  }, [content, selectedFile]);

  return (
    <div className="workspace-view">
      <div className="workspace-bar">
        <nav className="workspace-breadcrumb" aria-label="Path">
          <button type="button" onClick={() => void loadDir('')}>
            {t('ws.root')}
          </button>
          {segments.map((segment, index) => (
            <span key={`${segment}-${index}`}>
              <ChevronRight size={13} aria-hidden="true" />
              <button
                type="button"
                onClick={() => void loadDir(segments.slice(0, index + 1).join('/'))}
              >
                {segment}
              </button>
            </span>
          ))}
        </nav>
        <button
          className="icon-button"
          type="button"
          title={t('common.refresh')}
          onClick={() => void loadDir(path)}
        >
          {loading ? <Loader2 className="spin" size={15} /> : <RefreshCcw size={15} />}
        </button>
      </div>

      {error && <div className="notice notice-error">{error}</div>}

      <div className="workspace-body">
        <div className="workspace-tree">
          {!loading && entries.length === 0 && (
            <div className="panel-empty">{t('ws.empty')}</div>
          )}
          {entries.map((entry) => {
            const active = !entry.is_dir && selectedFile === joinPath(path, entry.name);
            return (
              <button
                type="button"
                key={entry.name}
                className={`workspace-entry ${active ? 'is-active' : ''}`}
                onClick={() => {
                  if (entry.is_dir) {
                    void loadDir(joinPath(path, entry.name));
                  } else {
                    void openFile(entry.name);
                  }
                }}
              >
                {entry.is_dir ? (
                  <Folder size={15} aria-hidden="true" />
                ) : (
                  <FileText size={15} aria-hidden="true" />
                )}
                <span className="workspace-entry-name">{entry.name}</span>
                {entry.git_status && <span className="workspace-git">{entry.git_status}</span>}
              </button>
            );
          })}
        </div>

        <div className="workspace-viewer">
          {selectedFile ? (
            fileLoading ? (
              <div className="panel-empty">
                <Loader2 className="spin" size={18} aria-hidden="true" /> {t('ws.loadingFile')}
              </div>
            ) : (
              <>
                <div className="workspace-file-head">{selectedFile}</div>
                <pre className="workspace-code">
                  {highlighted ? (
                    <code className="hljs" dangerouslySetInnerHTML={{ __html: highlighted }} />
                  ) : (
                    <code className="hljs">{content}</code>
                  )}
                </pre>
              </>
            )
          ) : (
            <div className="empty-transcript">
              <FileText size={30} aria-hidden="true" />
              <h3>{t('ws.title')}</h3>
              <p>{t('ws.selectFile')}</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
