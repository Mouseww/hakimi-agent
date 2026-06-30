# Office Dashboard UI (frontend) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a cartoon "office" dashboard view to the WebUI that renders each persona as an employee at a desk, animating their real-time work state (working/typing, idle/gaming, jogging to deliver a consult, sitting together as a team, new-hire arrival) driven by the backend `/api/activity` feed.

**Architecture:** A new top-level `office` view in `App.tsx`. A `useActivityStream` hook seeds from `GET /api/activity/snapshot` and applies live `GET /api/activity/stream` SSE deltas through a pure reducer (`officeState.ts`) that mirrors the backend base+overlay state machine. A pure `officeLayout.ts` assigns stable desk seats. `OfficeView.tsx` renders the floor, `PersonaDesk.tsx` renders each flat-vector desk/character by state, a transient layer animates consult "runners" and team clusters. Clicking a desk opens that persona's chat (reuses existing `handleSelectPersona`); hovering shows a detail card.

**Tech Stack:** React 19 + TypeScript + Vite 8 (`hakimi-webui/`), flat-vector inline SVG + CSS keyframes, fetch-based SSE (matches existing `streamAgentChat`). Tests via vitest (added here) for the two pure modules. Gate: `npm run lint` + `npm run build` (tsc + vite); rebuild + commit `crates/hakimi-webui/static/`.

**Depends on:** Plan `2026-06-26-persona-activity-feed.md` (the `/api/activity/snapshot` + `/api/activity/stream` endpoints and the `ActivityEvent` shape). Implement that plan first.

**Spec:** `docs/superpowers/specs/2026-06-26-persona-office-dashboard-design.md`. Visual reference: the approved mockups `persona_office_concept` and `office_proposed_layout` (flat vector, slight top-down; lit screen + typing = working; game screen = idle; runner + 📋 = consult; clustered + ring = team; dashed desk = new hire).

**Run commands (from `hakimi-webui/`):** `npm run lint`, `npm run build`, `npm run test` (added in Task 2). These are local npm (not Docker).

---

## File Structure

**New (all under `hakimi-webui/src/`):**
- `activityTypes.ts` — `ActivityEvent`, `PersonaState`, `PersonaActivity` TS types (mirror the Rust serde shapes).
- `officeLayout.ts` — pure stable seat assignment.
- `officeState.ts` — pure activity reducer (`seedOffice`, `reduceActivity`, `displayedState`) mirroring the backend base+overlay machine.
- `useActivityStream.ts` — hook: snapshot + SSE + reconnect.
- `PersonaDesk.tsx` — one desk + flat-vector persona sprite, rendered by state.
- `OfficeView.tsx` — floor container: layout, desks, consult-runner layer, team clusters, hover card, click-through.
- `office.css` — office-specific styles + animation keyframes.
- `officeLayout.test.ts`, `officeState.test.ts` — vitest unit tests.

**Modified:**
- `api.ts` — `activitySnapshot()` + `streamActivity()` (SSE consumer).
- `App.tsx` — add `'office'` to the `view` union, render `OfficeView`, pass `onOffice`.
- `PersonaRail.tsx` — add `view='office'` to its prop union + an office nav button (`onOffice`).
- `i18n.tsx` — office message keys.
- `package.json` / `vitest.config.ts` — vitest devDep + `test` script + config.
- `crates/hakimi-webui/static/` — rebuilt embedded bundle.

---

## Task 1: Activity API types + client (snapshot + SSE)

**Files:**
- Create: `hakimi-webui/src/activityTypes.ts`
- Modify: `hakimi-webui/src/api.ts`

- [ ] **Step 1: Create `activityTypes.ts`** (mirrors the Rust serde shapes from the backend plan):

```ts
export type PersonaState = 'idle' | 'working' | 'consulting' | 'in_team';

export interface PersonaActivity {
  id: string;
  name: string;
  avatar: string;
  state: PersonaState;
  task_hint?: string;
  model?: string;
  team_id?: string;
}

export interface ActivitySnapshotResponse {
  personas: PersonaActivity[];
}

// Discriminated union matching #[serde(tag = "type", rename_all = "snake_case")].
export type ActivityEvent =
  | { type: 'persona_created'; id: string; name: string; avatar: string }
  | { type: 'persona_updated'; id: string; name: string; avatar: string }
  | { type: 'persona_deleted'; id: string }
  | { type: 'turn_started'; persona_id: string; task_hint?: string | null; model?: string | null }
  | { type: 'turn_ended'; persona_id: string }
  | { type: 'consult_started'; from_id: string; to_id: string; task_hint?: string | null }
  | { type: 'consult_ended'; from_id: string; to_id: string }
  | { type: 'team_formed'; team_id: string; lead_id: string; member_ids: string[]; task_hint?: string | null }
  | { type: 'team_disbanded'; team_id: string };
```

- [ ] **Step 2: Add the snapshot fetch + SSE consumer to `api.ts`.** First add the import at the top of `api.ts` (near the other imports):

```ts
import type { ActivityEvent, ActivitySnapshotResponse } from './activityTypes';
```

Add `activitySnapshot` to the `api` object (after `workspaceRead`):

