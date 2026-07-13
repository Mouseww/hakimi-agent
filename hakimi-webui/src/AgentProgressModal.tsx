import { useEffect, useState } from 'react';
import './agent-progress-modal.css';

interface Message {
  role: 'user' | 'assistant' | 'tool';
  content: string;
  timestamp?: number;
}

interface AgentProgressModalProps {
  agentId: string;
  agentName: string;
  onClose: () => void;
}

export function AgentProgressModal({ agentId, agentName, onClose }: AgentProgressModalProps) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    // TODO: 实现 persona 工作记录持久化和查询API
    // 暂时显示占位信息
    setMessages([]);
    setLoading(false);
  }, [agentId]);

  const handleBackdropClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) {
      onClose();
    }
  };

  const handleClearAll = () => {
    if (confirm(`确定要清空 ${agentName} 的所有工作记录吗？`)) {
      // TODO: 调用 DELETE /api/persona/:id/messages
      setMessages([]);
    }
  };

  return (
    <div className="agent-progress-modal-backdrop" onClick={handleBackdropClick}>
      <div className="agent-progress-modal">
        <div className="agent-progress-modal-header">
          <h2>{agentName} 工作进度</h2>
          <div className="agent-progress-modal-actions">
            {messages.length > 0 && (
              <button className="agent-progress-btn-clear" onClick={handleClearAll} title="清空所有记录">
                🗑️ 清空
              </button>
            )}
            <button className="agent-progress-modal-close" onClick={onClose}>
              ✕
            </button>
          </div>
        </div>
        <div className="agent-progress-modal-body">
          {loading ? (
            <div className="agent-progress-loading">加载中...</div>
          ) : messages.length === 0 ? (
            <div className="agent-progress-empty">暂无工作记录</div>
          ) : (
            <div className="agent-progress-messages">
              {messages.map((msg, i) => (
                <div key={i} className={`agent-progress-message agent-progress-message-${msg.role}`}>
                  <div className="agent-progress-message-role">
                    {msg.role === 'user' ? '👤 用户' : msg.role === 'assistant' ? '🤖 助手' : '🔧 工具'}
                  </div>
                  <div className="agent-progress-message-content">
                    {msg.content.length > 500 ? msg.content.slice(0, 500) + '...' : msg.content}
                  </div>
                  {msg.timestamp && (
                    <div className="agent-progress-message-time">
                      {new Date(msg.timestamp).toLocaleTimeString('zh-CN')}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
