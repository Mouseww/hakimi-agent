import { useState } from 'react';
import type { DeskState } from './officeState';
import { displayedState } from './officeState';

interface PersonaDeskMarvisProps {
  desk: DeskState;
  x: number;
  y: number;
  onOpen: (id: string) => void;
  onHover: (id: string | null) => void;
}

// 截取任务提示的前 N 个字符
function truncateTask(task: string | undefined, maxLength: number = 30): string {
  if (!task) return '';
  return task.length > maxLength ? task.substring(0, maxLength) + '...' : task;
}

// Marvis-style status mapping
type MarvisStatus = 'working' | 'busy' | 'planning' | 'away' | 'creative' | 'focused';

const STATUS_COLORS: Record<MarvisStatus, string> = {
  working: '#10b981',   // 绿
  busy: '#ef4444',      // 红
  planning: '#a855f7',  // 紫
  away: '#3b82f6',      // 蓝
  creative: '#f59e0b',  // 黄
  focused: '#06b6d4',   // 青
};

const STATUS_LABELS: Record<MarvisStatus, string> = {
  working: '正常工作',
  busy: '高负载',
  planning: '项目规划',
  away: '离线/休息',
  creative: '创意设计',
  focused: '深度专注',
};

function inferMarvisStatus(desk: DeskState): MarvisStatus {
  const state = displayedState(desk);
  
  // 离线状态
  if (state === 'idle') return 'away';
  
  // 团队协作 -> 规划
  if (state === 'in_team') return 'planning';
  
  // Consulting -> 忙碌
  if (state === 'consulting') return 'busy';
  
  // Working 状态根据任务提示细分
  if (state === 'working') {
    const hint = (desk.taskHint || '').toLowerCase();
    
    if (/plan|规划|看板|kanban|design.*system/.test(hint)) return 'planning';
    if (/creative|创意|design|设计|ui|ux|art/.test(hint)) return 'creative';
    if (/focus|专注|write|writing|文档|doc/.test(hint)) return 'focused';
    if (/busy|忙|overload|高负载|urgent/.test(hint)) return 'busy';
    
    // 默认正常工作
    return 'working';
  }
  
  return 'working';
}

function avatarEmoji(desk: DeskState): string {
  if (desk.avatar.trim()) return desk.avatar.trim().slice(0, 2);
  
  const status = inferMarvisStatus(desk);
  const emojiMap: Record<MarvisStatus, string> = {
    working: '😊',
    busy: '🤓',
    planning: '🤔',
    away: '😴',
    creative: '🎨',
    focused: '🧘',
  };
  
  return emojiMap[status];
}

