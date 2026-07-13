import React, { useState } from 'react';

interface PersonaDeskMarvisV3Props {
  name: string;
  role: string;
  status: 'working' | 'busy' | 'planning' | 'away' | 'creative' | 'focused';
  taskHint?: string;
  onClick?: () => void;
}

const STATUS_CONFIG = {
  working: { color: '#10b981', tailColor: '#34d399', label: '工作中', emoji: '💼' },
  busy: { color: '#ef4444', tailColor: '#f87171', label: '高负载', emoji: '🔥' },
  planning: { color: '#a855f7', tailColor: '#c084fc', label: '项目规划', emoji: '📋' },
  away: { color: '#3b82f6', tailColor: '#60a5fa', label: '离线/休息', emoji: '😴' },
  creative: { color: '#f59e0b', tailColor: '#fbbf24', label: '创意设计', emoji: '🎨' },
  focused: { color: '#06b6d4', tailColor: '#22d3ee', label: '深度专注', emoji: '🎯' },
};

export const PersonaDeskMarvisV3: React.FC<PersonaDeskMarvisV3Props> = ({
  name,
  role,
  status,
  taskHint,
  onClick,
}) => {
  const [showModal, setShowModal] = useState(false);
  const config = STATUS_CONFIG[status];

  const handleClick = () => {
    if (onClick) {
      onClick();
    } else {
      setShowModal(true);
    }
  };

  return (
    <>
      <div
        className="workstation-marvis-v3"
        onClick={handleClick}
        style={{ '--status-color': config.color, '--tail-color': config.tailColor } as React.CSSProperties}
        tabIndex={0}
        role="button"
        aria-label={`${name} - ${role} - ${config.label}`}
      >
        {/* 工位卡片 */}
        <div className="desk-container-marvis-v3">
          <div className="desk-marvis-v3">
            {/* 显示器 */}
            <div className="monitor-marvis-v3">
              <div className="screen-marvis-v3">
                {status !== 'away' && taskHint && (
                  <div className="task-overlay-v3">
                    <div className="task-text-v3">{taskHint}</div>
                  </div>
                )}
                {status === 'away' && (
                  <div className="task-overlay-v3">
                    <div className="task-text-v3" style={{ color: '#64748b' }}>😴 Zzz...</div>
                  </div>
                )}
              </div>
            </div>

            {/* 显示器支架 */}
            <div className="monitor-stand-v3" />

            {/* 猫咪侧面剪影 SVG - 优化版 */}
            <svg
              className="cat-silhouette-marvis-v3"
              viewBox="0 0 90 90"
              fill="none"
              xmlns="http://www.w3.org/2000/svg"
            >
              <g className="cat-body">
                {/* 左耳 - 更尖锐 */}
                <path
                  d="M 34 20 L 30 10 L 38 16 Z"
                  fill="#8b9bb0"
                />
                {/* 右耳 - 更尖锐 */}
                <path
                  d="M 48 20 L 44 8 L 52 14 Z"
                  fill="#8b9bb0"
                />
                {/* 头部 - 略微调整 */}
                <ellipse cx="41" cy="28" rx="12" ry="11" fill="#94a3b8" />
                {/* 吻部 - 更明显 */}
                <path
                  d="M 52 28 Q 57 28 57 30 Q 57 32 52 32 L 50 30 Z"
                  fill="#94a3b8"
                />
                {/* 鼻尖（小黑点） */}
                <circle cx="56" cy="30" r="1.5" fill="#334155" />
                
                {/* 身体 */}
                <ellipse cx="41" cy="52" rx="15" ry="20" fill="#94a3b8" />
                
                {/* 围脖/领带（状态色） */}
                <ellipse
                  cx="41"
                  cy="40"
                  rx="10"
                  ry="5"
                  fill={config.color}
                  className="status-scarf-v3"
                />
                
                {/* 前肢（打字姿势） */}
                <rect x="34" y="68" width="4" height="11" rx="2" fill="#8b9bb0" className="cat-arm-left" />
                <rect x="43" y="68" width="4" height="11" rx="2" fill="#8b9bb0" className="cat-arm-right" />
                
                {/* 尾巴（独立颜色） */}
                <path
                  d="M 54 58 Q 64 62 66 50 Q 67 43 64 38"
                  stroke={config.tailColor}
                  strokeWidth="4.5"
                  strokeLinecap="round"
                  fill="none"
                  className="cat-tail"
                />
              </g>
            </svg>

            {/* 阴影 */}
            <div className="shadow-marvis-v3" />
          </div>
        </div>

        {/* 工位标签 */}
        <div className="desk-label-v3">
          <div className="desk-name-v3">{name}</div>
          <div className="desk-status-v3">
            <span>{config.emoji}</span> {config.label}
          </div>
        </div>
      </div>

      {/* 任务详情模态框 */}
      {showModal && (
        <div className="task-modal-overlay-v3" onClick={() => setShowModal(false)}>
          <div className="task-modal-content-v3" onClick={(e) => e.stopPropagation()}>
            <div className="task-modal-header-v3">
              <div className="task-modal-title-v3">
                <span className="task-modal-emoji-v3">{config.emoji}</span>
                <span className="task-modal-name-v3">{name}</span>
              </div>
              <button className="task-modal-close-v3" onClick={() => setShowModal(false)}>
                ✕
              </button>
            </div>
            <div className="task-modal-body-v3">
              <div className="task-info-row-v3">
                <div className="task-info-label-v3">角色</div>
                <div className="task-info-value-v3">{role}</div>
              </div>
              <div className="task-info-row-v3">
                <div className="task-info-label-v3">状态</div>
                <div className="status-badge-v3" style={{ background: `${config.color}22`, borderColor: config.color }}>
                  <div className="status-dot-v3" style={{ background: config.color }} />
                  <span>{config.label}</span>
                </div>
              </div>
              {taskHint && (
                <div className="task-info-row-v3">
                  <div className="task-info-label-v3">当前任务</div>
                  <div className="task-info-value-v3">{taskHint}</div>
                </div>
              )}
            </div>
            <div className="task-modal-footer-v3">
              <button className="task-modal-button-v3" onClick={() => setShowModal(false)}>
                关闭
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
};
