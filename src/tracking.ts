export interface Region { x: number; y: number; width: number; height: number }
export interface TrackingOptions { region: Region; darkness: number; showReference: boolean }
export interface TrackingUpdate { revision: number; angle: number | null; origin: { x: number; y: number } | null; confidence: number; message: string; capturedAt: number }

const clamp = (n: number, min: number, max: number) => Math.max(min, Math.min(max, n));
export function adjustRegion(region: Region, patch: Partial<Region>): Region {
  const next = { ...region, ...patch };
  next.width = clamp(next.width, .005, 1);
  next.height = clamp(next.height, .005, 1);
  if (patch.width !== undefined) next.width = Math.min(next.width, 1 - region.x);
  if (patch.height !== undefined) next.height = Math.min(next.height, 1 - region.y);
  next.x = clamp(next.x, 0, 1 - next.width);
  next.y = clamp(next.y, 0, 1 - next.height);
  return next;
}

export function dragRegion(region: Region, handle: string, dx: number, dy: number): Region {
  if (handle === 'move') return adjustRegion(region, { x: region.x + dx, y: region.y + dy });
  let { x, y, width, height } = region;
  if (handle.includes('w')) { x = clamp(x + dx, 0, x + width - .005); width = region.x + region.width - x; }
  if (handle.includes('n')) { y = clamp(y + dy, 0, y + height - .005); height = region.y + region.height - y; }
  if (handle.includes('e')) width = clamp(width + dx, .005, 1 - x);
  if (handle.includes('s')) height = clamp(height + dy, .005, 1 - y);
  // Subtraction at the minimum size can yield 0.004999…; keep the serialized
  // rectangle inside the backend's inclusive validation limits.
  return { x, y, width: Math.max(.005, width), height: Math.max(.005, height) };
}

export function activeTracking(
  state: { trackingEnabled: boolean; calibrating: boolean; visible: boolean; revision: number } | null,
  update: TrackingUpdate | null,
  now: number,
): TrackingUpdate | null {
  if (!state?.trackingEnabled || state.calibrating || !state.visible || !update
    || update.revision !== state.revision || now - update.capturedAt > 300
    || update.capturedAt > now + 100) return null;
  return update;
}
