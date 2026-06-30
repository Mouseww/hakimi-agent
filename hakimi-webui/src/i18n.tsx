import { createContext, useContext, type ReactNode } from 'react';

type Entry = { en: string; zh: string };

const messages = {
  'office.live': { en: 'Live', zh: '实时' },
  'office.offline': { en: 'Offline (reconnecting)', zh: '离线(重连中)' },
  'office.clickHint': { en: 'Click a desk to open chat', zh: '点击工位进入对话' },
  'office.team': { en: 'Team', zh: '组队' },
  'office.empty': { en: 'No personas yet', zh: '暂无人格' },
  'office.state.idle': { en: 'idle', zh: '空闲' },
  'office.state.working': { en: 'working', zh: '执行中' },
  'office.state.consulting': { en: 'delegating', zh: '委派中' },
  'office.state.in_team': { en: 'in a team', zh: '组队中' },
  'office.delegatedBy': { en: 'Delegated by', zh: '受委派自' },
} satisfies Record<string, Entry>;

type MessageKey = keyof typeof messages;

type Lang = 'en' | 'zh';

interface I18nContextValue {
  lang: Lang;
  t: (key: MessageKey) => string;
}

const I18nContext = createContext<I18nContextValue>({
  lang: 'en',
  t: (key) => messages[key]?.en ?? key,
});

function detectLang(): Lang {
  const nav = typeof navigator !== 'undefined' ? navigator.language : 'en';
  return nav.startsWith('zh') ? 'zh' : 'en';
}

// eslint-disable-next-line react-refresh/only-export-components
export function useI18n() {
  return useContext(I18nContext);
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const lang = detectLang();
  const t = (key: MessageKey): string => messages[key]?.[lang] ?? messages[key]?.en ?? key;
  return <I18nContext.Provider value={{ lang, t }}>{children}</I18nContext.Provider>;
}
