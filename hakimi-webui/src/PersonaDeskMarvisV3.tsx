import React, { useState } from 'react';
import { MonitorScreenContent } from './MonitorScreenContent';
import type { TodoItem } from './types/todo';
import type { ActiveToolCall } from './types/toolCall';

interface PersonaDeskMarvisV3Props {
  name: string;
  role: string;
  status: 'working' | 'busy' | 'planning' | 'away' | 'creative' | 'focused';
  taskHint?: string;
  onClick?: () => void;
  scarfColor?: string;  // 独立围脖颜色
  tailColor?: string;   // 独立尾巴颜色
  hideCat?: boolean;    // 隐藏猫咪（行走时）
  todos?: TodoItem[];   // 任务追踪列表
  activeToolCall?: ActiveToolCall | null;  // 当前执行的工具
}

const STATUS_CONFIG = {
  working: { label: '工作中', emoji: '💼' },
  busy: { label: '高负载', emoji: '🔥' },
  planning: { label: '项目规划', emoji: '📋' },
  away: { label: '离线/休息', emoji: '😴' },
  creative: { label: '创意设计', emoji: '🎨' },
  focused: { label: '深度专注', emoji: '🎯' },
};

export const PersonaDeskMarvisV3: React.FC<PersonaDeskMarvisV3Props> = ({
  name,
  role,
  status,
  taskHint,
  onClick,
  scarfColor = '#10b981',
  tailColor = '#34d399',
  hideCat = false,
  todos,
  activeToolCall,
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
        style={{ '--scarf-color': scarfColor, '--tail-color': tailColor } as React.CSSProperties}
        tabIndex={0}
        role="button"
        aria-label={`${name} - ${role} - ${config.label}`}
      >
        {/* 工位卡片 */}
        <div className="desk-container-marvis-v3">
          <div className="desk-marvis-v3">
            
            {/* 电脑桌 */}
            <div className="computer-desk-v3" />
            
            {/* 显示器（缩小） */}
            <div className="monitor-marvis-v3">
              <div className="screen-marvis-v3">
                <MonitorScreenContent status={status} taskHint={taskHint} todos={todos} activeToolCall={activeToolCall} />
              </div>
            </div>

            {/* 显示器支架 */}
            <div className="monitor-stand-v3" />

            {/* 椅子（无靠背） */}
            <div className="chair-v3">
              <div className="chair-seat-v3" />
            </div>

            {/* 猫咪侧面剪影 SVG - 坐在椅子上（行走时隐藏） */}
            {!hideCat && (
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
                
                {/* 身体（加宽） */}
                <ellipse cx="41" cy="52" rx="17" ry="20" fill="#94a3b8" />
                
                {/* 围脖/领带（独立颜色，向上提） */}
                <ellipse
                  cx="41"
                  cy="35"
                  rx="10"
                  ry="5"
                  fill={scarfColor}
                  className="status-scarf-v3"
                />
                
                {/* 前肢（打字姿势） */}
                <rect x="34" y="68" width="4" height="11" rx="2" fill="#8b9bb0" className="cat-arm-left" />
                <rect x="43" y="68" width="4" height="11" rx="2" fill="#8b9bb0" className="cat-arm-right" />
                
                {/* 尾巴（独立颜色） */}
                <path
                  d="M 54 58 Q 64 62 66 50 Q 67 43 64 38"
                  stroke={tailColor}
                  strokeWidth="4.5"
                  strokeLinecap="round"
                  fill="none"
                  className="cat-tail"
                />
              </g>
            </svg>
            )}

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
                <div className="status-badge-v3" style={{ background: `${scarfColor}22`, borderColor: scarfColor }}>
                  <div className="status-dot-v3" style={{ background: scarfColor }} />
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
