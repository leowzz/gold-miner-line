import { useEffect, useRef, useState, type PointerEvent } from 'react';
import { dragRegion, type Region } from './tracking';

const handles = ['n', 's', 'e', 'w', 'ne', 'nw', 'se', 'sw'];
export function DetectionRegion({ region, width, height, onChange }: {
  region: Region; width: number; height: number; onChange: (region: Region) => void;
}) {
  const [draft, setDraft] = useState(region);
  const latest = useRef(region);
  const drag = useRef<{ x: number; y: number; handle: string; region: Region } | null>(null);
  useEffect(() => { if (!drag.current) { setDraft(region); latest.current = region; } }, [region]);
  const begin = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    const handle = (e.target as HTMLElement).dataset.handle || 'move';
    drag.current = { x: e.clientX, y: e.clientY, handle, region: latest.current };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const move = (e: PointerEvent<HTMLDivElement>) => {
    const start = drag.current;
    if (!start) return;
    latest.current = dragRegion(start.region, start.handle, (e.clientX - start.x) / width, (e.clientY - start.y) / height);
    setDraft(latest.current);
  };
  const finish = () => {
    if (!drag.current) return;
    drag.current = null;
    onChange(latest.current);
  };
  return <div data-overlay-control className="detection-region" aria-label="夹子识别框"
    style={{ left: draft.x * width, top: draft.y * height, width: draft.width * width, height: draft.height * height }}
    onPointerDown={begin} onPointerMove={move} onPointerUp={e => { move(e); finish(); }}
    onLostPointerCapture={finish} onPointerCancel={finish}>
    <span className={`region-label ${draft.y * height < 30 ? 'below' : ''} ${(1 - draft.x) * width < 150 ? 'align-right' : ''}`}>夹子识别框 · 拖动移动</span>
    {handles.map(handle => <span data-overlay-control key={handle} data-handle={handle} className={`region-handle region-${handle}`} />)}
  </div>;
}
