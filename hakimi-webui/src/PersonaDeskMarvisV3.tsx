import React, { useState } from 'react';

interface PersonaDeskMarvisV3Props {
  name: string;
  role: string;
  status: 'working' | 'busy' | 'planning' | 'away' | 'creative' | 'focused';
  taskHint?: string;
  onClick?: () => void;
}

const STATUS_CONFIG = {
  working: { color: '#10b981', label: '工作中', emoji: '💼' },
  busy: { color: '#ef4444', label: '高负载', emoji: '🔥' },
  planning: { color: '#a855f7', label: '项目规划', emoji: '📋' },
  away: { color: '#3b82f6', label: '离线/休息', emoji: '😴' },
  creative: { color: '#f59e0b', label: '创意设计', emoji: '🎨' },
  focused: { color: '#06b6d4', label: '深度专注', emoji: '🎯' },
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
        style={{ '--status-color': config.color } as React.CSSProperties}
        tabIndex={0}
        role="button"
        aria-label={`${name} - ${role} - ${config.label}`}
      >
        {/* 状态条 */}
        <div className="status-bar-marvis-v3" style={{ background: config.color }} />

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

            {/* 猫咪侧面剪影 SVG */}
            <svg
              className="cat-silhouette-marvis-v3"
              viewBox="0 0 80 80"
              fill="none"
              xmlns="http://www.w3.org/2000/svg"
            >
              {/* 猫咪侧面剪影 - 灰白色（与黑色屏幕形成对比） */}
              <g className="cat-body">
                {/* 左耳 */}
                <path
                  d="M 32 18 L 28 10 L 36 14 Z"
                  fill="#94a3b8"
                />
                {/* 右耳 */}
                <path
                  d="M 44 18 L 40 8 L 48 12 Z"
                  fill="#94a3b8"
                />
                {/* 头部 */}
                <ellipse cx="38" cy="26" rx="11" ry="10" fill="#94a3b8" />
                {/* 吻部 */}
                <path
                  d="M 48 26 Q 52 26 52 28 Q 52 30 48 30 L 46 28 Z"
                  fill="#94a3b8"
                />
                {/* 身体 */}
                <ellipse cx="38" cy="48" rx="14" ry="18" fill="#94a3b8" />
                {/* 彩色状态条 */}
                <rect
                  x="28"
                  y="44"
                  width="20"
                  height="8"
                  rx="2"
                  fill={config.color}
                  className="status-badge-v3"
                />
                {/* 前肢（打字姿势） */}
                <rect x="32" y="62" width="4" height="10" rx="2" fill="#94a3b8" className="cat-arm-left" />
                <rect x="40" y="62" width="4" height="10" rx="2" fill="#94a3b8" className="cat-arm-right" />
                {/* 尾巴 */}
                <path
                  d="M 50 55 Q 58 58 60 48 Q 61 42 58 38"
                  stroke="#94a3b8"
                  strokeWidth="4"
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
          <div className="desk-role-v3">{role}</div>
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
