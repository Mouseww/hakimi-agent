import { useEffect, useState } from 'react';

// API Types
interface SanitizedConfig {
  model_default: string;
  model_provider: string;
  agent_max_turns: number;
  agent_verbose: boolean;
  agent_system_prompt: string;
  terminal_cwd: string;
  terminal_timeout: number;
  compression_engine: string;
  compression_context_length: number;
}

interface ConfigUpdate {
  model_default?: string;
  model_provider?: string;
  agent_max_turns?: number;
  agent_verbose?: boolean;
  agent_system_prompt?: string;
  terminal_cwd?: string;
  terminal_timeout?: number;
  compression_engine?: string;
  compression_context_length?: number;
}

export default function SettingsPanel() {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState<{ text: string, type: 'success' | 'error' } | null>(null);

  // Form state
  const [formData, setFormData] = useState<Partial<SanitizedConfig>>({});

  useEffect(() => {
    const fetchConfig = async () => {
      try {
        setLoading(true);
        const res = await fetch('/api/config');
        if (!res.ok) throw new Error('Failed to fetch config');
        const data: SanitizedConfig = await res.json();
        setFormData(data);
      } catch (err) {
        setMessage({ text: String(err), type: 'error' });
      } finally {
        setLoading(false);
      }
    };

    void fetchConfig();
  }, []);

  const handleSave = async () => {
    try {
      setSaving(true);
      setMessage(null);

      // Prepare payload (only send changed values or just send everything that is mapped)
      const payload: ConfigUpdate = {
        model_default: formData.model_default,
        model_provider: formData.model_provider,
        agent_max_turns: formData.agent_max_turns ? Number(formData.agent_max_turns) : undefined,
        agent_verbose: formData.agent_verbose,
        agent_system_prompt: formData.agent_system_prompt,
        terminal_cwd: formData.terminal_cwd,
        terminal_timeout: formData.terminal_timeout ? Number(formData.terminal_timeout) : undefined,
        compression_engine: formData.compression_engine,
        compression_context_length: formData.compression_context_length ? Number(formData.compression_context_length) : undefined,
      };

      const res = await fetch('/api/config', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      });

      if (!res.ok) {
        const errData = await res.json();
        throw new Error(errData.error || 'Failed to save config');
      }

      const updatedData = await res.json();
      setFormData(updatedData);
      setMessage({ text: 'Settings saved successfully! 🐾', type: 'success' });
      
      setTimeout(() => setMessage(null), 3000);
    } catch (err) {
      setMessage({ text: String(err), type: 'error' });
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return <div className="p-8 text-center text-secondary">Loading settings... 🐾</div>;
  }

  return (
    <div className="max-w-4xl mx-auto p-6 space-y-8">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-3xl font-bold text-foreground">Hakimi Settings</h1>
          <p className="text-secondary mt-2">Manage your agent's behavior, models, and capabilities.</p>
        </div>
        <button
          onClick={handleSave}
          disabled={saving}
          className="bg-primary hover:bg-blue-600 text-white px-6 py-2 rounded-lg font-medium transition-colors disabled:opacity-50"
        >
          {saving ? 'Saving...' : 'Save Changes'}
        </button>
      </div>

      {message && (
        <div className={`p-4 rounded-lg ${message.type === 'success' ? 'bg-green-50 text-green-700 border border-green-200' : 'bg-red-50 text-red-700 border border-red-200'}`}>
          {message.text}
        </div>
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* Model Settings */}
        <section className="bg-white border border-border rounded-xl p-6 shadow-sm">
          <h2 className="text-xl font-semibold mb-4 flex items-center gap-2">
            <span className="text-2xl">🤖</span> Model & Provider
          </h2>
          
          <div className="space-y-4">
            <div>
              <label className="block text-sm font-medium text-foreground mb-1">Provider</label>
              <select
                className="w-full border border-border rounded-md px-3 py-2 bg-background text-foreground"
                value={formData.model_provider || 'auto'}
                onChange={(e) => setFormData({ ...formData, model_provider: e.target.value })}
              >
                <option value="auto">Auto Detect</option>
                <option value="openai">OpenAI</option>
                <option value="anthropic">Anthropic</option>
                <option value="openrouter">OpenRouter</option>
              </select>
            </div>
            
            <div>
              <label className="block text-sm font-medium text-foreground mb-1">Default Model</label>
              <input
                type="text"
                className="w-full border border-border rounded-md px-3 py-2"
                placeholder="e.g. claude-3-5-sonnet-20241022"
                value={formData.model_default || ''}
                onChange={(e) => setFormData({ ...formData, model_default: e.target.value })}
              />
            </div>
          </div>
        </section>

        {/* Behavior Settings */}
        <section className="bg-white border border-border rounded-xl p-6 shadow-sm">
          <h2 className="text-xl font-semibold mb-4 flex items-center gap-2">
            <span className="text-2xl">🧠</span> Agent Behavior
          </h2>
          
          <div className="space-y-4">
            <div>
              <label className="block text-sm font-medium text-foreground mb-1">Max Turns</label>
              <input
                type="number"
                className="w-full border border-border rounded-md px-3 py-2"
                value={formData.agent_max_turns || 30}
                onChange={(e) => setFormData({ ...formData, agent_max_turns: Number(e.target.value) })}
              />
              <p className="text-xs text-secondary mt-1">Maximum tool-calling iterations before stopping.</p>
            </div>

            <div className="flex items-center gap-3 mt-4">
              <input
                type="checkbox"
                id="verbose"
                className="w-4 h-4 text-primary rounded border-border"
                checked={formData.agent_verbose || false}
                onChange={(e) => setFormData({ ...formData, agent_verbose: e.target.checked })}
              />
              <label htmlFor="verbose" className="text-sm font-medium text-foreground">
                Enable Verbose Mode (Debug Logs)
              </label>
            </div>
          </div>
        </section>

        {/* System Prompt (Full Width) */}
        <section className="bg-white border border-border rounded-xl p-6 shadow-sm md:col-span-2">
          <h2 className="text-xl font-semibold mb-4 flex items-center gap-2">
            <span className="text-2xl">🎭</span> System Prompt
          </h2>
          <div>
            <textarea
              className="w-full border border-border rounded-md px-3 py-2 font-mono text-sm min-h-[150px]"
              placeholder="You are Hakimi, a powerful AI assistant..."
              value={formData.agent_system_prompt || ''}
              onChange={(e) => setFormData({ ...formData, agent_system_prompt: e.target.value })}
            />
            <p className="text-xs text-secondary mt-1">Define the agent's personality and core instructions.</p>
          </div>
        </section>

        {/* Terminal Settings */}
        <section className="bg-white border border-border rounded-xl p-6 shadow-sm">
          <h2 className="text-xl font-semibold mb-4 flex items-center gap-2">
            <span className="text-2xl">💻</span> Terminal Environment
          </h2>
          
          <div className="space-y-4">
            <div>
              <label className="block text-sm font-medium text-foreground mb-1">Working Directory (CWD)</label>
              <input
                type="text"
                className="w-full border border-border rounded-md px-3 py-2"
                placeholder="/path/to/workdir"
                value={formData.terminal_cwd || ''}
                onChange={(e) => setFormData({ ...formData, terminal_cwd: e.target.value })}
              />
            </div>

            <div>
              <label className="block text-sm font-medium text-foreground mb-1">Timeout (seconds)</label>
              <input
                type="number"
                className="w-full border border-border rounded-md px-3 py-2"
                value={formData.terminal_timeout || 60}
                onChange={(e) => setFormData({ ...formData, terminal_timeout: Number(e.target.value) })}
              />
            </div>
          </div>
        </section>

        {/* Context Compression */}
        <section className="bg-white border border-border rounded-xl p-6 shadow-sm">
          <h2 className="text-xl font-semibold mb-4 flex items-center gap-2">
            <span className="text-2xl">🗜️</span> Memory & Compression
          </h2>
          
          <div className="space-y-4">
            <div>
              <label className="block text-sm font-medium text-foreground mb-1">Compression Engine</label>
              <select
                className="w-full border border-border rounded-md px-3 py-2 bg-background"
                value={formData.compression_engine || 'simple'}
                onChange={(e) => setFormData({ ...formData, compression_engine: e.target.value })}
              >
                <option value="simple">Simple (Truncation)</option>
                <option value="smart">Smart (LLM Summarization)</option>
              </select>
            </div>

            <div>
              <label className="block text-sm font-medium text-foreground mb-1">Context Length (Tokens)</label>
              <input
                type="number"
                className="w-full border border-border rounded-md px-3 py-2"
                value={formData.compression_context_length || 128000}
                onChange={(e) => setFormData({ ...formData, compression_context_length: Number(e.target.value) })}
              />
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}