export interface Settings {
  originX: number;
  originY: number;
  count: number;
  spread: number;
  rotation: number;
  length: number;
  color: string;
  width: number;
  opacity: number;
  showOrigin: boolean;
}

export function rays(s: Settings, width: number, height: number) {
  const x = s.originX * width;
  const y = s.originY * height;
  const length = Math.hypot(width, height) * s.length;
  return Array.from({ length: s.count }, (_, i) => {
    const degrees = s.rotation + (s.count === 1 ? 0 : -s.spread / 2 + i * s.spread / (s.count - 1));
    const angle = degrees * Math.PI / 180;
    return { x, y, endX: x + Math.sin(angle) * length, endY: y + Math.cos(angle) * length,
      central: s.count % 2 === 1 && i === Math.floor(s.count / 2) };
  });
}

export function normalizedPoint(x: number, y: number, width: number, height: number) {
  return { originX: Math.max(0, Math.min(1, x / Math.max(1, width))),
    originY: Math.max(0, Math.min(1, y / Math.max(1, height))) };
}
