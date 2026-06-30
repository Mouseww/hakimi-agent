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