```ts
  activitySnapshot: () => request<ActivitySnapshotResponse>('/api/activity/snapshot'),
```

Then add a fetch-based SSE consumer at the end of the file (mirrors `streamAgentChat`'s reader/parse, but parses `event: activity` frames and runs until aborted):

```ts
/**
 * Consume the persona-activity SSE stream, calling `onEvent` for each event.
 * Resolves when the stream ends or `signal` aborts. Mirrors streamAgentChat's
 * fetch-based SSE parsing so the Bearer token is sent (EventSource can't).
 */
export async function streamActivity(opts: {
  onEvent: (event: ActivityEvent) => void;
  signal: AbortSignal;
}): Promise<void> {
  const token = getAuthToken();
  const headers: Record<string, string> = {};
  if (token) {
    headers.Authorization = `Bearer ${token}`;
  }

  const response = await fetch('/api/activity/stream', { headers, signal: opts.signal });
  if (!response.ok || !response.body) {
    throw new Error(`activity stream ${response.status} ${response.statusText}`);
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';

  for (;;) {
    const { done, value } = await reader.read();
    if (done) {
      break;
    }
    buffer += decoder.decode(value, { stream: true });
    // SSE frames are separated by a blank line.
    let sep = buffer.indexOf('\n\n');
    while (sep !== -1) {
      const frame = buffer.slice(0, sep);
      buffer = buffer.slice(sep + 2);
      const dataLine = frame
        .split('\n')
        .find((line) => line.startsWith('data:'));
      if (dataLine) {
        const json = dataLine.slice(5).trim();
        try {
          opts.onEvent(JSON.parse(json) as ActivityEvent);
        } catch {
          // ignore malformed frame
        }
      }
      sep = buffer.indexOf('\n\n');
    }
  }
}
```

- [ ] **Step 3: Verify types compile.**

Run (from `hakimi-webui/`): `npm run build`
Expected: `tsc -b` passes and `vite build` succeeds (no usage yet beyond the new exports).

- [ ] **Step 4: Commit.**

```bash
git add hakimi-webui/src/activityTypes.ts hakimi-webui/src/api.ts
git commit -m "feat(webui): activity API types + snapshot + SSE consumer"
```

---

## Task 2: Add vitest

**Files:**
- Modify: `hakimi-webui/package.json`
- Create: `hakimi-webui/vitest.config.ts`

- [ ] **Step 1: Install vitest.**

Run (from `hakimi-webui/`): `npm install -D vitest`
(If a peer-dependency conflict with the installed Vite major is reported, pin to the matching major, e.g. `npm install -D vitest@<major-matching-vite>`.)

- [ ] **Step 2: Add the `test` script** to `package.json` `scripts`:

```json
    "test": "vitest run",
```

- [ ] **Step 3: Create `vitest.config.ts`** (Node env is fine; the two tested modules are pure, no DOM):

```ts
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },
});
```

- [ ] **Step 4: Verify the runner works** (no tests yet → vitest exits 0 with "no test files" or similar; that's fine).

Run (from `hakimi-webui/`): `npm run test`
Expected: runs without error (exit 0). If it errors on "no test files found", that is acceptable; the next task adds tests.

- [ ] **Step 5: Commit.**

```bash
git add hakimi-webui/package.json hakimi-webui/package-lock.json hakimi-webui/vitest.config.ts
git commit -m "chore(webui): add vitest for pure-module unit tests"
```

---

## Task 3: Pure seat-layout engine

**Files:**
- Create: `hakimi-webui/src/officeLayout.ts`
- Test: `hakimi-webui/src/officeLayout.test.ts`

- [ ] **Step 1: Write the failing test** (`officeLayout.test.ts`):

```ts
import { describe, expect, it } from 'vitest';
import { assignSeats } from './officeLayout';

describe('assignSeats', () => {
  it('places ids in row-major order with stable coordinates', () => {
    const layout = assignSeats(['a', 'b', 'c'], undefined, 2);
    expect(layout.seats.get('a')).toMatchObject({ row: 0, col: 0 });
    expect(layout.seats.get('b')).toMatchObject({ row: 0, col: 1 });
    expect(layout.seats.get('c')).toMatchObject({ row: 1, col: 0 });
    // coordinates are derived and stable
    expect(layout.seats.get('a')!.x).toBeLessThan(layout.seats.get('b')!.x);
    expect(layout.seats.get('c')!.y).toBeGreaterThan(layout.seats.get('a')!.y);
  });

  it('keeps existing seats and fills freed gaps for new ids', () => {
    const first = assignSeats(['a', 'b', 'c'], undefined, 2);
    // 'b' leaves; 'd' joins -> 'd' should take b's freed slot (row0,col1), a & c stay put
    const second = assignSeats(['a', 'c', 'd'], first.seats, 2);
    expect(second.seats.get('a')).toMatchObject({ row: 0, col: 0 });
    expect(second.seats.get('c')).toMatchObject({ row: 1, col: 0 });
    expect(second.seats.get('d')).toMatchObject({ row: 0, col: 1 });
  });
});
```

- [ ] **Step 2: Run it to confirm it fails.**

Run (from `hakimi-webui/`): `npm run test`
Expected: FAIL (`assignSeats` not found).

- [ ] **Step 3: Implement `officeLayout.ts`:**

```ts
export interface Seat {
  id: string;
  row: number;
  col: number;
  x: number; // top-left desk x, in layout units
  y: number;
}

export interface OfficeLayout {
  seats: Map<string, Seat>;
  cols: number;
  rows: number;
}

export const CELL_W = 150;
export const CELL_H = 130;
const PAD = 24;

function coords(row: number, col: number): { x: number; y: number } {
  return { x: PAD + col * CELL_W, y: PAD + row * CELL_H };
}

/**
 * Assign each id a desk seat in a `cols`-wide grid, row-major. Stable: ids present
 * in `prev` keep their seat; new ids fill the lowest-index free slot (so a freed
 * desk is reused before growing). Pure.
 */
export function assignSeats(
  ids: string[],
  prev?: Map<string, Seat>,
  cols = 4,
): OfficeLayout {
  const seats = new Map<string, Seat>();
  const taken = new Set<number>(); // flat slot index = row * cols + col
  const idSet = new Set(ids);

  // 1. keep stable seats for surviving ids
  if (prev) {
    for (const id of ids) {
      const p = prev.get(id);
      if (p) {
        const slot = p.row * cols + p.col;
        seats.set(id, p);
        taken.add(slot);
      }
    }
  }

  // 2. assign new ids to the lowest free slot
  let next = 0;
  for (const id of ids) {
    if (seats.has(id)) {
      continue;
    }
    while (taken.has(next)) {
      next += 1;
    }
    const row = Math.floor(next / cols);
    const col = next % cols;
    const { x, y } = coords(row, col);
    seats.set(id, { id, row, col, x, y });
    taken.add(next);
  }

  // 3. drop seats for ids no longer present (already excluded by construction)
  for (const id of Array.from(seats.keys())) {
    if (!idSet.has(id)) {
      seats.delete(id);
    }
  }

  const maxSlot = Math.max(0, ...Array.from(seats.values()).map((s) => s.row * cols + s.col));
  const rows = Math.floor(maxSlot / cols) + 1;
  return { seats, cols, rows };
}
```

- [ ] **Step 4: Run the test to confirm it passes.**

Run (from `hakimi-webui/`): `npm run test`
Expected: PASS.

- [ ] **Step 5: Commit.**

```bash
git add hakimi-webui/src/officeLayout.ts hakimi-webui/src/officeLayout.test.ts
git commit -m "feat(webui): pure stable office seat-layout engine"
```

---

## Task 4: Pure activity reducer (base + overlay state machine)

**Files:**
- Create: `hakimi-webui/src/officeState.ts`
- Test: `hakimi-webui/src/officeState.test.ts`

This mirrors the backend `apply`/`displayed_state` (base `working` + overlays `consulting`/`team`) so live SSE deltas stay consistent with the backend.

- [ ] **Step 1: Write the failing test** (`officeState.test.ts`):

```ts
import { describe, expect, it } from 'vitest';
import { displayedState, reduceActivity, seedOffice, type DeskState } from './officeState';

function desk(state: Partial<DeskState>): DeskState {
  return { id: 'x', name: 'X', avatar: '🙂', working: false, ...state };
}

describe('officeState', () => {
  it('seeds desks from a snapshot', () => {
    const s = seedOffice([{ id: 'a', name: 'A', avatar: '🤖', state: 'working' }]);
    expect(s.get('a')!.working).toBe(true);
    expect(displayedState(s.get('a')!)).toBe('working');
  });

  it('turn start/end toggles working', () => {
    let s = new Map<string, DeskState>([['a', desk({ id: 'a' })]]);
    s = reduceActivity(s, { type: 'turn_started', persona_id: 'a', task_hint: 'fix', model: 'opus' });
    expect(displayedState(s.get('a')!)).toBe('working');
    expect(s.get('a')!.taskHint).toBe('fix');
    s = reduceActivity(s, { type: 'turn_ended', persona_id: 'a' });
    expect(displayedState(s.get('a')!)).toBe('idle');
  });

  it('consult overlays working and restores it on end', () => {
    let s = new Map<string, DeskState>([['a', desk({ id: 'a', working: true })]]);
    s = reduceActivity(s, { type: 'consult_started', from_id: 'a', to_id: 'b', task_hint: null });
    expect(displayedState(s.get('a')!)).toBe('consulting');
    expect(s.get('a')!.consultingTo).toBe('b');
    s = reduceActivity(s, { type: 'consult_ended', from_id: 'a', to_id: 'b' });
    expect(displayedState(s.get('a')!)).toBe('working'); // base preserved
  });

  it('team masks other states until disbanded', () => {
    let s = new Map<string, DeskState>([
      ['a', desk({ id: 'a', working: true })],
      ['b', desk({ id: 'b' })],
    ]);
    s = reduceActivity(s, { type: 'team_formed', team_id: 't1', lead_id: 'a', member_ids: ['b'], task_hint: null });
    expect(displayedState(s.get('a')!)).toBe('in_team');
    expect(displayedState(s.get('b')!)).toBe('in_team');
    s = reduceActivity(s, { type: 'team_disbanded', team_id: 't1' });
    expect(displayedState(s.get('a')!)).toBe('working');
    expect(displayedState(s.get('b')!)).toBe('idle');
  });

  it('persona_created adds a desk, persona_deleted removes it', () => {
    let s = new Map<string, DeskState>();
    s = reduceActivity(s, { type: 'persona_created', id: 'a', name: 'A', avatar: '🤖' });
    expect(s.has('a')).toBe(true);
    s = reduceActivity(s, { type: 'persona_deleted', id: 'a' });
    expect(s.has('a')).toBe(false);
  });
});
```

- [ ] **Step 2: Run it to confirm it fails.**

Run (from `hakimi-webui/`): `npm run test`
Expected: FAIL (`officeState` not found).

- [ ] **Step 3: Implement `officeState.ts`:**

```ts
import type { ActivityEvent, PersonaActivity, PersonaState } from './activityTypes';

export interface DeskState {
  id: string;
  name: string;
  avatar: string;
  working: boolean;            // base
  consultingTo?: string;       // overlay
  teamId?: string;             // overlay
  taskHint?: string;
  model?: string;
}

export type OfficeState = Map<string, DeskState>;

/** Displayed state from base + overlay (priority: in_team > consulting > working > idle). */
export function displayedState(desk: DeskState): PersonaState {
  if (desk.teamId) return 'in_team';
  if (desk.consultingTo) return 'consulting';
  if (desk.working) return 'working';
  return 'idle';
}

/** Build initial office state from the snapshot rows (displayed-state approximation). */
export function seedOffice(rows: PersonaActivity[]): OfficeState {
  const map: OfficeState = new Map();
  for (const r of rows) {
    map.set(r.id, {
      id: r.id,
      name: r.name,
      avatar: r.avatar,
      working: r.state === 'working',
      consultingTo: r.state === 'consulting' ? '?' : undefined,
      teamId: r.team_id ?? (r.state === 'in_team' ? '?' : undefined),
      taskHint: r.task_hint,
      model: r.model,
    });
  }
  return map;
}

function ensure(map: OfficeState, id: string): DeskState {
  let d = map.get(id);
  if (!d) {
    d = { id, name: id, avatar: '', working: false };
    map.set(id, d);
  }
  return d;
}

/** Apply one event, returning a NEW map (immutable update). */
export function reduceActivity(state: OfficeState, event: ActivityEvent): OfficeState {
  const map: OfficeState = new Map(state);
  const set = (id: string, patch: Partial<DeskState>) => {
    const base = map.get(id) ?? { id, name: id, avatar: '', working: false };
    map.set(id, { ...base, ...patch });
  };

  switch (event.type) {
    case 'persona_created':
    case 'persona_updated': {
      const prev = map.get(event.id);
      set(event.id, { name: event.name, avatar: event.avatar, working: prev?.working ?? false });
      break;
    }
    case 'persona_deleted':
      map.delete(event.id);
      break;
    case 'turn_started':
      set(event.persona_id, {
        working: true,
        taskHint: event.task_hint ?? undefined,
        model: event.model ?? undefined,
      });
      break;
    case 'turn_ended':
      set(event.persona_id, { working: false, taskHint: undefined, model: undefined });
      break;
    case 'consult_started':
      set(event.from_id, { consultingTo: event.to_id });
      break;
    case 'consult_ended':
      set(event.from_id, { consultingTo: undefined });
      break;
    case 'team_formed':
      for (const id of [event.lead_id, ...event.member_ids]) {
        set(id, { teamId: event.team_id });
      }
      break;
    case 'team_disbanded':
      for (const [id, d] of map) {
        if (d.teamId === event.team_id) {
          map.set(id, { ...d, teamId: undefined });
        }
      }
      break;
  }
  // touch `ensure` to keep it used if no branch hit (defensive; tree-shaken otherwise)
  void ensure;
  return map;
}
```

(Note: `ensure` is included for symmetry with the backend's lazy-insert but the `set` helper already handles missing ids; if eslint flags it as unused, delete the `ensure` function and the `void ensure;` line.)

- [ ] **Step 4: Run the test to confirm it passes.**

Run (from `hakimi-webui/`): `npm run test`
Expected: PASS (both `officeLayout` and `officeState` suites).

- [ ] **Step 5: Commit.**

```bash
git add hakimi-webui/src/officeState.ts hakimi-webui/src/officeState.test.ts
git commit -m "feat(webui): pure activity reducer mirroring backend state machine"
```

---

## Task 5: `useActivityStream` hook (snapshot + SSE + reconnect)

**Files:**
- Create: `hakimi-webui/src/useActivityStream.ts`

- [ ] **Step 1: Implement the hook.**

```ts
import { useEffect, useRef, useState } from 'react';
import { api, streamActivity } from './api';
import type { ActivityEvent } from './activityTypes';
import { reduceActivity, seedOffice, type OfficeState } from './officeState';

export interface ActivityStream {
  office: OfficeState;
  connected: boolean;
}

/**
 * Seed the office from the snapshot, then apply live SSE deltas. Reconnects with
 * backoff; on each (re)connect it re-seeds from the snapshot to resync after any
 * dropped events.
 */
export function useActivityStream(enabled: boolean): ActivityStream {
  const [office, setOffice] = useState<OfficeState>(new Map());
  const [connected, setConnected] = useState(false);

  // keep latest office in a ref so the SSE callback reduces onto current state
  const officeRef = useRef<OfficeState>(office);
  officeRef.current = office;

  useEffect(() => {
    if (!enabled) {
      return;
    }
    let cancelled = false;
    const controller = new AbortController();
    let backoff = 1000;

    const apply = (event: ActivityEvent) => {
      const next = reduceActivity(officeRef.current, event);
      officeRef.current = next;
      setOffice(next);
    };

    async function run() {
      while (!cancelled) {
        try {
          const snap = await api.activitySnapshot();
          if (cancelled) return;
          const seeded = seedOffice(snap.personas);
          officeRef.current = seeded;
          setOffice(seeded);
          setConnected(true);
          backoff = 1000;
          await streamActivity({ onEvent: apply, signal: controller.signal });
        } catch {
          if (cancelled) return;
          setConnected(false);
        }
        if (cancelled) return;
        await new Promise((r) => setTimeout(r, backoff));
        backoff = Math.min(backoff * 2, 15000);
      }
    }

    void run();
    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [enabled]);

  return { office, connected };
}
```

- [ ] **Step 2: Verify it compiles.**

Run (from `hakimi-webui/`): `npm run build`
Expected: passes (the hook is exported but not yet used; tsc is fine with that).

- [ ] **Step 3: Commit.**

```bash
git add hakimi-webui/src/useActivityStream.ts
git commit -m "feat(webui): useActivityStream hook (snapshot + SSE + reconnect)"
```

---

## Task 6: `PersonaDesk` component + office CSS (flat-vector sprite by state)

**Files:**
- Create: `hakimi-webui/src/PersonaDesk.tsx`
- Create: `hakimi-webui/src/office.css`

Visual reference: the approved `office_proposed_layout` mockup. Working = lit screen (pulsing) + typing hands; idle = game screen (hopping blocks); consulting/in_team desks are dimmed (the motion is rendered by `OfficeView`'s overlay layer, not the desk itself).

- [ ] **Step 1: Create `office.css`** (animation keyframes + layout, theming via existing app CSS variables where available):

```css
.office-view { position: relative; width: 100%; height: 100%; overflow: auto; padding: 8px; }
.office-floor { position: relative; }
.office-hint { font-size: 12px; color: var(--muted, #8a7a5f); margin: 4px 8px; }

.persona-desk { position: absolute; width: 132px; height: 112px; cursor: pointer; }
.persona-desk .desk-name { font-size: 12px; font-weight: 600; text-align: center; }
.persona-desk.is-dimmed { opacity: 0.45; }

.screen-lit { fill: #8be0ff; animation: deskGlow 1.6s ease-in-out infinite; }
@keyframes deskGlow { 0%,100% { fill: #7fd6f5; } 50% { fill: #caf2ff; } }
.screen-game { fill: #26304a; }
.game-block { animation: deskHop 1s steps(2) infinite; }
.game-block.b2 { animation-duration: 1.3s; }
.typing-hand { animation: deskTap .35s ease-in-out infinite alternate; }
.typing-hand.h2 { animation-delay: .17s; }
@keyframes deskTap { from { transform: translateY(0); } to { transform: translateY(3px); } }
@keyframes deskHop { 0%,100% { transform: translateY(0); } 50% { transform: translateY(7px); } }

.office-runner { position: absolute; transition: transform 1.4s ease-in-out; will-change: transform; pointer-events: none; }
.office-team-ring { position: absolute; border: 2.5px solid var(--accent, #c98a2b); background: rgba(251,244,230,.5); border-radius: 16px; pointer-events: none; }
.office-team-label { position: absolute; font-size: 12px; font-weight: 700; color: var(--accent, #9a6a16); }

.office-card { position: absolute; z-index: 5; background: var(--card, #fffaf0); border: 1px solid var(--accent, #c98a2b);
  border-radius: 9px; padding: 8px 10px; font-size: 12px; box-shadow: 0 4px 14px rgba(0,0,0,.12); pointer-events: none; }
.office-card strong { display: block; font-size: 13px; }
.office-card .muted { color: var(--muted, #6b5b45); }

@media (prefers-reduced-motion: reduce) {
  .screen-lit, .game-block, .typing-hand, .office-runner { animation: none; transition: none; }
}
```

- [ ] **Step 2: Create `PersonaDesk.tsx`:**

```tsx
import type { DeskState } from './officeState';
import { displayedState } from './officeState';

interface PersonaDeskProps {
  desk: DeskState;
  x: number;
  y: number;
  onOpen: (id: string) => void;
  onHover: (id: string | null) => void;
}

function avatarText(desk: DeskState): string {
  if (desk.avatar.trim()) return desk.avatar.trim().slice(0, 2);
  return (desk.name.trim() || desk.id).slice(0, 1).toUpperCase();
}

export default function PersonaDesk({ desk, x, y, onOpen, onHover }: PersonaDeskProps) {
  const state = displayedState(desk);
  const working = state === 'working';
  const idle = state === 'idle';
  // consulting/in_team desks are dimmed; their motion is drawn by the overlay layer.
  const dimmed = state === 'consulting' || state === 'in_team';

  return (
    <div
      className={`persona-desk ${dimmed ? 'is-dimmed' : ''}`}
      style={{ left: x, top: y }}
      role="button"
      tabIndex={0}
      title={`${desk.name || desk.id} · ${state}`}
      onClick={() => onOpen(desk.id)}
      onKeyDown={(e) => { if (e.key === 'Enter') onOpen(desk.id); }}
      onMouseEnter={() => onHover(desk.id)}
      onMouseLeave={() => onHover(null)}
    >
      <svg viewBox="0 0 132 96" width="132" height="96" aria-hidden="true">
        <rect x="2" y="44" width="128" height="44" rx="7" fill="#c79a5b" />
        <rect
          className={working ? 'screen-lit' : 'screen-game'}
          x="36" y="6" width="56" height="38" rx="4"
        />
        {idle && (
          <>
            <rect className="game-block" x="46" y="16" width="9" height="9" rx="2" fill="#ffd166" />
            <rect className="game-block b2" x="64" y="26" width="9" height="9" rx="2" fill="#06d6a0" />
          </>
        )}
        <rect x="60" y="44" width="11" height="8" fill="#3a3f4b" />
        <circle cx="66" cy="80" r="16" fill="#4f86c6" />
        <circle cx="66" cy="58" r="11" fill="#f2c79a" />
        <text x="66" y="62" textAnchor="middle" fontSize="11">{avatarText(desk)}</text>
        {working && (
          <g>
            <rect className="typing-hand" x="50" y="76" width="11" height="6" rx="3" fill="#f2c79a" />
            <rect className="typing-hand h2" x="71" y="76" width="11" height="6" rx="3" fill="#f2c79a" />
          </g>
        )}
      </svg>
      <div className="desk-name">{desk.name || desk.id}</div>
    </div>
  );
}
```

- [ ] **Step 3: Verify it compiles.**

Run (from `hakimi-webui/`): `npm run build`
Expected: passes.

- [ ] **Step 4: Commit.**

```bash
git add hakimi-webui/src/PersonaDesk.tsx hakimi-webui/src/office.css
git commit -m "feat(webui): PersonaDesk flat-vector sprite + office styles"
```

---

## Task 7: `OfficeView` container (floor, runners, team clusters, hover card)

**Files:**
- Create: `hakimi-webui/src/OfficeView.tsx`

- [ ] **Step 1: Implement `OfficeView.tsx`.** It composes the hook + layout + desks, draws a consult "runner" between seats for each `consulting` persona, a ring + label around each team, and a hover detail card.

```tsx
import { useMemo, useRef, useState } from 'react';
import './office.css';
import PersonaDesk from './PersonaDesk';
import { useActivityStream } from './useActivityStream';
import { assignSeats, CELL_H, CELL_W } from './officeLayout';
import { displayedState, type DeskState } from './officeState';
import { useI18n } from './i18n';

interface OfficeViewProps {
  onOpenPersona: (id: string) => void;
}

const COLS = 4;

export default function OfficeView({ onOpenPersona }: OfficeViewProps) {
  const { t } = useI18n();
  const { office, connected } = useActivityStream(true);
  const [hoverId, setHoverId] = useState<string | null>(null);

  // stable seat assignment across renders
  const seatRef = useRef<Map<string, ReturnType<typeof assignSeats>['seats']> | null>(null);
  const ids = useMemo(() => Array.from(office.keys()).sort(), [office]);
  const prevSeats = useRef<ReturnType<typeof assignSeats>['seats'] | undefined>(undefined);
  const layout = useMemo(() => {
    const next = assignSeats(ids, prevSeats.current, COLS);
    prevSeats.current = next.seats;
    return next;
  }, [ids]);

  const desks = Array.from(office.values());
  const width = COLS * CELL_W + 48;
  const height = layout.rows * CELL_H + 80;

  // consult runners: from-seat -> to-seat
  const runners = desks
    .filter((d) => displayedState(d) === 'consulting' && d.consultingTo && d.consultingTo !== '?')
    .map((d) => {
      const from = layout.seats.get(d.id);
      const to = layout.seats.get(d.consultingTo!);
      if (!from || !to) return null;
      return { id: d.id, avatar: d.avatar, from, to };
    })
    .filter(Boolean) as Array<{ id: string; avatar: string; from: { x: number; y: number }; to: { x: number; y: number } }>;

  // team clusters: group desks by teamId
  const teams = new Map<string, DeskState[]>();
  for (const d of desks) {
    if (d.teamId && d.teamId !== '?') {
      const arr = teams.get(d.teamId) ?? [];
      arr.push(d);
      teams.set(d.teamId, arr);
    }
  }

  const hovered = hoverId ? office.get(hoverId) : null;
  const hoveredSeat = hoverId ? layout.seats.get(hoverId) : null;

  return (
    <div className="office-view">
      <div className="office-hint">
        {connected ? t('office.live') : t('office.offline')} · {t('office.clickHint')}
      </div>
      <div className="office-floor" style={{ width, height }}>
        {/* team rings (behind desks) */}
        {Array.from(teams.entries()).map(([teamId, members]) => {
          const seats = members.map((m) => layout.seats.get(m.id)).filter(Boolean) as Array<{ x: number; y: number }>;
          if (seats.length === 0) return null;
          const minX = Math.min(...seats.map((s) => s.x)) - 10;
          const minY = Math.min(...seats.map((s) => s.y)) - 22;
          const maxX = Math.max(...seats.map((s) => s.x)) + CELL_W - 8;
          const maxY = Math.max(...seats.map((s) => s.y)) + CELL_H - 24;
          return (
            <div key={teamId}>
              <div className="office-team-ring" style={{ left: minX, top: minY, width: maxX - minX, height: maxY - minY }} />
              <div className="office-team-label" style={{ left: minX + 8, top: minY + 2 }}>👥 {t('office.team')}</div>
            </div>
          );
        })}

        {/* desks */}
        {desks.map((d) => {
          const seat = layout.seats.get(d.id);
          if (!seat) return null;
          return (
            <PersonaDesk key={d.id} desk={d} x={seat.x} y={seat.y} onOpen={onOpenPersona} onHover={setHoverId} />
          );
        })}

        {/* consult runners (CSS transition animates the position change) */}
        {runners.map((r) => (
          <div
            key={`run-${r.id}`}
            className="office-runner"
            style={{ transform: `translate(${r.to.x + 50}px, ${r.to.y + 70}px)` }}
            data-from={`${r.from.x},${r.from.y}`}
          >
            <svg viewBox="0 0 40 48" width="40" height="48">
              <circle cx="14" cy="14" r="11" fill="#f2c79a" />
              <rect x="2" y="24" width="24" height="18" rx="7" fill="#7d5ba6" />
              <text x="14" y="18" textAnchor="middle" fontSize="11">{r.avatar || '🏃'}</text>
              <text x="30" y="14" fontSize="11">📋</text>
            </svg>
          </div>
        ))}

        {/* hover detail card */}
        {hovered && hoveredSeat && (
          <div className="office-card" style={{ left: hoveredSeat.x + CELL_W - 12, top: hoveredSeat.y }}>
            <strong>{hovered.avatar} {hovered.name || hovered.id}</strong>
            <span className="muted">{t(`office.state.${displayedState(hovered)}`)}</span>
            {hovered.taskHint && <div className="muted">{hovered.taskHint}</div>}
            {hovered.model && <div className="muted">{hovered.model}</div>}
          </div>
        )}

        {desks.length === 0 && <div className="office-hint">{t('office.empty')}</div>}
      </div>
    </div>
  );
}
```

(Runner note: the runner is positioned at the target seat; because `.office-runner` has a CSS `transition` on `transform`, when a `consult_started` first places it and a subsequent render moves it, it eases between seats — a lightweight "jog." Exact path polish is a deferred refinement per the spec; this delivers the visible cross-floor motion to the teammate.)

- [ ] **Step 2: Verify it compiles** (after Task 8 adds the i18n keys it uses; if building now, temporarily expect `t('office.*')` keys missing — Task 8 adds them. Build at the end of Task 8.)

- [ ] **Step 3: Commit.**

```bash
git add hakimi-webui/src/OfficeView.tsx
git commit -m "feat(webui): OfficeView floor with runners, team rings, hover card"
```

---

## Task 8: Wire into the app (view, nav rail, i18n)

**Files:**
- Modify: `hakimi-webui/src/App.tsx`, `hakimi-webui/src/PersonaRail.tsx`, `hakimi-webui/src/i18n.tsx`

- [ ] **Step 1: Add i18n keys.** In `i18n.tsx`, add to the `messages` object (anywhere in it):

```ts
  'office.nav': { en: 'Office', zh: '办公室' },
  'office.live': { en: 'Live', zh: '实时' },
  'office.offline': { en: 'Offline (reconnecting)', zh: '离线(重连中)' },
  'office.clickHint': { en: 'Click a desk to open chat', zh: '点击工位进入对话' },
  'office.team': { en: 'Team', zh: '组队' },
  'office.empty': { en: 'No personas yet', zh: '暂无人格' },
  'office.state.idle': { en: 'idle', zh: '空闲' },
  'office.state.working': { en: 'working', zh: '执行中' },
  'office.state.consulting': { en: 'consulting', zh: '找人交付' },
  'office.state.in_team': { en: 'in a team', zh: '组队中' },
```

- [ ] **Step 2: Add the office nav button to `PersonaRail.tsx`.** Update the prop union and add `onOffice`:

In `PersonaRailProps`, change `view` to include `'office'` and add the callback:

```ts
  view: 'chat' | 'config' | 'instance' | 'workspace' | 'office';
  onSelect: (id: string) => void;
  onEdit: (id: string) => void;
  onCreate: () => void;
  onInstance: () => void;
  onWorkspace: () => void;
  onOffice: () => void;
```

Destructure `onOffice` in the params, import an icon (`Building2`) — change the import line to `import { Bot, Building2, FolderTree, Plus, Settings } from 'lucide-react';` — and add a button at the top of `.persona-rail-foot` (before the New-persona button):

```tsx
        <button
          type="button"
          className={`persona-instance ${view === 'office' ? 'is-active' : ''}`}
          title="Office"
          onClick={onOffice}
        >
          <Building2 size={18} aria-hidden="true" />
        </button>
```

(If `Building2` is not exported by the installed `lucide-react`, use `LayoutGrid` instead — pick one that resolves; verify with the build.)

- [ ] **Step 3: Wire `App.tsx`.**
  1. Add the import: `import OfficeView from './OfficeView';`
  2. Widen the `view` state union (line ~173) to include `'office'`:
     `const [view, setView] = useState<'chat' | 'config' | 'instance' | 'workspace' | 'office'>('chat');`
  3. Pass `onOffice` to `PersonaRail` (in the `<PersonaRail ... />` props, after `onWorkspace`):
     `onOffice={() => setView('office')}`
  4. Render the office in the `console-main` view switch. Change the head of the chain (line ~567) from:
     `{view === 'instance' ? (` 
     to add an office branch first:

```tsx
          {view === 'office' ? (
            <OfficeView onOpenPersona={handleSelectPersona} />
          ) : view === 'instance' ? (
```

  (`handleSelectPersona` already sets the active persona, switches to `chat`, and clears the transcript — exactly the desired click-through.)

- [ ] **Step 4: Lint + build (full app now uses everything).**

Run (from `hakimi-webui/`): `npm run lint` then `npm run test` then `npm run build`
Expected: eslint clean, vitest passes (layout + state suites), `tsc -b` + `vite build` succeed. Fix any type/lint errors (e.g. unused `seatRef` in OfficeView — remove it if eslint flags it; the active refs are `prevSeats` and the layout memo).

- [ ] **Step 5: Commit.**

```bash
git add hakimi-webui/src/App.tsx hakimi-webui/src/PersonaRail.tsx hakimi-webui/src/i18n.tsx
git commit -m "feat(webui): office view nav + wiring + i18n"
```

---

## Task 9: Rebuild + commit the embedded bundle

**Files:**
- Modify: `crates/hakimi-webui/static/` (generated)

The running binary serves the committed `crates/hakimi-webui/static/` bundle; frontend CI does not cover it, so the bundle must be rebuilt and committed.

- [ ] **Step 1: Build the bundle.**

Run (from `hakimi-webui/`): `npm run build`
Expected: emits `crates/hakimi-webui/static/index.html`, `app.js`, `app.css` (per the existing `vite.config.ts` `outDir`/`base`).

- [ ] **Step 2: Commit the regenerated bundle.**

```bash
git add crates/hakimi-webui/static/
git commit -m "build(webui): rebuild embedded bundle with office dashboard"
```

- [ ] **Step 3: Manual smoke (optional, recommended).** Run the unified server, open the WebUI, save the bearer token, click the Office nav button, and confirm: desks render for all personas; chatting with a persona (in another tab/the chat view) flips its screen to lit + typing; a `team`-tool consult shows the teammate desk active and a runner/team ring; creating a persona adds a desk.

---

## Final Verification

- [ ] From `hakimi-webui/`: `npm run lint` (clean), `npm run test` (layout + state suites pass), `npm run build` (succeeds), and the `crates/hakimi-webui/static/` bundle is committed.
- [ ] Backend plan (`2026-06-26-persona-activity-feed.md`) is merged/available so `/api/activity/*` exists; otherwise the office shows "Offline (reconnecting)" and falls back to an empty/idle floor (graceful per spec §8).

---

## Self-Review (completed during planning)

**Spec coverage:** §3/§6 two-layer + components → Tasks 1-8; §4 base+overlay state machine on the frontend → Task 4 (`officeState` + tests mirroring backend transitions); §6 layout engine → Task 3; SSE client + reconnect + snapshot resync → Tasks 1, 5; §5 six behaviors → desk states (Task 6: working lit+typing, idle game) + runners + team rings + new-desk (a `persona_created` event adds a desk via the reducer; an unseated persona simply gets the next free seat) + click/hover interactivity (Tasks 6-7); §8 degradation (reconnect backoff, snapshot reseed, reduced-motion, offline fallback) → Tasks 5-7; nav/i18n → Task 8; embedded bundle → Task 9.

**Type consistency:** `ActivityEvent`/`PersonaActivity`/`PersonaState` (Task 1) are consumed identically by `officeState` (Task 4), `useActivityStream` (Task 5), `OfficeView` (Task 7). `DeskState`/`displayedState`/`reduceActivity`/`seedOffice` (Task 4) used by Tasks 5-7. `assignSeats`/`Seat`/`CELL_W`/`CELL_H` (Task 3) used by Task 7. `streamActivity`/`api.activitySnapshot` (Task 1) used by Task 5. `view` union widened consistently in `App.tsx` + `PersonaRail.tsx` (Task 8).

**Placeholders:** Task 6-step2 build is deferred to Task 8 (the component uses i18n keys added in Task 8) — noted explicitly, not a gap. Icon name (`Building2`/`LayoutGrid`) and the optional `ensure`/`seatRef` cleanups have explicit "verify with build / remove if flagged" instructions. No TODO/TBD.

**Scope:** Frontend only; consumes the backend feed plan. Deferred per spec §10: drag/manual seating, 50+ zoom, richer idle variety, sound, exact runner pathing (current runner uses a CSS-transition ease between seats).
```
