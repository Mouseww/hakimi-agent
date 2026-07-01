import { createContext, useCallback, useContext, useState, type ReactNode } from 'react';

type Entry = { en: string; zh: string };

const messages = {
  // ---- Office view ----
  'office.live': { en: 'Live', zh: '实时' },
  'office.offline': { en: 'Offline (reconnecting)', zh: '离线(重连中)' },
  'office.clickHint': { en: 'Click a desk to open chat', zh: '点击工位进入对话' },
  'office.team': { en: 'Team', zh: '组队' },
  'office.empty': { en: 'No agents yet', zh: '暂无员工' },
  'office.state.idle': { en: 'idle', zh: '空闲' },
  'office.state.working': { en: 'working', zh: '执行中' },
  'office.state.consulting': { en: 'delegating', zh: '委派中' },
  'office.state.in_team': { en: 'in a team', zh: '组队中' },
  'office.delegatedBy': { en: 'Delegated by', zh: '受委派自' },

  // ---- Topbar ----
  'topbar.brand': { en: 'Hakimi Agent', zh: 'Hakimi Agent' },
  'topbar.console': { en: 'Operator Console', zh: '运维控制台' },
  'topbar.offline': { en: 'offline', zh: '离线' },
  'topbar.modelPending': { en: 'model pending', zh: '模型加载中' },
  'topbar.bearerToken': { en: 'Bearer token', zh: 'Bearer 令牌' },
  'topbar.saveToken': { en: 'Save token', zh: '保存令牌' },
  'topbar.refresh': { en: 'Refresh', zh: '刷新' },

  // ---- Auth / Login ----
  'auth.required': { en: 'Authentication Required', zh: '需要身份认证' },
  'auth.enterToken': { en: 'Enter your Bearer token to access the console.', zh: '请输入 Bearer 令牌以访问控制台。' },
  'auth.login': { en: 'Login', zh: '登录' },
  'auth.tokenPlaceholder': { en: 'Paste your Bearer token here', zh: '在此粘贴 Bearer 令牌' },

  // ---- Agent Rail ----
  'rail.newPersona': { en: 'New agent', zh: '新建员工' },
  'rail.office': { en: 'Office', zh: '办公室' },
  'rail.workspace': { en: 'Workspace files', zh: '工作区文件' },
  'rail.instance': { en: 'Settings', zh: '设置' },
  'rail.configure': { en: 'Configure', zh: '配置' },

  // ---- Chat view ----
  'chat.liveAgent': { en: 'Live Agent', zh: '在线代理' },
  'chat.chat': { en: 'Chat', zh: '对话' },
  'chat.hideSessions': { en: 'Hide sessions', zh: '隐藏会话' },
  'chat.showSessions': { en: 'Show sessions', zh: '显示会话' },
  'chat.hidePanel': { en: 'Hide panel', zh: '隐藏面板' },
  'chat.showPanel': { en: 'Show panel', zh: '显示面板' },
  'chat.ready': { en: 'Ready', zh: '就绪' },
  'chat.sendTask': { en: 'Send a task to Hakimi', zh: '发送任务给 Hakimi' },
  'chat.chatEnabled': { en: 'chat enabled', zh: '对话已启用' },
  'chat.chatPending': { en: 'chat pending', zh: '对话等待中' },
  'chat.send': { en: 'Send', zh: '发送' },
  'chat.runningTurn': { en: 'Running turn', zh: '执行中' },
  'chat.working': { en: 'Working', zh: '执行中' },
  'chat.parallel': { en: 'parallel', zh: '并行' },
  'chat.copy': { en: 'Copy', zh: '复制' },
  'chat.retry': { en: 'Retry', zh: '重试' },
  'chat.delete': { en: 'Delete', zh: '删除' },

  // ---- Sessions ----
  'sessions.title': { en: 'Sessions', zh: '会话' },
  'sessions.recentWork': { en: 'Recent Work', zh: '最近工作' },
  'sessions.filter': { en: 'Filter sessions', zh: '筛选会话' },
  'sessions.none': { en: 'No sessions', zh: '暂无会话' },
  'sessions.deleteConfirm': { en: 'Delete this session? This cannot be undone.', zh: '删除此会话？此操作无法撤销。' },
  'sessions.loading': { en: 'Loading session', zh: '加载会话中' },
  'sessions.loadingRuntime': { en: 'Loading runtime', zh: '加载运行时' },
  'sessions.newSession': { en: 'New session', zh: '新建会话' },
  'sessions.tokens': { en: 'tokens', zh: '令牌数' },
  'sessions.tools': { en: 'tools', zh: '工具' },
  'sessions.sessions': { en: 'sessions', zh: '会话' },

  // ---- Right panel ----
  'panel.runtime': { en: 'Runtime', zh: '运行时' },
  'panel.tools': { en: 'Tools', zh: '工具' },
  'panel.skills': { en: 'Skills', zh: '技能' },
  'panel.server': { en: 'Server', zh: '服务器' },
  'panel.status': { en: 'Status', zh: '状态' },
  'panel.model': { en: 'Model', zh: '模型' },
  'panel.auth': { en: 'Auth', zh: '认证' },
  'panel.required': { en: 'required', zh: '必需' },
  'panel.open': { en: 'open', zh: '开放' },
  'panel.persistence': { en: 'Persistence', zh: '持久化' },
  'panel.resources': { en: 'Resources', zh: '资源' },
  'panel.capabilities': { en: 'Capabilities', zh: '能力' },
  'panel.sessionInspector': { en: 'Session Inspector', zh: '会话检查器' },
  'panel.noSession': { en: 'No session selected', zh: '未选择会话' },
  'panel.loadingMessages': { en: 'Loading messages', zh: '加载消息中' },
  'panel.toolRegistry': { en: 'Tool Registry', zh: '工具注册表' },
  'panel.filterTools': { en: 'Filter tools', zh: '筛选工具' },
  'panel.toolsets': { en: 'Toolsets', zh: '工具集' },
  'panel.activeSkills': { en: 'Active Skills', zh: '活跃技能' },
  'panel.none': { en: 'none', zh: '无' },
  'panel.skillCatalog': { en: 'Skill Catalog', zh: '技能目录' },
  'panel.enabled': { en: 'enabled', zh: '已启用' },
  'panel.off': { en: 'off', zh: '关闭' },
  'panel.unknown': { en: 'unknown', zh: '未知' },
  'panel.modelUnknown': { en: 'model unknown', zh: '模型未知' },
  'panel.webhook': { en: 'webhook', zh: 'webhook' },
  'panel.credentials': { en: 'credentials', zh: '凭证' },
  'panel.mcp': { en: 'MCP', zh: 'MCP' },

  // ---- Agent config form ----
  'persona.edit': { en: 'Edit agent', zh: '编辑员工' },
  'persona.new': { en: 'New agent', zh: '新建员工' },
  'persona.create': { en: 'Create an agent', zh: '创建员工' },
  'persona.cancel': { en: 'Cancel', zh: '取消' },
  'persona.save': { en: 'Save', zh: '保存' },
  'persona.deleteBtn': { en: 'Delete', zh: '删除' },
  'persona.identity': { en: 'Identity', zh: '身份' },
  'persona.id': { en: 'id', zh: 'id' },
  'persona.name': { en: 'name', zh: '名称' },
  'persona.avatarEmoji': { en: 'avatar (emoji)', zh: '头像 (emoji)' },
  'persona.description': { en: 'description', zh: '描述' },
  'persona.model': { en: 'Model', zh: '模型' },
  'persona.modelField': { en: 'model', zh: '模型' },
  'persona.inheritDefault': { en: '(inherit default)', zh: '(继承默认)' },
  'persona.reasoningEffort': { en: 'reasoning effort', zh: '推理力度' },
  'persona.default': { en: '(default)', zh: '(默认)' },
  'persona.isDefault': { en: 'Default agent (gateway fallback)', zh: '默认员工 (网关回退)' },
  'persona.addressable': { en: 'Allow other agents to consult this agent (team)', zh: '允许其他员工咨询此员工 (团队协作)' },
  'persona.proactiveDelegation': { en: 'Proactive delegation (auto-seek work from others)', zh: '主动委派 (自动寻找并分配工作给其他员工)' },
  'persona.systemPrompt': { en: 'System prompt', zh: '系统提示词' },
  'persona.identityPrompt': { en: 'identity prompt', zh: '身份提示词' },
  'persona.skills': { en: 'Skills', zh: '技能' },
  'persona.noSkills': { en: 'No skills available', zh: '暂无可用技能' },
  'persona.channelBindings': { en: 'Channel bindings', zh: '频道绑定' },
  'persona.bindingsHint': { en: 'one platform:bot_id per line (empty = WebUI only)', zh: '每行一个 platform:bot_id (为空 = 仅 WebUI)' },
  'persona.memory': { en: 'Memory', zh: '记忆' },
  'persona.memoryDir': { en: 'Memory directory', zh: '记忆目录' },
  'persona.memoryIndex': { en: 'Memory index', zh: '记忆索引' },
  'persona.noMemory': { en: 'No memory files', zh: '暂无记忆文件' },
  'persona.idError': { en: 'Agent id must match [a-z0-9][a-z0-9_-]{0,63}', zh: '员工 ID 必须匹配 [a-z0-9][a-z0-9_-]{0,63}' },

  // ---- Instance settings / Bindings ----
  'instance.routing': { en: 'Routing', zh: '路由' },
  'instance.channelBindings': { en: 'Channel bindings', zh: '频道绑定' },
  'instance.platformBotId': { en: 'platform:bot_id', zh: '平台:bot_id' },
  'instance.persona': { en: 'agent', zh: '员工' },
  'instance.noBindings': { en: 'No channel bindings. Unbound channels fall back to the default agent.', zh: '暂无频道绑定。未绑定的频道将使用默认员工。' },
  'instance.defaultPersona': { en: 'Default agent (fallback)', zh: '默认员工 (回退)' },
  'instance.addBinding': { en: 'Add binding', zh: '添加绑定' },
  'instance.editBinding': { en: 'Edit', zh: '编辑' },
  'instance.deleteBinding': { en: 'Delete', zh: '删除' },
  'instance.platform': { en: 'Platform', zh: '平台' },
  'instance.botIdLabel': { en: 'Instance name', zh: '实例名称' },
  'instance.botIdPlaceholder': { en: 'e.g. my_bot', zh: '例如 my_bot' },
  'instance.selectPersona': { en: 'Select agent', zh: '选择员工' },
  'instance.saveBinding': { en: 'Save', zh: '保存' },
  'instance.cancelBinding': { en: 'Cancel', zh: '取消' },
  'instance.deleteConfirm': { en: 'Remove this channel binding?', zh: '移除此频道绑定？' },
  'instance.actions': { en: 'actions', zh: '操作' },
  'instance.preview': { en: 'Binding key', zh: '绑定键' },

  // Platform display names
  'instance.platform.telegram': { en: 'Telegram', zh: 'Telegram' },
  'instance.platform.qqbot': { en: 'QQ Bot', zh: 'QQ 机器人' },
  'instance.platform.clawbot': { en: 'ClawBot (WeChat)', zh: 'ClawBot (微信)' },
  'instance.platform.weixin': { en: 'WeChat iLink', zh: '微信 iLink' },
  'instance.platform.discord': { en: 'Discord', zh: 'Discord' },
  'instance.platform.slack': { en: 'Slack', zh: 'Slack' },
  'instance.platform.dingtalk': { en: 'DingTalk', zh: '钉钉' },
  'instance.platform.feishu': { en: 'Feishu / Lark', zh: '飞书' },
  'instance.platform.wecom': { en: 'WeCom', zh: '企业微信' },
  'instance.platform.email': { en: 'Email', zh: '邮件' },
  'instance.platform.whatsapp': { en: 'WhatsApp', zh: 'WhatsApp' },
  'instance.platform.signal': { en: 'Signal', zh: 'Signal' },
  'instance.platform.matrix': { en: 'Matrix', zh: 'Matrix' },
  'instance.platform.mattermost': { en: 'Mattermost', zh: 'Mattermost' },
  'instance.platform.sms': { en: 'SMS', zh: '短信' },
  'instance.platform.bluebubbles': { en: 'BlueBubbles (iMessage)', zh: 'BlueBubbles (iMessage)' },
  'instance.platform.homeassistant': { en: 'Home Assistant', zh: 'Home Assistant' },
  'instance.platform.webhook': { en: 'Webhook', zh: 'Webhook' },
  'instance.platform.msgraph': { en: 'Microsoft Graph', zh: 'Microsoft Graph' },

  // Platform-specific hints explaining what the instance name means
  'instance.hint.telegram': { en: 'Instance name is a custom label (e.g. "devbot"). It matches the bot_id in your config.yaml gateway section, NOT the Telegram bot token or numeric ID.', zh: '实例名称为自定义标签（如 "devbot"），需与 config.yaml 中 gateway 配置的 bot_id 一致，不是 Telegram 的 bot token 或数字 ID。' },
  'instance.hint.qqbot': { en: 'Instance name is a custom label (e.g. "qqbot"). It matches the bot_id in your config.yaml qqbot gateway section.', zh: '实例名称为自定义标签（如 "qqbot"），需与 config.yaml 中 qqbot gateway 的 bot_id 一致。' },
  'instance.hint.clawbot': { en: 'Instance name is a custom label (e.g. "clawbot"). It matches the bot_id in your config.yaml clawbot gateway section.', zh: '实例名称为自定义标签（如 "clawbot"），需与 config.yaml 中 clawbot gateway 的 bot_id 一致。' },
  'instance.hint.weixin': { en: 'Instance name is a custom label for the WeChat iLink integration. It matches the bot_id in your config.yaml ilink gateway section.', zh: '实例名称为微信 iLink 集成的自定义标签，需与 config.yaml 中 ilink gateway 的 bot_id 一致。' },
  'instance.hint.discord': { en: 'Instance name is a custom label (e.g. "mybot"). It matches the bot_id in your config.yaml discord gateway section.', zh: '实例名称为自定义标签（如 "mybot"），需与 config.yaml 中 discord gateway 的 bot_id 一致。' },
  'instance.hint.slack': { en: 'Instance name is a custom label (e.g. "support"). It matches the bot_id in your config.yaml slack gateway section.', zh: '实例名称为自定义标签（如 "support"），需与 config.yaml 中 slack gateway 的 bot_id 一致。' },
  'instance.hint.dingtalk': { en: 'Instance name matches the bot_id in your config.yaml dingtalk gateway section.', zh: '实例名称需与 config.yaml 中 dingtalk gateway 的 bot_id 一致。' },
  'instance.hint.feishu': { en: 'Instance name matches the bot_id in your config.yaml feishu gateway section.', zh: '实例名称需与 config.yaml 中 feishu gateway 的 bot_id 一致。' },
  'instance.hint.wecom': { en: 'Instance name matches the bot_id in your config.yaml wecom gateway section.', zh: '实例名称需与 config.yaml 中 wecom gateway 的 bot_id 一致。' },
  'instance.hint.email': { en: 'Instance name matches the bot_id in your config.yaml email gateway section.', zh: '实例名称需与 config.yaml 中 email gateway 的 bot_id 一致。' },
  'instance.hint.whatsapp': { en: 'Instance name matches the bot_id in your config.yaml whatsapp gateway section.', zh: '实例名称需与 config.yaml 中 whatsapp gateway 的 bot_id 一致。' },
  'instance.hint.signal': { en: 'Instance name matches the bot_id in your config.yaml signal gateway section.', zh: '实例名称需与 config.yaml 中 signal gateway 的 bot_id 一致。' },
  'instance.hint.matrix': { en: 'Instance name matches the bot_id in your config.yaml matrix gateway section.', zh: '实例名称需与 config.yaml 中 matrix gateway 的 bot_id 一致。' },
  'instance.hint.mattermost': { en: 'Instance name matches the bot_id in your config.yaml mattermost gateway section.', zh: '实例名称需与 config.yaml 中 mattermost gateway 的 bot_id 一致。' },
  'instance.hint.sms': { en: 'Instance name matches the bot_id in your config.yaml sms gateway section.', zh: '实例名称需与 config.yaml 中 sms gateway 的 bot_id 一致。' },
  'instance.hint.bluebubbles': { en: 'Instance name matches the bot_id in your config.yaml bluebubbles gateway section.', zh: '实例名称需与 config.yaml 中 bluebubbles gateway 的 bot_id 一致。' },
  'instance.hint.homeassistant': { en: 'Instance name matches the bot_id in your config.yaml homeassistant gateway section.', zh: '实例名称需与 config.yaml 中 homeassistant gateway 的 bot_id 一致。' },
  'instance.hint.webhook': { en: 'Instance name matches the bot_id in your config.yaml webhook gateway section.', zh: '实例名称需与 config.yaml 中 webhook gateway 的 bot_id 一致。' },
  'instance.hint.msgraph': { en: 'Instance name matches the bot_id in your config.yaml msgraph gateway section.', zh: '实例名称需与 config.yaml 中 msgraph gateway 的 bot_id 一致。' },

  // ---- Gateway panel ----
  'gateway.title': { en: 'Gateway Management', zh: 'Gateway 管理' },
  'gateway.refresh': { en: 'Refresh', zh: '刷新' },
  'gateway.runningStatus': { en: 'Running Status', zh: '运行状态' },
  'gateway.status': { en: 'Gateway Status', zh: 'Gateway 状态' },
  'gateway.running': { en: 'Running', zh: '运行中' },
  'gateway.stopped': { en: 'Stopped', zh: '未运行' },
  'gateway.connectedPlatforms': { en: 'Connected Platforms', zh: '已连接平台' },
  'gateway.none': { en: 'None', zh: '无' },
  'gateway.totalMessages': { en: 'Total Messages', zh: '总消息数' },
  'gateway.restart': { en: 'Restart Gateway', zh: '重启 Gateway' },
  'gateway.restarting': { en: 'Restarting...', zh: '重启中...' },
  'gateway.restartSent': { en: 'Gateway restart request sent', zh: 'Gateway 重启请求已发送' },
  'gateway.config': { en: 'Configuration', zh: '配置管理' },
  'gateway.busyMode': { en: 'Busy input mode', zh: '繁忙输入模式' },
  'gateway.parallel': { en: 'Parallel (parallel)', zh: '并行模式 (parallel)' },
  'gateway.queue': { en: 'Queue (queue)', zh: '队列模式 (queue)' },
  'gateway.interrupt': { en: 'Interrupt (interrupt)', zh: '中断模式 (interrupt)' },
  'gateway.busyHint': { en: 'Parallel: all messages run concurrently. Queue: new messages wait. Interrupt: new messages cancel current task.', zh: '并行模式：所有消息独立并发执行。队列模式：新消息排队等待。中断模式：新消息取消当前任务。' },
  'gateway.allowAll': { en: 'Allow all users', zh: '允许所有用户访问' },
  'gateway.allowAllHint': { en: 'When enabled, all users can use Gateway. When disabled, only whitelisted users.', zh: '启用后，所有用户都可以使用 Gateway。禁用后仅限白名单用户。' },
  'gateway.whitelist': { en: 'Whitelisted users', zh: '白名单用户' },
  'gateway.whitelistPlaceholder': { en: 'One user ID or username per line', zh: '每行一个用户 ID 或用户名' },
  'gateway.whitelistHint': { en: 'One user ID or username per line (e.g. telegram:123456789)', zh: '每行一个用户 ID 或用户名（例如：telegram:123456789）' },
  'gateway.filterNarration': { en: 'Filter narration text', zh: '过滤叙述性文本' },
  'gateway.filterNarrationHint': { en: 'Remove "executing...", "completed..." and other narration from responses.', zh: '移除响应中的 "正在执行..."、"已完成..." 等叙述性内容。' },
  'gateway.saveConfig': { en: 'Save config', zh: '保存配置' },
  'gateway.saving': { en: 'Saving...', zh: '保存中...' },
  'gateway.configSaved': { en: 'Config saved', zh: '配置已保存' },
  'gateway.loadingStatus': { en: 'Loading Gateway status...', zh: '加载 Gateway 状态...' },
  'gateway.enabledPlatforms': { en: 'Enabled Platforms', zh: '已启用平台' },
  'gateway.disabledPlatforms': { en: 'Available Platforms', zh: '可用平台' },
  'gateway.noEnabled': { en: 'No platforms enabled yet', zh: '尚无启用的平台' },
  'gateway.editPlatform': { en: 'Edit platform config', zh: '编辑平台配置' },
  'gateway.platformConfig': { en: 'Platform Configuration', zh: '平台配置' },
  'gateway.savedNeedRestart': { en: 'Config saved. Restart Gateway to apply changes.', zh: '配置已保存，需重启 Gateway 生效。' },
  'gateway.onePerLine': { en: 'One item per line', zh: '每行一项' },
  'gateway.enterSecret': { en: 'Enter new value (leave empty to keep current)', zh: '输入新值（留空保持当前值）' },
  'gateway.secretHint': { en: 'Leave empty to keep existing value', zh: '留空保持现有值' },

  // ---- Settings panel ----
  'settings.controlCenter': { en: 'Control Center', zh: '控制中心' },
  'settings.runtimeConfig': { en: 'Runtime Configuration', zh: '运行时配置' },
  'settings.save': { en: 'Save', zh: '保存' },
  'settings.saving': { en: 'Saving', zh: '保存中' },
  'settings.updated': { en: 'Runtime config updated', zh: '运行时配置已更新' },

  // ---- Language ----
  'lang.switch': { en: '中文', zh: 'EN' },
  'lang.tooltip': { en: 'Switch to Chinese', zh: 'Switch to English' },
} satisfies Record<string, Entry>;

