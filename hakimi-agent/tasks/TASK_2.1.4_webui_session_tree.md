# 任务 2.1.4: WebUI 可视化会话树

**状态**: 🔵 进行中 (0%)  
**开始时间**: 2026-07-10 07:30 UTC  
**预估时间**: 4 小时  

**优先级**: 🔵 中  
**依赖**: 任务 2.1.1, 2.1.2, 2.1.3 (Lineage 功能已完成)  
**解锁**: Phase 2 功能完整性对齐的最后一环

---

## 📋 目标

在 WebUI 中实现会话树可视化组件，展示父子会话关系，让用户直观理解会话的演化路径。

---

## 🎯 验收标准

- [ ] SessionTree 组件实现完成（React/TypeScript）
- [ ] 树形结构正确渲染（支持 3+ 层嵌套）
- [ ] 折叠/展开交互功能
- [ ] 点击节点跳转到对应会话
- [ ] 显示会话元数据（创建时间、消息数量）
- [ ] 响应式布局（移动端适配）
- [ ] 集成测试通过

---

## 🛠️ 实施步骤

### 步骤 1: 后端 API 准备 (30 分钟)

**文件**: `crates/hakimi-server/src/api/sessions.rs`

**新增端点**:
```rust
/// GET /api/sessions/:session_id/tree
/// 返回会话树结构（包含父节点和子节点）
#[get("/api/sessions/{session_id}/tree")]
async fn get_session_tree(
    session_id: web::Path<String>,
    state: web::Data<AppState>,
) -> Result<HttpResponse, HakimiError> {
    let session_db = &state.session_db;
    
    // 1. 获取当前会话
    let current = session_db.get_session(&session_id)?;
    
    // 2. 获取根会话
    let root = session_db.get_root_session(&session_id)?;
    
    // 3. 获取完整谱系（从根到当前）
    let lineage = session_db.get_session_lineage(&session_id)?;
    
    // 4. 获取所有子会话（递归）
    let children = get_session_tree_recursive(session_db, &session_id)?;
    
    let tree = SessionTreeResponse {
        current: current.clone(),
        root: root.clone(),
        lineage,
        children,
    };
    
    Ok(HttpResponse::Ok().json(tree))
}

/// 递归获取子会话树
fn get_session_tree_recursive(
    db: &SessionDB,
    session_id: &str,
) -> Result<Vec<SessionTreeNode>, HakimiError> {
    let children = db.get_child_sessions(session_id)?;
    
    let mut tree_nodes = Vec::new();
    for child in children {
        let child_children = get_session_tree_recursive(db, &child.id)?;
        tree_nodes.push(SessionTreeNode {
            session: child,
            children: child_children,
        });
    }
    
    Ok(tree_nodes)
}
```

