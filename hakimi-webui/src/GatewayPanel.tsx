import { Activity, BadgeCheck, Loader2, RefreshCcw, ShieldCheck } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api, type GatewayConfigResponse, type GatewayStatusResponse } from './api';

export default function GatewayPanel() {
  const [status, setStatus] = useState<GatewayStatusResponse | null>(null);
  const [config, setConfig] = useState<GatewayConfigResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);

  const fetchData = async () => {
    setLoading(true);
    setError(null);
    try {
      const [statusRes, configRes] = await Promise.all([
        api.getGatewayStatus(),
        api.getGatewayConfig(),
      ]);
      setStatus(statusRes);
      setConfig(configRes);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load gateway data');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    const timer = window.setTimeout(() => {
      void fetchData();
    }, 0);
    return () => window.clearTimeout(timer);
  }, []);

  const handleSaveConfig = async () => {
    if (!config) return;
    setSaving(true);
    setError(null);
    setSuccessMsg(null);
    try {
      await api.updateGatewayConfig(config);
      setSuccessMsg('配置已保存');
      setTimeout(() => setSuccessMsg(null), 3000);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save config');
    } finally {
      setSaving(false);
    }
  };

  const handleRestart = async () => {
    setRestarting(true);
    setError(null);
    setSuccessMsg(null);
    try {
      await api.restartGateway();
      setSuccessMsg('Gateway 重启请求已发送');
      setTimeout(() => {
        setSuccessMsg(null);
        fetchData();
      }, 2000);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to restart gateway');
    } finally {
      setRestarting(false);
    }
  };

  if (loading) {
    return (
      <div className="gateway-panel-loading">
        <Loader2 className="icon-spin" size={32} />
        <p>加载 Gateway 状态...</p>
      </div>
    );
  }

  return (
    <div className="gateway-panel">
      <div className="gateway-header">
        <h2>Gateway 管理</h2>
        <button className="btn-refresh" onClick={fetchData} disabled={loading}>
          <RefreshCcw size={16} />
          刷新
        </button>
      </div>

      {error && (
        <div className="alert alert-error">
          <Activity size={16} />
          {error}
        </div>
      )}

      {successMsg && (
        <div className="alert alert-success">
          <BadgeCheck size={16} />
          {successMsg}
        </div>
      )}

      {/* Status Section */}
      <section className="gateway-status">
        <h3>运行状态</h3>
        <div className="status-grid">
          <div className="status-item">
            <span className="status-label">Gateway 状态</span>
            <div className="status-value">
              {status?.running ? (
                <>
                  <Activity className="status-icon status-running" size={20} />
                  <span className="status-text status-running">运行中</span>
                </>
              ) : (
                <>
                  <ShieldCheck className="status-icon status-stopped" size={20} />
                  <span className="status-text status-stopped">未运行</span>
                </>
              )}
            </div>
          </div>

          {status?.running && (
            <>
              <div className="status-item">
                <span className="status-label">已连接平台</span>
                <span className="status-value">
                  {status.platforms.length > 0 ? (
                    <div className="platform-list">
                      {status.platforms.map((p, i) => (
                        <div key={i} className="platform-item">
                          <BadgeCheck size={14} className="platform-icon" />
                          <span>{p.platform || p.name}</span>
                          {p.bot_id && <code className="bot-id">{p.bot_id}</code>}
                        </div>
                      ))}
                    </div>
                  ) : (
                    <span className="text-muted">无</span>
                  )}
                </span>
              </div>

              <div className="status-item">
                <span className="status-label">总消息数</span>
                <span className="status-value status-number">
                  {status.total_messages_sent || 0}
                </span>
              </div>
            </>
          )}
        </div>

        {status?.running && (
          <button
            className="btn-restart"
            onClick={handleRestart}
            disabled={restarting}
          >
            {restarting ? (
              <>
                <Loader2 className="icon-spin" size={16} />
                重启中...
              </>
            ) : (
              <>
                <RefreshCcw size={16} />
                重启 Gateway
              </>
            )}
          </button>
        )}
      </section>

      {/* Config Section */}
      {config && (
        <section className="gateway-config">
          <h3>配置管理</h3>
          <div className="config-form">
            <div className="form-group">
              <label htmlFor="busy-mode">繁忙输入模式</label>
              <select
                id="busy-mode"
                value={config.busy_input_mode}
                onChange={(e) =>
                  setConfig({ ...config, busy_input_mode: e.target.value })
                }
              >
                <option value="queue">队列模式 (queue)</option>
                <option value="interrupt">中断模式 (interrupt)</option>
              </select>
              <small className="form-hint">
                队列模式：新消息排队等待。中断模式：新消息取消当前任务。
              </small>
            </div>

            <div className="form-group">
              <label>
                <input
                  type="checkbox"
                  checked={config.allow_all}
                  onChange={(e) =>
                    setConfig({ ...config, allow_all: e.target.checked })
                  }
                />
                允许所有用户访问
              </label>
              <small className="form-hint">
                启用后，所有用户都可以使用 Gateway。禁用后仅限白名单用户。
              </small>
            </div>

            {!config.allow_all && (
              <div className="form-group">
                <label htmlFor="allowed-users">白名单用户</label>
                <textarea
                  id="allowed-users"
                  rows={3}
                  placeholder="每行一个用户 ID 或用户名"
                  value={config.allowed_users.join('\n')}
                  onChange={(e) =>
                    setConfig({
                      ...config,
                      allowed_users: e.target.value
                        .split('\n')
                        .map((s) => s.trim())
                        .filter((s) => s.length > 0),
                    })
                  }
                />
                <small className="form-hint">
                  每行一个用户 ID 或用户名（例如：telegram:123456789）
                </small>
              </div>
            )}

            <div className="form-group">
              <label>
                <input
                  type="checkbox"
                  checked={config.filter_silence_narration}
                  onChange={(e) =>
                    setConfig({
                      ...config,
                      filter_silence_narration: e.target.checked,
                    })
                  }
                />
                过滤叙述性文本
              </label>
              <small className="form-hint">
                移除响应中的 "正在执行..."、"已完成..." 等叙述性内容。
              </small>
            </div>

            <button
              className="btn-save"
              onClick={handleSaveConfig}
              disabled={saving}
            >
              {saving ? (
                <>
                  <Loader2 className="icon-spin" size={16} />
                  保存中...
                </>
              ) : (
                <>
                  <BadgeCheck size={16} />
                  保存配置
                </>
              )}
            </button>
          </div>
        </section>
      )}
    </div>
  );
}