export type MessageKey = keyof typeof messages;

export type Lang = 'en' | 'zh';

interface I18nContextValue {
  lang: Lang;
  t: (key: MessageKey) => string;
  setLang: (lang: Lang) => void;
}

const LANG_STORAGE_KEY = 'hakimi-webui-lang';

function detectLang(): Lang {
  const stored = typeof localStorage !== 'undefined' ? localStorage.getItem(LANG_STORAGE_KEY) : null;
  if (stored === 'en' || stored === 'zh') return stored;
  const nav = typeof navigator !== 'undefined' ? navigator.language : 'en';
  return nav.startsWith('zh') ? 'zh' : 'en';
}

const I18nContext = createContext<I18nContextValue>({
  lang: 'en',
  t: (key) => messages[key]?.en ?? key,
  setLang: () => {},
});

// eslint-disable-next-line react-refresh/only-export-components
export function useI18n() {
  return useContext(I18nContext);
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(detectLang);

  const setLang = useCallback((next: Lang) => {
    setLangState(next);
    try {
      localStorage.setItem(LANG_STORAGE_KEY, next);
    } catch {
      // localStorage may be unavailable
    }
  }, []);

  const t = useCallback(
    (key: MessageKey): string => messages[key]?.[lang] ?? messages[key]?.en ?? key,
    [lang],
  );

  return <I18nContext.Provider value={{ lang, t, setLang }}>{children}</I18nContext.Provider>;
}