**数据结构**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTreeResponse {
    /// 当前会话
    pub current: SessionMetadata,
    /// 根会话
    pub root: SessionMetadata,
    /// 完整谱系（从根到当前）
    pub lineage: Vec<SessionMetadata>,
    /// 子会话树
    pub children: Vec<SessionTreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTreeNode {
    pub session: SessionMetadata,
    pub children: Vec<SessionTreeNode>,
}
```

**验收**: `curl http://localhost:3005/api/sessions/{id}/tree` 返回正确 JSON

---

### 步骤 2: 前端组件实现 (90 分钟)

**文件**: `crates/hakimi-server/webui/src/components/SessionTree.tsx`

**组件结构**:
```tsx
import React, { useState, useEffect } from 'react';
import { ChevronRight, ChevronDown, Calendar, MessageSquare } from 'lucide-react';

interface SessionTreeProps {
  sessionId: string;
  onSessionClick?: (sessionId: string) => void;
}

interface SessionTreeNode {
  session: SessionMetadata;
  children: SessionTreeNode[];
}

interface SessionMetadata {
  id: string;
  created_at: string;
  title?: string;
  message_count: number;
  parent_id?: string;
  root_id?: string;
}

export const SessionTree: React.FC<SessionTreeProps> = ({ sessionId, onSessionClick }) => {
  const [treeData, setTreeData] = useState<SessionTreeResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expandedNodes, setExpandedNodes] = useState<Set<string>>(new Set([sessionId]));

  useEffect(() => {
    fetchSessionTree();
  }, [sessionId]);

  const fetchSessionTree = async () => {
    try {
      setLoading(true);
      const response = await fetch(`/api/sessions/${sessionId}/tree`);
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }
      const data = await response.json();
      setTreeData(data);
      setError(null);
    } catch (err) {
      setError(err.message);
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
    return <div className="flex items-center justify-center p-8">
      <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500" />
    </div>;
  }

  if (error) {
    return <div className="p-4 bg-red-50 text-red-700 rounded-lg">
      <p>加载会话树失败: {error}</p>
      <button onClick={fetchSessionTree} className="mt-2 text-sm underline">
        重试
      </button>
    </div>;
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
      <div className="mb-4 flex items-center gap-2 text-sm text-gray-600 overflow-x-auto">
        {treeData.lineage.map((session, index) => (
          <React.Fragment key={session.id}>
            {index > 0 && <ChevronRight className="w-4 h-4 flex-shrink-0" />}
            <button
              onClick={() => handleNodeClick(session.id)}
              className={`px-2 py-1 rounded hover:bg-gray-100 whitespace-nowrap ${
                session.id === sessionId ? 'bg-blue-100 text-blue-700 font-medium' : ''
              }`}
            >
              {session.title || `会话 ${session.id.slice(0, 8)}`}
            </button>
          </React.Fragment>
        ))}
      </div>

      {/* 子会话树 */}
      {treeData.children.length > 0 && (
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
      )}

      {treeData.children.length === 0 && (
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
              <Calendar className="w-3 h-3" />
              <span>{new Date(node.session.created_at).toLocaleDateString('zh-CN')}</span>
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
```

**样式文件**: `crates/hakimi-server/webui/src/components/SessionTree.css` (可选，使用 Tailwind)

**验收**: 组件渲染正确，交互流畅

---

### 步骤 3: 集成到主界面 (30 分钟)

**文件**: `crates/hakimi-server/webui/src/pages/SessionView.tsx`

**集成代码**:
```tsx
import { SessionTree } from '../components/SessionTree';

export const SessionView: React.FC = () => {
  const { sessionId } = useParams<{ sessionId: string }>();
  const navigate = useNavigate();

  const handleSessionClick = (newSessionId: string) => {
    navigate(`/sessions/${newSessionId}`);
  };

  return (
    <div className="session-view grid grid-cols-1 lg:grid-cols-4 gap-4 p-4">
      {/* 左侧：会话树（桌面端） */}
      <aside className="hidden lg:block lg:col-span-1">
        <SessionTree
          sessionId={sessionId}
          onSessionClick={handleSessionClick}
        />
      </aside>

      {/* 中间：主对话区 */}
      <main className="lg:col-span-3">
        <ChatInterface sessionId={sessionId} />
      </main>

      {/* 移动端：抽屉式会话树 */}
      <MobileDrawer>
        <SessionTree
          sessionId={sessionId}
          onSessionClick={handleSessionClick}
        />
      </MobileDrawer>
    </div>
  );
};
```

---

### 步骤 4: 测试 (60 分钟)

#### 单元测试
**文件**: `crates/hakimi-server/tests/api/session_tree_test.rs`

```rust
#[tokio::test]
async fn test_get_session_tree_single() {
    let app = setup_test_app().await;
    let session_id = create_test_session(&app).await;

    let response = app
        .client()
        .get(&format!("/api/sessions/{}/tree", session_id))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    let tree: SessionTreeResponse = response.json().await.unwrap();
    
    assert_eq!(tree.current.id, session_id);
    assert_eq!(tree.root.id, session_id);
    assert_eq!(tree.lineage.len(), 1);
    assert_eq!(tree.children.len(), 0);
}

#[tokio::test]
async fn test_get_session_tree_with_children() {
    let app = setup_test_app().await;
    
    // 创建父会话
    let parent_id = create_test_session(&app).await;
    
    // 创建 2 个子会话
    let child1_id = create_child_session(&app, &parent_id).await;
    let child2_id = create_child_session(&app, &parent_id).await;
    
    // 创建孙会话
    let grandchild_id = create_child_session(&app, &child1_id).await;

    let response = app
        .client()
        .get(&format!("/api/sessions/{}/tree", parent_id))
        .send()
        .await
        .unwrap();

    let tree: SessionTreeResponse = response.json().await.unwrap();
    
    assert_eq!(tree.children.len(), 2);
    assert_eq!(tree.children[0].children.len(), 1);
    assert_eq!(tree.children[1].children.len(), 0);
    
    // 验证孙会话
    assert_eq!(tree.children[0].children[0].session.id, grandchild_id);
}

#[tokio::test]
async fn test_get_session_tree_lineage() {
    let app = setup_test_app().await;
    
    let root_id = create_test_session(&app).await;
    let child_id = create_child_session(&app, &root_id).await;
    let grandchild_id = create_child_session(&app, &child_id).await;

    let response = app
        .client()
        .get(&format!("/api/sessions/{}/tree", grandchild_id))
        .send()
        .await
        .unwrap();

    let tree: SessionTreeResponse = response.json().await.unwrap();
    
    assert_eq!(tree.root.id, root_id);
    assert_eq!(tree.lineage.len(), 3);
    assert_eq!(tree.lineage[0].id, root_id);
    assert_eq!(tree.lineage[1].id, child_id);
    assert_eq!(tree.lineage[2].id, grandchild_id);
}
```

#### 前端测试
**文件**: `crates/hakimi-server/webui/src/components/SessionTree.test.tsx`

```tsx
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { SessionTree } from './SessionTree';
import '@testing-library/jest-dom';

// Mock fetch
global.fetch = jest.fn();

describe('SessionTree', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('displays loading state initially', () => {
    (global.fetch as jest.Mock).mockImplementation(() => 
      new Promise(() => {}) // Never resolves
    );

    render(<SessionTree sessionId="test-123" />);
    expect(screen.getByRole('status')).toBeInTheDocument();
  });

  it('renders session tree correctly', async () => {
    const mockData = {
      current: { id: 'session-2', title: '当前会话', message_count: 10 },
      root: { id: 'session-1', title: '根会话', message_count: 5 },
      lineage: [
        { id: 'session-1', title: '根会话' },
        { id: 'session-2', title: '当前会话' },
      ],
      children: [
        {
          session: { id: 'session-3', title: '子会话', message_count: 3 },
          children: [],
        },
      ],
    };

    (global.fetch as jest.Mock).mockResolvedValue({
      ok: true,
      json: async () => mockData,
    });

    render(<SessionTree sessionId="session-2" />);

    await waitFor(() => {
      expect(screen.getByText('根会话')).toBeInTheDocument();
      expect(screen.getByText('当前会话')).toBeInTheDocument();
      expect(screen.getByText('子会话')).toBeInTheDocument();
    });
  });

  it('handles node expansion', async () => {
    const mockData = {
      current: { id: 'session-1' },
      root: { id: 'session-1' },
      lineage: [{ id: 'session-1' }],
      children: [
        {
          session: { id: 'session-2', title: '父节点' },
          children: [
            {
              session: { id: 'session-3', title: '子节点' },
              children: [],
            },
          ],
        },
      ],
    };

    (global.fetch as jest.Mock).mockResolvedValue({
      ok: true,
      json: async () => mockData,
    });

    render(<SessionTree sessionId="session-1" />);

    await waitFor(() => {
      expect(screen.getByText('父节点')).toBeInTheDocument();
    });

    // 初始状态：子节点未展开（假设默认折叠）
    // 点击展开按钮
    const expandButton = screen.getByRole('button', { name: /expand/i });
    fireEvent.click(expandButton);

    // 验证子节点显示
    expect(screen.getByText('子节点')).toBeInTheDocument();
  });

  it('handles session click', async () => {
    const mockData = {
      current: { id: 'session-1' },
      root: { id: 'session-1' },
      lineage: [{ id: 'session-1' }],
      children: [],
    };

    (global.fetch as jest.Mock).mockResolvedValue({
      ok: true,
      json: async () => mockData,
    });

    const onSessionClick = jest.fn();
    render(<SessionTree sessionId="session-1" onSessionClick={onSessionClick} />);

    await waitFor(() => {
      expect(screen.getByText(/会话 session-1/i)).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText(/会话 session-1/i));
    expect(onSessionClick).toHaveBeenCalledWith('session-1');
  });

  it('displays error message on fetch failure', async () => {
    (global.fetch as jest.Mock).mockRejectedValue(new Error('Network error'));

    render(<SessionTree sessionId="test-123" />);

    await waitFor(() => {
      expect(screen.getByText(/加载会话树失败/i)).toBeInTheDocument();
    });

    // 验证重试按钮
    const retryButton = screen.getByText(/重试/i);
    expect(retryButton).toBeInTheDocument();
  });
});
```

---

## 📊 完成检查清单

- [ ] 后端 API `/api/sessions/:id/tree` 实现完成
- [ ] `SessionTreeResponse` 和 `SessionTreeNode` 数据结构定义
- [ ] `get_session_tree_recursive()` 递归逻辑正确
- [ ] `SessionTree.tsx` 组件实现完成
- [ ] 折叠/展开交互正确
- [ ] 点击跳转功能正常
- [ ] 响应式布局适配（桌面/移动端）
- [ ] 后端单元测试通过（3+ 测试用例）
- [ ] 前端单元测试通过（4+ 测试用例）
- [ ] 集成测试通过（E2E）
- [ ] 编译无错误：`cargo build --release`
- [ ] 前端构建成功：`npm run build`
- [ ] 浏览器手动测试通过

---

## 🎨 UI/UX 设计要点

### 视觉设计
- **树形缩进**: 每层 20px，最多显示 5 层（更深层级滚动）
- **当前节点高亮**: 蓝色背景 + 左侧边框
- **图标**: 使用 lucide-react (ChevronRight/Down, MessageSquare, Calendar)
- **颜色**: Tailwind 默认调色板（blue-500, gray-600）

### 交互设计
- **默认展开**: 当前会话及其祖先节点
- **延迟加载**: 超过 50 个子会话时分页加载
- **快捷键**: `←/→` 折叠/展开，`↑/↓` 导航
- **拖拽**: 暂不支持（未来版本）

### 性能优化
- **虚拟滚动**: 使用 `react-window` 处理大型树（>100 节点）
- **防抖**: 节点点击防抖 300ms
- **缓存**: API 响应缓存 5 分钟

---

## 🔗 参考资料

- [React 树形组件最佳实践](https://react-typescript-cheatsheet.netlify.app/docs/basic/getting-started/advanced)
- [Tailwind CSS 文档](https://tailwindcss.com/docs)
- [lucide-react 图标库](https://lucide.dev/)
- [react-window 虚拟滚动](https://react-window.vercel.app/)

---

**创建时间**: 2026-07-10  
**预计完成**: 2026-07-10（4 小时内）  
**负责人**: 自动化进化引擎
