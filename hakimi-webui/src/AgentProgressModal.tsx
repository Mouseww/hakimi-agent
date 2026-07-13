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
    // 获取 agent 的对话历史
    fetch(`/api/persona/${agentId}/messages`)
      .then(res => res.json())
      .then(data => {
        setMessages(data.messages || []);
        setLoading(false);
      })
      .catch(err => {
        console.error('Failed to load agent messages:', err);
        setLoading(false);
      });
  }, [agentId]);

  const handleBackdropClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) {
      onClose();
    }
  };

  return (
    <div className="agent-progress-modal-backdrop" onClick={handleBackdropClick}>
      <div className="agent-progress-modal">
        <div className="agent-progress-modal-header">
          <h2>{agentName} 工作进度</h2>
          <button className="agent-progress-modal-close" onClick={onClose}>
            ✕
          </button>
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
