/* eslint-disable react-refresh/only-export-components --
   This module intentionally co-locates the provider, hook, and message catalog;
   fast-refresh of this rarely-edited file is not a concern. */
import { createContext, useContext, useMemo, useState, type ReactNode } from 'react';

export type Lang = 'en' | 'zh';

const STORAGE_KEY = 'hakimi-webui-lang';

interface Entry {
  en: string;
  zh: string;
}

// Lightweight, dependency-free message catalog. Keys are namespaced by surface.
// Only static UI chrome is translated; dynamic content (persona names, models,
// session titles, message bodies) is left as-is.
const messages = {
  'lang.toggle': { en: '中文', zh: 'EN' },
  'lang.toggleTitle': { en: 'Switch to Chinese', zh: 'Switch to English' },

  'top.subtitle': { en: 'Hakimi Agent', zh: 'Hakimi 智能体' },
  'top.title': { en: 'Operator Console', zh: '操作台' },
  'top.offline': { en: 'offline', zh: '离线' },
  'top.modelPending': { en: 'model pending', zh: '模型待定' },
  'top.token': { en: 'Bearer token', zh: '访问令牌' },
  'top.saveToken': { en: 'Save token', zh: '保存令牌' },
  'common.refresh': { en: 'Refresh', zh: '刷新' },

  'metric.sessions': { en: 'sessions', zh: '会话' },
  'metric.tools': { en: 'tools', zh: '工具' },
  'metric.tokens': { en: 'tokens', zh: '令牌' },

  'sessions.eyebrow': { en: 'Sessions', zh: '会话' },
  'sessions.heading': { en: 'Recent Work', zh: '最近会话' },
  'sessions.filter': { en: 'Filter sessions', zh: '筛选会话' },
  'sessions.empty': { en: 'No sessions', zh: '暂无会话' },
  'sessions.msg': { en: 'msg', zh: '条' },
  'sessions.toolsShort': { en: 'tools', zh: '工具' },
  'sessions.modelUnknown': { en: 'model unknown', zh: '模型未知' },
  'sessions.delete': { en: 'Delete session', zh: '删除会话' },
  'sessions.deleteConfirm': {
    en: 'Delete this session? This cannot be undone.',
    zh: '删除该会话?此操作不可撤销。',
  },

  'chat.live': { en: 'Live Agent', zh: '当前智能体' },
  'chat.default': { en: 'Chat', zh: '对话' },
  'chat.hideSessions': { en: 'Hide sessions', zh: '隐藏会话栏' },
  'chat.showSessions': { en: 'Show sessions', zh: '显示会话栏' },
  'chat.hidePanel': { en: 'Hide panel', zh: '隐藏面板' },
  'chat.showPanel': { en: 'Show panel', zh: '显示面板' },
  'chat.loadingSession': { en: 'Loading session', zh: '加载会话中' },
  'chat.loadingRuntime': { en: 'Loading runtime', zh: '加载运行时' },
  'chat.ready': { en: 'Ready', zh: '就绪' },
  'chat.running': { en: 'Running turn', zh: '执行中' },
  'chat.session': { en: 'session', zh: '会话' },
  'chat.copy': { en: 'Copy', zh: '复制' },
  'chat.retry': { en: 'Retry', zh: '重试' },
  'chat.delete': { en: 'Delete', zh: '删除' },
  'chat.placeholder': { en: 'Send a task to Hakimi', zh: '给 Hakimi 发送任务' },
  'chat.chatEnabled': { en: 'chat enabled', zh: '对话已启用' },
  'chat.chatPending': { en: 'chat pending', zh: '对话待启用' },
  'chat.send': { en: 'Send', zh: '发送' },
  'chat.roleUser': { en: 'user', zh: '用户' },
  'chat.roleAssistant': { en: 'assistant', zh: '助手' },
  'chat.interrupted': {
    en: 'Connection interrupted; the partial reply was kept. Use the message Retry to continue.',
    zh: '连接中断,已保留收到的部分回复。可点该消息的重试按钮继续。',
  },

  'panel.runtime': { en: 'Runtime', zh: '运行时' },
  'panel.tools': { en: 'Tools', zh: '工具' },
  'panel.skills': { en: 'Skills', zh: '技能' },
  'panel.server': { en: 'Server', zh: '服务' },
  'panel.status': { en: 'Status', zh: '状态' },
  'panel.model': { en: 'Model', zh: '模型' },
  'panel.auth': { en: 'Auth', zh: '鉴权' },
  'panel.persistence': { en: 'Persistence', zh: '持久化' },
  'panel.resources': { en: 'Resources', zh: '资源' },
  'panel.credentials': { en: 'credentials', zh: '凭据' },
  'panel.capabilities': { en: 'Capabilities', zh: '能力' },
  'panel.sessionInspector': { en: 'Session Inspector', zh: '会话检视' },
  'panel.noSession': { en: 'No session selected', zh: '未选择会话' },
  'panel.loadingMessages': { en: 'Loading messages', zh: '加载消息中' },
  'panel.toolRegistry': { en: 'Tool Registry', zh: '工具注册表' },
  'panel.filterTools': { en: 'Filter tools', zh: '筛选工具' },
  'panel.toolsets': { en: 'Toolsets', zh: '工具集' },
  'panel.activeSkills': { en: 'Active Skills', zh: '已激活技能' },
  'panel.skillCatalog': { en: 'Skill Catalog', zh: '技能目录' },
  'panel.none': { en: 'none', zh: '无' },

  'rail.newPersona': { en: 'New persona', zh: '新建人格' },
  'rail.workspace': { en: 'Workspace files', zh: '工作空间文件' },
  'rail.instance': { en: 'Instance settings', zh: '实例设置' },
  'rail.configure': { en: 'Configure', zh: '配置' },

  'form.editPersona': { en: 'Edit persona', zh: '编辑人格' },
  'form.newPersona': { en: 'New persona', zh: '新建人格' },
  'form.createTitle': { en: 'Create a persona', zh: '创建人格' },
  'form.cancel': { en: 'Cancel', zh: '取消' },
  'form.save': { en: 'Save', zh: '保存' },
  'form.delete': { en: 'Delete', zh: '删除' },
  'form.identity': { en: 'Identity', zh: '身份' },
  'form.id': { en: 'id', zh: 'id' },
  'form.name': { en: 'name', zh: '名称' },
  'form.avatar': { en: 'avatar (emoji)', zh: '头像(emoji)' },
  'form.description': { en: 'description', zh: '简介' },
  'form.descriptionPlaceholder': { en: 'Short role summary', zh: '简短角色描述' },
  'form.modelGroup': { en: 'Model', zh: '模型' },
  'form.model': { en: 'model', zh: '模型' },
  'form.modelInherit': { en: '(inherit default)', zh: '(继承默认)' },
  'form.reasoning': { en: 'reasoning effort', zh: '推理强度' },
  'form.default': { en: '(default)', zh: '(默认)' },
  'form.isDefault': { en: 'Default persona (gateway fallback)', zh: '默认人格(网关兜底)' },
  'form.systemPrompt': { en: 'System prompt', zh: '系统提示词' },
  'form.identityPrompt': { en: 'identity prompt', zh: '身份提示词' },
  'form.skills': { en: 'Skills', zh: '技能' },
  'form.noSkills': { en: 'No skills available', zh: '暂无可用技能' },
  'form.bindings': { en: 'Channel bindings', zh: 'Channel 绑定' },
  'form.bindingsHint': {
    en: 'one platform:bot_id per line (empty = WebUI only)',
    zh: '每行一个 platform:bot_id(留空 = 仅 WebUI)',
  },
  'form.idError': {
    en: 'Persona id must match [a-z0-9][a-z0-9_-]{0,63}',
    zh: '人格 id 须匹配 [a-z0-9][a-z0-9_-]{0,63}',
  },

  'instance.routing': { en: 'Routing', zh: '路由' },
  'instance.bindings': { en: 'Channel bindings', zh: 'Channel 绑定' },
  'instance.channel': { en: 'platform:bot_id', zh: 'platform:bot_id' },
  'instance.persona': { en: 'persona', zh: '人格' },
  'instance.noBindings': {
    en: 'No channel bindings. Unbound channels fall back to the default persona.',
    zh: '暂无 channel 绑定。未绑定的 channel 落到默认人格。',
  },
  'instance.defaultFallback': { en: 'Default persona (fallback):', zh: '默认人格(兜底):' },

  'ws.root': { en: 'workspace', zh: '工作空间' },
  'ws.empty': { en: 'Empty directory', zh: '空目录' },
  'ws.title': { en: 'Workspace', zh: '工作空间' },
  'ws.selectFile': { en: 'Select a file to view', zh: '选择文件查看' },
  'ws.loadingFile': { en: 'Loading file', zh: '加载文件中' },
} satisfies Record<string, Entry>;

export type MessageKey = keyof typeof messages;

interface I18nContextValue {
  lang: Lang;
  setLang: (lang: Lang) => void;
  t: (key: MessageKey) => string;
}

const I18nContext = createContext<I18nContextValue | null>(null);

function readInitialLang(): Lang {
  const stored = window.localStorage.getItem(STORAGE_KEY);
  if (stored === 'en' || stored === 'zh') {
    return stored;
  }
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh' : 'en';
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(readInitialLang);

  const value = useMemo<I18nContextValue>(
    () => ({
      lang,
      setLang: (next) => {
        setLangState(next);
        window.localStorage.setItem(STORAGE_KEY, next);
      },
      t: (key) => messages[key]?.[lang] ?? key,
    }),
    [lang],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nContextValue {
  const ctx = useContext(I18nContext);
  if (!ctx) {
    throw new Error('useI18n must be used within I18nProvider');
  }
  return ctx;
}
