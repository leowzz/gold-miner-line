import { describe, expect, it } from 'vitest';
import { normalizedPoint, rays, type Settings } from './geometry';

const settings: Settings = { originX: .5, originY: .14, count: 9, spread: 150,
  rotation: 0, length: 1, color: '#53e3ea', width: 1.5, opacity: .65, showOrigin: true };

describe('fan geometry', () => {
  it('has symmetric endpoints around a downward center ray', () => {
    const lines = rays(settings, 1700, 1126);
    expect(lines).toHaveLength(9);
    expect(lines[4].endX).toBe(850);
    expect(lines[4].endY).toBeGreaterThan(lines[4].y);
    expect(lines[4].central).toBe(true);
    expect(lines[0].endX + lines[8].endX).toBeCloseTo(1700);
    expect(lines[0].endY).toBeCloseTo(lines[8].endY);
    expect(Math.hypot(lines[0].endX - 850, lines[0].endY - 157.64)).toBeCloseTo(Math.hypot(1700, 1126));
  });
  it('rotates a single ray right for positive angles', () => {
    const [line] = rays({ ...settings, count: 1, rotation: 90 }, 100, 100);
    expect(line.endY).toBeCloseTo(line.y);
    expect(line.endX).toBeGreaterThan(line.x);
  });
  it('scales coordinates and length together', () => {
    const a = rays(settings, 850, 563)[0];
    const b = rays(settings, 1700, 1126)[0];
    expect(b.x).toBe(a.x * 2);
    expect(b.y).toBe(a.y * 2);
    expect(b.endX).toBeCloseTo(a.endX * 2);
    expect(b.endY).toBeCloseTo(a.endY * 2);
  });
  it('clamps a dragged origin to the overlay', () => {
    expect(normalizedPoint(-10, 700, 960, 640)).toEqual({ originX: 0, originY: 1 });
    const point = normalizedPoint(480, 89.6, 960, 640);
    expect(point.originX).toBe(.5);
    expect(point.originY).toBeCloseTo(.14);
  });
});
