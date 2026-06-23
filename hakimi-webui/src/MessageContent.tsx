import type { MouseEvent } from 'react';
import Markdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

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
 * Renders message text as GitHub-flavored Markdown. react-markdown escapes raw
 * HTML by default (no rehype-raw), so this is XSS-safe.
 */
export default function MessageContent({ content }: MessageContentProps) {
  return (
    <div className="markdown-body" onClick={openLinksInNewTab}>
      <Markdown remarkPlugins={[remarkGfm]}>{content}</Markdown>
    </div>
  );
}
