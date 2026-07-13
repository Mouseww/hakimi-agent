import React, { useEffect, useState } from 'react';

interface WalkingCatProps {
  fromSeat: { x: number; y: number };
  toSeat: { x: number; y: number };
  phase: 'going' | 'returning';
  scarfColor: string;
  tailColor: string;
}

// 计算猫咪在工位上的实际位置（底部中心）
function catPosition(seat: { x: number; y: number }): { x: number; y: number } {
  return {
    x: seat.x + 110,  // 工位宽度 220px，中心在 110px
    y: seat.y + 195,  // 工位高度 230px，猫咪 bottom: 35px，所以 y = 230 - 35 = 195
  };
}

// 生成走廊路径（贝塞尔曲线，避开工位）
function calculatePath(from: { x: number; y: number }, to: { x: number; y: number }): string {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  const distance = Math.sqrt(dx * dx + dy * dy);
  
  // 控制点：在起点和终点之间，稍微偏向下方（走廊）
  const controlOffset = Math.min(distance * 0.3, 80);
  const midX = (from.x + to.x) / 2;
  const midY = Math.max(from.y, to.y) + controlOffset;
  
  return `M ${from.x} ${from.y} Q ${midX} ${midY}, ${to.x} ${to.y}`;
}

export const WalkingCat: React.FC<WalkingCatProps> = ({ 
  fromSeat, 
  toSeat, 
  phase, 
  scarfColor, 
  tailColor 
}) => {
  const [progress, setProgress] = useState(0);
  
  const start = phase === 'going' ? catPosition(fromSeat) : catPosition(toSeat);
  const end = phase === 'going' ? catPosition(toSeat) : catPosition(fromSeat);
  
  useEffect(() => {
    setProgress(0);
    const startTime = Date.now();
    const duration = 1400; // 与 PHASE_DURATIONS 一致
    
    const animate = () => {
      const elapsed = Date.now() - startTime;
      const p = Math.min(elapsed / duration, 1);
      setProgress(p);
      
      if (p < 1) {
        requestAnimationFrame(animate);
      }
    };
    
    const id = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(id);
  }, [start.x, start.y, end.x, end.y, phase]);
  
  // 使用二次贝塞尔曲线插值
  const dx = end.x - start.x;
  const dy = end.y - start.y;
  const distance = Math.sqrt(dx * dx + dy * dy);
  const controlOffset = Math.min(distance * 0.3, 80);
  const midX = (start.x + end.x) / 2;
  const midY = Math.max(start.y, end.y) + controlOffset;
  
  const t = progress;
  const x = (1 - t) * (1 - t) * start.x + 2 * (1 - t) * t * midX + t * t * end.x;
  const y = (1 - t) * (1 - t) * start.y + 2 * (1 - t) * t * midY + t * t * end.y;
  
  // 计算朝向（基于切线方向）
  const tangentX = 2 * (1 - t) * (midX - start.x) + 2 * t * (end.x - midX);
  const facingRight = tangentX > 0;
  
  return (
    <>
      {/* 路径可视化（调试用，可选） */}
      <svg 
        style={{ 
          position: 'absolute', 
          left: 0, 
          top: 0, 
          width: '100%', 
          height: '100%', 
          pointerEvents: 'none',
          zIndex: 28,
        }}
      >
        <path
          d={calculatePath(start, end)}
          fill="none"
          stroke="rgba(148, 163, 184, 0.2)"
          strokeWidth="1"
          strokeDasharray="4 4"
        />
      </svg>
      
      {/* 行走中的猫咪 */}
      <div
        className="walking-cat-v3"
        style={{
          position: 'absolute',
          left: x,
          top: y,
          transform: `translate(-50%, -50%) ${facingRight ? 'scaleX(1)' : 'scaleX(-1)'}`,
          width: '60px',
          height: '60px',
          zIndex: 29,
          transition: 'none',
        }}
      >
        <svg 
          viewBox="0 0 80 80" 
          width="60" 
          height="60"
          style={{
            filter: 'drop-shadow(0 2px 4px rgba(0, 0, 0, 0.2))',
          }}
        >
          {/* 猫咪侧面剪影（简化版，行走姿态）*/}
          <g>
            {/* 身体 */}
            <ellipse cx="40" cy="45" rx="18" ry="14" fill="#94a3b8" />
            
            {/* 头部 */}
            <circle cx="28" cy="38" r="12" fill="#94a3b8" />
            
            {/* 耳朵 */}
            <path d="M 22 30 L 18 22 L 26 28 Z" fill="#94a3b8" />
            <path d="M 32 30 L 36 22 L 28 28 Z" fill="#94a3b8" />
            
            {/* 吻部 */}
            <ellipse cx="20" cy="40" rx="5" ry="4" fill="#94a3b8" />
            
            {/* 鼻尖 */}
            <circle cx="17" cy="40" r="1.5" fill="#64748b" />
            
            {/* 围脖 */}
            <ellipse 
              cx="32" 
              cy="44" 
              rx="8" 
              ry="4" 
              fill={scarfColor}
              className="walking-cat-scarf"
            />
            
            {/* 尾巴（行走时摆动）*/}
            <path 
              d="M 55 48 Q 65 45, 70 42" 
              fill="none" 
              stroke={tailColor} 
              strokeWidth="4" 
              strokeLinecap="round"
              className="walking-cat-tail"
            />
            
            {/* 前腿（行走动画）*/}
            <g className="walking-cat-front-legs">
              <line x1="32" y1="56" x2="32" y2="68" stroke="#94a3b8" strokeWidth="3.5" strokeLinecap="round" />
              <line x1="38" y1="56" x2="38" y2="68" stroke="#94a3b8" strokeWidth="3.5" strokeLinecap="round" />
            </g>
            
            {/* 后腿（行走动画）*/}
            <g className="walking-cat-back-legs">
              <line x1="48" y1="56" x2="48" y2="68" stroke="#94a3b8" strokeWidth="3.5" strokeLinecap="round" />
              <line x1="54" y1="56" x2="54" y2="68" stroke="#94a3b8" strokeWidth="3.5" strokeLinecap="round" />
            </g>
          </g>
        </svg>
      </div>
    </>
  );
};