export default function PersonaDeskMarvis({ desk, x, y, onHover }: Omit<PersonaDeskMarvisProps, 'onOpen'>) {
  const [showModal, setShowModal] = useState(false);
  const status = inferMarvisStatus(desk);
  const statusColor = STATUS_COLORS[status];
  const statusLabel = STATUS_LABELS[status];
  const emoji = avatarEmoji(desk);
  const working = displayedState(desk) === 'working';
  
  return (
    <div
      className="workstation-marvis"
      style={{ left: x, top: y }}
      role="button"
      tabIndex={0}
      onClick={() => setShowModal(true)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          setShowModal(true);
        }
      }}
      onMouseEnter={() => onHover(desk.id)}
      onMouseLeave={() => onHover(null)}
    >
      {/* 状态色条 */}
      <div className="status-bar-marvis" style={{ background: statusColor }} />
      
      <div className="desk-container-marvis">
        {/* 卡片主体 */}
        <div className="desk-marvis">
          {/* 显示器底座 */}
          <div className="monitor-stand" />
          
          {/* 显示器 */}
          <div className="monitor-marvis">
            <div className={`screen-marvis screen-${status}`}>
              {/* 背景装饰（根据状态） */}
              {status === 'working' && (
                <div className="screen-document">
                  <div className="line" />
                  <div className="line" />
                  <div className="line" />
                  <div className="line" />
                </div>
              )}
              
              {status === 'busy' && (
                <div className="screen-code">
                  <div className="code-line">fn main() {'{'}</div>
                  <div className="code-line">  let x = 42;</div>
                  <div className="code-line">  println!("{'{}'}", x);</div>
                  <div className="code-line">{'}'}</div>
                </div>
              )}
              
              {status === 'planning' && (
                <div className="screen-kanban">
                  <div className="kanban-card" />
                  <div className="kanban-card" />
                  <div className="kanban-card" />
                  <div className="kanban-card" />
                </div>
              )}
              
              {status === 'away' && (
                <div className="screen-off">
                  <span className="zzz">💤</span>
                </div>
              )}
              
              {status === 'creative' && (
                <div className="screen-gallery">
                  <div className="gallery-thumb" />
                  <div className="gallery-thumb" />
                  <div className="gallery-thumb" />
                  <div className="gallery-thumb" />
                </div>
              )}
              
              {status === 'focused' && (
                <div className="screen-document">
                  <div className="line" />
                  <div className="line" />
                  <div className="line" />
                  <div className="line" />
                </div>
              )}
              
              {/* 任务文字叠加层 */}
              {desk.taskHint && status !== 'away' && (
                <div className="task-overlay">
                  <div className="task-text">{truncateTask(desk.taskHint, 35)}</div>
                </div>
              )}
            </div>
          </div>
          
          {/* 椅子 */}
          <div className="chair-marvis" />
          
          {/* 猫咪角色 */}
          <div className={`person-marvis ${working ? 'working' : ''}`}>
            <svg viewBox="0 0 60 72" width="60" height="72">
              {/* 猫耳朵（左） */}
              <path 
                d="M 12 16 L 8 6 L 18 10 Z" 
                fill="#fef3e2" 
                stroke="#c98a2b" 
                strokeWidth="1.5"
              />
              {/* 猫耳朵（右） */}
              <path 
                d="M 48 16 L 52 6 L 42 10 Z" 
                fill="#fef3e2" 
                stroke="#c98a2b" 
                strokeWidth="1.5"
              />
              
              {/* 猫头 - 圆润方块 */}
              <rect 
                x="14" y="12" width="32" height="28" rx="8" 
                fill="#fef3e2" 
                stroke="#c98a2b" 
                strokeWidth="2" 
              />
              
              {/* 猫眼睛（左） */}
              <ellipse cx="23" cy="24" rx="3" ry="4" fill="#2d3748" />
              <ellipse cx="23" cy="23" rx="1.5" ry="2" fill="#ffffff" opacity="0.6" />
              
              {/* 猫眼睛（右） */}
              <ellipse cx="37" cy="24" rx="3" ry="4" fill="#2d3748" />
              <ellipse cx="37" cy="23" rx="1.5" ry="2" fill="#ffffff" opacity="0.6" />
              
              {/* 猫鼻子 */}
              <path 
                d="M 30 28 L 28 32 L 32 32 Z" 
                fill="#ff8fb1" 
              />
              
              {/* 猫胡须（左侧） */}
              <line x1="12" y1="26" x2="4" y2="24" stroke="#c98a2b" strokeWidth="1" opacity="0.6" />
              <line x1="12" y1="30" x2="4" y2="30" stroke="#c98a2b" strokeWidth="1" opacity="0.6" />
              <line x1="12" y1="34" x2="4" y2="36" stroke="#c98a2b" strokeWidth="1" opacity="0.6" />
              
              {/* 猫胡须（右侧） */}
              <line x1="48" y1="26" x2="56" y2="24" stroke="#c98a2b" strokeWidth="1" opacity="0.6" />
              <line x1="48" y1="30" x2="56" y2="30" stroke="#c98a2b" strokeWidth="1" opacity="0.6" />
              <line x1="48" y1="34" x2="56" y2="36" stroke="#c98a2b" strokeWidth="1" opacity="0.6" />
              
              {/* 猫嘴巴 */}
              <path 
                d="M 25 34 Q 30 36 35 34" 
                stroke="#c98a2b" 
                strokeWidth="1.5" 
                fill="none"
                strokeLinecap="round"
              />
              
              {/* 猫身体（穿工作服） */}
              <rect 
                x="16" y="42" width="28" height="18" rx="3"
                fill={statusColor} 
                opacity="0.9" 
                stroke={statusColor} 
                strokeWidth="1.5" 
              />
              
              {/* 工牌/徽章 */}
              <rect 
                x="26" y="48" width="8" height="6" rx="1"
                fill="#ffffff" 
                opacity="0.8"
              />
              <text 
                x="30" y="52.5" 
                textAnchor="middle" 
                fontSize="4" 
                fill={statusColor}
                fontWeight="bold"
              >
                {emoji}
              </text>
              
              {/* 猫前爪（左） */}
              <g className={working ? 'typing-arms' : ''}>
                <rect x="8" y="50" width="7" height="12" rx="2" fill="#fef3e2" stroke="#c98a2b" strokeWidth="1">
                  {working && <animate attributeName="y" values="50;52;50" dur="0.6s" repeatCount="indefinite" />}
                </rect>
              </g>
              
              {/* 猫前爪（右） */}
              <g className={working ? 'typing-arms' : ''}>
                <rect x="45" y="50" width="7" height="12" rx="2" fill="#fef3e2" stroke="#c98a2b" strokeWidth="1">
                  {working && <animate attributeName="y" values="50;52;50" dur="0.6s" repeatCount="indefinite" begin="0.3s" />}
                </rect>
              </g>
              
              {/* 猫尾巴 */}
              <path 
                d="M 44 54 Q 52 58 54 64 Q 56 68 52 70" 
                stroke={statusColor}
                strokeWidth="4"
                fill="none"
                strokeLinecap="round"
                opacity="0.8"
              >
                {working && (
                  <animateTransform
                    attributeName="transform"
                    type="rotate"
                    values="0 44 54; 5 44 54; 0 44 54; -5 44 54; 0 44 54"
                    dur="2s"
                    repeatCount="indefinite"
                  />
                )}
              </path>
            </svg>
          </div>
        </div>
        
        {/* 阴影 */}
        <div className="shadow-marvis" />
      </div>
      
      {/* 标签 */}
      <div className="desk-label-marvis">
        <div className="desk-name-marvis">{desk.name || desk.id}</div>
        <div className="desk-state-marvis">{statusLabel}</div>
      </div>
      
      {/* Tooltip */}
      <div className="tooltip-marvis">
        <strong>{desk.name || desk.id}</strong><br />
        状态: {statusLabel}<br />
        {desk.taskHint && `任务: ${desk.taskHint}`}
        {desk.model && <><br />模型: {desk.model}</>}
      </div>
      
      {/* 任务详情模态框 */}
      {showModal && (
        <div 
          className="task-modal-overlay" 
          onClick={() => setShowModal(false)}
        >
          <div 
            className="task-modal-content"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="task-modal-header">
              <h3>{emoji} {desk.name || desk.id}</h3>
              <button 
                className="task-modal-close"
                onClick={() => setShowModal(false)}
              >
                ×
              </button>
            </div>
            
            <div className="task-modal-body">
              <div className="task-modal-section">
                <label>状态</label>
                <div className="task-modal-status" style={{ color: statusColor }}>
                  {statusLabel}
                </div>
              </div>
              
              {desk.taskHint && (
                <div className="task-modal-section">
                  <label>当前任务</label>
                  <div className="task-modal-task">{desk.taskHint}</div>
                </div>
              )}
              
              {desk.model && (
                <div className="task-modal-section">
                  <label>模型</label>
                  <div>{desk.model}</div>
                </div>
              )}
              
              {desk.consultingTo && (
                <div className="task-modal-section">
                  <label>协作对象</label>
                  <div>正在咨询: {desk.consultingTo}</div>
                </div>
              )}
              
              {desk.delegatedFrom && (
                <div className="task-modal-section">
                  <label>委托来源</label>
                  <div>来自: {desk.delegatedFrom}</div>
                </div>
              )}
            </div>
            
            <div className="task-modal-footer">
              <button onClick={() => setShowModal(false)}>关闭</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
