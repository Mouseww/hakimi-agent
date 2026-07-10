import React, { useState, useEffect } from 'react';
import { ChevronRight, ChevronDown, MessageSquare } from 'lucide-react';
import { fetchSessionTree, type SessionTreeResponse, type SessionTreeNode } from './api';

interface SessionTreeProps {
  sessionId: string;
  onSessionClick?: (sessionId: string) => void;
}

export const SessionTree: React.FC<SessionTreeProps> = ({ sessionId, onSessionClick }) => {
  const [treeData, setTreeData] = useState<SessionTreeResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expandedNodes, setExpandedNodes] = useState<Set<string>>(new Set([sessionId]));

  useEffect(() => {
    fetchTree();
  }, [sessionId]);

  const fetchTree = async () => {
    try {
      setLoading(true);
      const data = await fetchSessionTree(sessionId);
      setTreeData(data);
      setError(null);
      
      // Auto-expand current session and all ancestors
      const expanded = new Set<string>([sessionId]);
      data.lineage.forEach(session => expanded.add(session.id));
      setExpandedNodes(expanded);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  };

  const toggleNode = (nodeId: string) => {
    setExpandedNodes(prev => {
      const newSet = new Set(prev);
      if (newSet.has(nodeId)) {
        newSet.delete(nodeId);
      } else {
        newSet.add(nodeId);
      }
      return newSet;
    });
  };

  const handleNodeClick = (nodeId: string) => {
    if (onSessionClick) {
      onSessionClick(nodeId);
    }
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center p-8">
        <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4 bg-red-50 text-red-700 rounded-lg">
        <p>加载会话树失败: {error}</p>
        <button onClick={fetchTree} className="mt-2 text-sm underline hover:no-underline">
          重试
        </button>
      </div>
    );
  }

  if (!treeData) {
    return null;
  }

  return (
    <div className="session-tree bg-white rounded-lg shadow-sm p-4">
      <div className="mb-4 border-b pb-2">
        <h3 className="text-lg font-semibold text-gray-800">会话树</h3>
        <p className="text-sm text-gray-500">
          根会话: {treeData.root.title || treeData.root.id.slice(0, 8)}
        </p>
      </div>

      {/* 谱系路径（面包屑） */}
      {treeData.lineage.length > 0 && (
        <div className="mb-4 flex items-center gap-2 text-sm text-gray-600 overflow-x-auto pb-2">
          {treeData.lineage.map((session, index) => (
            <React.Fragment key={session.id}>
              {index > 0 && <ChevronRight className="w-4 h-4 flex-shrink-0" />}
              <button
                onClick={() => handleNodeClick(session.id)}
                className={`px-2 py-1 rounded hover:bg-gray-100 whitespace-nowrap transition-colors ${
                  session.id === sessionId ? 'bg-blue-100 text-blue-700 font-medium' : ''
                }`}
              >
                {session.title || `会话 ${session.id.slice(0, 8)}`}
              </button>
            </React.Fragment>
          ))}
        </div>
      )}

      {/* 子会话树 */}
      {treeData.children.length > 0 ? (
        <div className="mt-4">
          <h4 className="text-sm font-medium text-gray-700 mb-2">子会话</h4>
          <div className="space-y-1">
            {treeData.children.map(node => (
              <SessionTreeNodeComponent
                key={node.session.id}
                node={node}
                currentSessionId={sessionId}
                expandedNodes={expandedNodes}
                onToggle={toggleNode}
                onClick={handleNodeClick}
                depth={0}
              />
            ))}
          </div>
        </div>
      ) : (
        <div className="text-sm text-gray-500 italic text-center py-4">
          当前会话暂无子会话
        </div>
      )}
    </div>
  );
};

interface SessionTreeNodeComponentProps {
  node: SessionTreeNode;
  currentSessionId: string;
  expandedNodes: Set<string>;
  onToggle: (nodeId: string) => void;
  onClick: (nodeId: string) => void;
  depth: number;
}

const SessionTreeNodeComponent: React.FC<SessionTreeNodeComponentProps> = ({
  node,
  currentSessionId,
  expandedNodes,
  onToggle,
  onClick,
  depth,
}) => {
  const isExpanded = expandedNodes.has(node.session.id);
  const isCurrent = node.session.id === currentSessionId;
  const hasChildren = node.children.length > 0;

  const formatDate = (dateString: string) => {
    try {
      return new Date(dateString).toLocaleDateString('zh-CN', {
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
      });
    } catch {
      return dateString;
    }
  };

  return (
    <div className="session-tree-node">
      <div
        className={`flex items-center gap-2 p-2 rounded cursor-pointer hover:bg-gray-50 transition-colors ${
          isCurrent ? 'bg-blue-50 border-l-2 border-blue-500' : ''
        }`}
        style={{ paddingLeft: `${depth * 20 + 8}px` }}
      >
        {/* 展开/折叠图标 */}
        {hasChildren && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              onToggle(node.session.id);
            }}
            className="flex-shrink-0 w-4 h-4 flex items-center justify-center hover:bg-gray-200 rounded"
            aria-label={isExpanded ? '折叠' : '展开'}
          >
            {isExpanded ? (
              <ChevronDown className="w-4 h-4 text-gray-600" />
            ) : (
              <ChevronRight className="w-4 h-4 text-gray-600" />
            )}
          </button>
        )}
        {!hasChildren && <div className="w-4 h-4 flex-shrink-0" />}

        {/* 会话信息 */}
        <div
          onClick={() => onClick(node.session.id)}
          className="flex-1 flex items-center gap-3 min-w-0"
        >
          <MessageSquare className="w-4 h-4 text-gray-400 flex-shrink-0" />
          <div className="flex-1 min-w-0">
            <div className="text-sm font-medium text-gray-800 truncate">
              {node.session.title || `会话 ${node.session.id.slice(0, 8)}`}
            </div>
            <div className="flex items-center gap-2 text-xs text-gray-500">
              <span>{formatDate(node.session.created_at)}</span>
              <span>·</span>
              <span>{node.session.message_count} 条消息</span>
            </div>
          </div>
        </div>
      </div>

      {/* 递归渲染子节点 */}
      {isExpanded && hasChildren && (
        <div className="ml-2">
          {node.children.map(child => (
            <SessionTreeNodeComponent
              key={child.session.id}
              node={child}
              currentSessionId={currentSessionId}
              expandedNodes={expandedNodes}
              onToggle={onToggle}
              onClick={onClick}
              depth={depth + 1}
            />
          ))}
        </div>
      )}
    </div>
  );
};
