import type { DeskState } from './officeState';
import { displayedState } from './officeState';

interface PersonaDeskMarvisProps {
  desk: DeskState;
  x: number;
  y: number;
  onOpen: (id: string) => void;
  onHover: (id: string | null) => void;
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

export default function PersonaDeskMarvis({ desk, x, y, onOpen, onHover }: PersonaDeskMarvisProps) {
  const status = inferMarvisStatus(desk);
  const statusColor = STATUS_COLORS[status];
  const statusLabel = STATUS_LABELS[status];
  const working = displayedState(desk) === 'working';
  const emoji = avatarEmoji(desk);
  
  return (
    <div
      className="workstation-marvis"
      style={{ left: x, top: y }}
      role="button"
      tabIndex={0}
      onClick={() => onOpen(desk.id)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onOpen(desk.id);
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
          {/* 显示器 */}
          <div className="monitor-marvis">
            <div className={`screen-marvis screen-${status}`}>
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
            </div>
          </div>
          
          {/* 椅子 */}
          <div className="chair-marvis" />
          
          {/* 角色 */}
          <div className="character-marvis">
            <svg viewBox="0 0 60 60" width="60" height="60">
              {/* 头部 */}
              <circle cx="30" cy="20" r="14" fill="#fef3e2" stroke="#c98a2b" strokeWidth="2" />
              {/* 表情 */}
              <text x="30" y="24" textAnchor="middle" fontSize="16">{emoji}</text>
              {/* 身体 */}
              <ellipse 
                cx="30" cy="42" rx="16" ry="12" 
                fill={statusColor} 
                opacity="0.3" 
                stroke={statusColor} 
                strokeWidth="1.5" 
              />
              {/* 手臂 - 工作时打字动画 */}
              <g className={working ? 'typing-arms' : ''}>
                <line x1="18" y1="38" x2="12" y2="50" stroke="#c98a2b" strokeWidth="2.5" strokeLinecap="round">
                  {working && <animate attributeName="y2" values="50;52;50" dur="0.6s" repeatCount="indefinite" />}
                </line>
                <line x1="42" y1="38" x2="48" y2="50" stroke="#c98a2b" strokeWidth="2.5" strokeLinecap="round">
                  {working && <animate attributeName="y2" values="50;52;50" dur="0.6s" begin="0.3s" repeatCount="indefinite" />}
                </line>
              </g>
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
    </div>
  );
}
