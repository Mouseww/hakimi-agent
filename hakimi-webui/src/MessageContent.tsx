import type { MouseEvent } from 'react';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import hljs from './highlighter';

interface MessageContentProps {
  content: string;
}

// Open links in a new tab without overriding react-markdown's `a` renderer
// (which would force handling its `node` prop). Delegation keeps it simple.
function openLinksInNewTab(event: MouseEvent<HTMLDivElement>) {
  const anchor = (event.target as HTMLElement).closest('a');
  if (anchor instanceof HTMLAnchorElement && anchor.href) {
    event.preventDefault();
    window.open(anchor.href, '_blank', 'noopener,noreferrer');
  }
}

/**
 * Strip internal tool markers that may leak into assistant content during streaming.
 * These markers (hakimi_tool:, hakimi_tool_result:) are protocol-level metadata that
 * should not be displayed to the user.
 */
function stripToolMarkers(text: string): string {
  return text
    .replace(/hakimi_tool:.*?(?=\n|$)/g, '')
    .replace(/hakimi_tool_result:.*?(?=\n|$)/g, '')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}

/**
 * Renders message text as GitHub-flavored Markdown. Fenced code blocks with a
 * known language are syntax-highlighted via the shared highlight.js instance.
 * react-markdown escapes raw HTML by default (no rehype-raw), so this is XSS-safe.
 */
export default function MessageContent({ content }: MessageContentProps) {
  const cleanContent = stripToolMarkers(content);

  return (
    <div className="markdown-body" onClick={openLinksInNewTab}>
      <Markdown
        remarkPlugins={[remarkGfm]}
        components={{
          code({ className, children }) {
            const match = /language-([\w-]+)/.exec(className ?? '');
            const language = match?.[1];
            if (language && hljs.getLanguage(language)) {
              const text = String(children ?? '').replace(/\n$/, '');
              const html = hljs.highlight(text, { language }).value;
              return (
                <code
                  className={`hljs language-${language}`}
                  dangerouslySetInnerHTML={{ __html: html }}
                />
              );
            }
            return <code className={className}>{children}</code>;
          },
        }}
      >
        {cleanContent}
      </Markdown>
    </div>
  );
}
