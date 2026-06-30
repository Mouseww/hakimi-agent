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
