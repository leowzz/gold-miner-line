import { expect, it } from 'vitest';
import { activeTracking, adjustRegion, dragRegion, type TrackingUpdate } from './tracking';

const state = { trackingEnabled: true, calibrating: false, visible: true, revision: 3 };
const update: TrackingUpdate = { revision: 3, angle: -40, origin: { x: .48, y: .15 }, confidence: .97, message: '正在跟随短线', capturedAt: 1000 };

it('drops directions after a mode, calibration or parameter change', () => {
  expect(activeTracking(state, update, 1100)).toBe(update);
  for (const patch of [{trackingEnabled: false}, {calibrating: true}, {visible: false}, {revision: 4}]) {
    expect(activeTracking({ ...state, ...patch }, update, 1100)).toBeNull();
  }
});

it('rejects queued stale frames, clears lost directions and accepts zero degrees', () => {
  expect(activeTracking(state, update, 1400)).toBeNull();
  expect(activeTracking(state, null, 1100)).toBeNull();
  expect(activeTracking(state, { ...update, angle: null }, 1100)?.angle).toBeNull();
  expect(activeTracking(state, { ...update, angle: 0 }, 1100)?.angle).toBe(0);
});

it('moves and resizes the rectangle within the overlay, independent of fan origin', () => {
  const region = { x: .4, y: .2, width: .1, height: .08 };
  expect(dragRegion(region, 'move', 2, -1)).toEqual({ ...region, x: .9, y: 0 });
  const resized = dragRegion(region, 'nw', -.1, -.1);
  expect(resized.x).toBeCloseTo(.3);
  expect(resized.width).toBeCloseTo(.2);
  expect(resized.y + resized.height).toBeCloseTo(.28);
  expect(dragRegion(region, 'se', 2, 2)).toEqual({ ...region, width: .6, height: .8 });
  expect(dragRegion(region, 'nw', 1, 1).width).toBeCloseTo(.005);
  expect(adjustRegion(region, { width: 1 }).width).toBe(.6);
  // Pointer deltas are normalized, so equal relative drags at 100% / 150% match.
  expect(dragRegion(region, 'move', 30/600, 20/400)).toEqual(dragRegion(region, 'move', 45/900, 30/600));
});
