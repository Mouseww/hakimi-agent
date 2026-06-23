declare module 'lucide-react' {
  import type { FC, SVGProps } from 'react';

  export type LucideProps = SVGProps<SVGSVGElement> & {
    size?: number | string;
    absoluteStrokeWidth?: boolean;
  };

  export const Activity: FC<LucideProps>;
  export const BadgeCheck: FC<LucideProps>;
  export const Bot: FC<LucideProps>;
  export const Boxes: FC<LucideProps>;
  export const Brain: FC<LucideProps>;
  export const Database: FC<LucideProps>;
  export const FileSearch: FC<LucideProps>;
  export const Gauge: FC<LucideProps>;
  export const KeyRound: FC<LucideProps>;
  export const Layers3: FC<LucideProps>;
  export const Loader2: FC<LucideProps>;
  export const MessageSquare: FC<LucideProps>;
  export const Plus: FC<LucideProps>;
  export const RefreshCcw: FC<LucideProps>;
  export const Save: FC<LucideProps>;
  export const Search: FC<LucideProps>;
  export const Send: FC<LucideProps>;
  export const Server: FC<LucideProps>;
  export const Settings: FC<LucideProps>;
  export const Share2: FC<LucideProps>;
  export const Shield: FC<LucideProps>;
  export const ShieldCheck: FC<LucideProps>;
  export const SlidersHorizontal: FC<LucideProps>;
  export const SquareTerminal: FC<LucideProps>;
  export const Terminal: FC<LucideProps>;
  export const Trash2: FC<LucideProps>;
  export const Workflow: FC<LucideProps>;
  export const Wrench: FC<LucideProps>;
  export const X: FC<LucideProps>;
}
