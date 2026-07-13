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

// 生成走廊路径（贝塞尔曲线，从工位侧面绕行）
function calculatePath(from: { x: number; y: number }, to: { x: number; y: number }): string {
  const dx = to.x - from.x;
  const dy = to.y - from.y;
  
  // 检测是否在同一行（垂直距离小于工位高度）
  const sameRow = Math.abs(dy) < 100;
  
  if (sameRow) {
    // 同一行：从工位下方绕行（走廊路径）
    const detourY = Math.max(from.y, to.y) + 60; // 工位下方 60px
    return `M ${from.x} ${from.y} L ${from.x} ${detourY} L ${to.x} ${detourY} L ${to.x} ${to.y}`;
  } else {
    // 不同行：从工位侧面绕行
    const sideOffset = dx > 0 ? -50 : 50; // 向外侧偏移
    const midX = (from.x + to.x) / 2 + sideOffset;
    const midY = (from.y + to.y) / 2;
    return `M ${from.x} ${from.y} Q ${midX} ${from.y}, ${midX} ${midY} Q ${midX} ${to.y}, ${to.x} ${to.y}`;
  }
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
  
  // 路径插值计算
  const dx = end.x - start.x;
  const dy = end.y - start.y;
  const sameRow = Math.abs(dy) < 100;
  
  let x: number, y: number, tangentX: number;
  
  if (sameRow) {
    // 同一行：折线路径（直线插值）
    const detourY = Math.max(start.y, end.y) + 60;
    const t = progress;
    
    if (t < 0.25) {
      // 第一段：垂直向下
      const t1 = t / 0.25;
      x = start.x;
      y = start.y + (detourY - start.y) * t1;
      tangentX = 0;
    } else if (t < 0.75) {
      // 第二段：水平移动
      const t2 = (t - 0.25) / 0.5;
      x = start.x + (end.x - start.x) * t2;
      y = detourY;
      tangentX = end.x - start.x;
    } else {
      // 第三段：垂直向上
      const t3 = (t - 0.75) / 0.25;
      x = end.x;
      y = detourY + (end.y - detourY) * t3;
      tangentX = 0;
    }
  } else {
    // 不同行：S形曲线（两段二次贝塞尔）
    const sideOffset = dx > 0 ? -50 : 50;
    const midX = (start.x + end.x) / 2 + sideOffset;
    const midY = (start.y + end.y) / 2;
    const t = progress;
    
    if (t < 0.5) {
      // 第一段贝塞尔：start -> (midX, start.y) -> (midX, midY)
      const t1 = t / 0.5;
      const p0x = start.x, p0y = start.y;
      const p1x = midX, p1y = start.y;
      const p2x = midX, p2y = midY;
      x = (1 - t1) * (1 - t1) * p0x + 2 * (1 - t1) * t1 * p1x + t1 * t1 * p2x;
      y = (1 - t1) * (1 - t1) * p0y + 2 * (1 - t1) * t1 * p1y + t1 * t1 * p2y;
      tangentX = 2 * (1 - t1) * (p1x - p0x) + 2 * t1 * (p2x - p1x);
    } else {
      // 第二段贝塞尔：(midX, midY) -> (midX, end.y) -> end
      const t2 = (t - 0.5) / 0.5;
      const p0x = midX, p0y = midY;
      const p1x = midX, p1y = end.y;
      const p2x = end.x, p2y = end.y;
      x = (1 - t2) * (1 - t2) * p0x + 2 * (1 - t2) * t2 * p1x + t2 * t2 * p2x;
      y = (1 - t2) * (1 - t2) * p0y + 2 * (1 - t2) * t2 * p1y + t2 * t2 * p2y;
      tangentX = 2 * (1 - t2) * (p1x - p0x) + 2 * t2 * (p2x - p1x);
    }
  }
  
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
