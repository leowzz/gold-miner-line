import React, { useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { normalizedPoint, rays, type Settings } from './geometry';
import './styles.css';

interface Snapshot {
  settings: Settings;
  visible: boolean;
  calibrating: boolean;
  revision: number;
  notice: string | null;
  saveError: string | null;
}

const isOverlay = new URLSearchParams(location.search).has('overlay');
document.documentElement.dataset.surface = isOverlay ? 'overlay' : 'panel';

function useModel() {
  const [state, setState] = useState<Snapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const revision = useRef(-1);
  const queue = useRef(Promise.resolve());
  const accept = (next: Snapshot) => {
    if (next.revision >= revision.current) {
      revision.current = next.revision;
      setState(next);
    }
  };
  useEffect(() => {
    if (!isTauri()) {
      setError('请通过桌面应用打开。开发时运行 make dev。');
      return;
    }
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      unlisten = await listen<Snapshot>('state-changed', event => { if (!disposed) accept(event.payload); });
      if (disposed) { unlisten(); return; }
      const initial = await invoke<Snapshot>('get_state');
      if (!disposed) accept(initial);
    })().catch(e => { if (!disposed) setError(String(e)); });
    return () => { disposed = true; unlisten?.(); };
  }, []);
  const command = (name: string, args?: Record<string, unknown>) => {
    queue.current = queue.current.then(async () => {
      try {
        accept(await invoke<Snapshot>(name, args));
        setError(null);
      } catch (e) { setError(String(e)); }
    });
  };
  const patch = (value: Partial<Settings>) => command('update_settings', { patch: value });
  return { state, error, command, patch, setError };
}

type Model = ReturnType<typeof useModel>;

function Field({ label, value, min, max, step = 1, suffix = '', onChange }: {
  label: string; value: number; min: number; max: number; step?: number; suffix?: string;
  onChange: (value: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));
  const focused = useRef(false);
  useEffect(() => { if (!focused.current) setDraft(String(value)); }, [value]);
  const commit = () => {
    focused.current = false;
    const parsed = Number(draft);
    const next = draft.trim() && Number.isFinite(parsed) ? Math.max(min, Math.min(max, Math.round(parsed / step) * step)) : value;
    const rounded = Number(Number(next).toFixed(3));
    setDraft(String(rounded));
    onChange(rounded);
  };
  return <div className="field">
    <div className="field-top"><span>{label}</span><div className="number-unit">
      <input aria-label={`${label}数值`} type="number" min={min} max={max} step={step}
        value={draft} onFocus={() => { focused.current = true; }} onChange={e => setDraft(e.target.value)}
        onBlur={commit} onKeyDown={e => { if (e.key === 'Enter') e.currentTarget.blur(); }} />
      {suffix && <span>{suffix}</span>}
    </div></div>
    <input aria-label={label} type="range" min={min} max={max} step={step} value={value}
      style={{ '--fill': `${(value - min) / (max - min) * 100}%` } as React.CSSProperties}
      onChange={e => onChange(Number(e.target.value))} />
  </div>;
}

function Fan({ settings, width, height, preview = false }: { settings: Settings; width: number; height: number; preview?: boolean }) {
  return <svg className="fan" width="100%" height="100%" viewBox={`0 0 ${width} ${height}`} aria-hidden="true">
    {rays(settings, width, height).map((line, i) => <line key={i} x1={line.x} y1={line.y} x2={line.endX} y2={line.endY}
      stroke={settings.color} strokeWidth={preview ? (line.central ? 1.6 : .8) : settings.width * (line.central ? 1.4 : 1)}
      opacity={Math.min(1, settings.opacity * (line.central ? 1.25 : 1))} />)}
    {settings.showOrigin && <g transform={`translate(${settings.originX * width} ${settings.originY * height})`} stroke={settings.color} fill="none" opacity={settings.opacity}>
      <circle r={preview ? 3 : 6} strokeWidth="1.5" /><path d="M-10 0H10M0-10V10" strokeWidth="1" />
    </g>}
  </svg>;
}

function Panel({ model }: { model: Model }) {
  const { state, patch, command, error } = model;
  if (!state) return <main className="loading"><h1>黄金矿工辅助线</h1><p>{error || '正在打开辅助窗口…'}</p></main>;
  const s = state.settings;
  return <main className="panel">
    <header className="flex items-center justify-between gap-3">
      <div className="flex items-center gap-3"><div className="brand-mark" aria-hidden="true">⌁</div><div><h1>黄金矿工辅助线</h1><p className="subtitle">对齐轴心，把握出钩方向</p></div></div>
      <span className={`status ${state.visible ? 'active' : ''}`}><i />{!state.visible ? '已隐藏' : state.calibrating ? '校准中' : '已锁定'}</span>
    </header>

    <section className="preview" aria-label="辅助线预览">
      <div className="preview-ground" />
      <Fan settings={s} width={360} height={150} preview />
      <span className="preview-label">实时预览</span>
      <span className="preview-meta">{s.count} 条 · {s.spread}°</span>
    </section>

    <div className="actions grid grid-cols-2 gap-2">
      <button className="primary" onClick={() => command('set_mode', { calibrating: !state.calibrating, visible: true })}>
        {state.calibrating ? '锁定 · 开始游戏' : '重新校准'}
      </button>
      <button onClick={() => command('set_mode', { visible: !state.visible })}>{state.visible ? '隐藏辅助线' : '显示辅助线'}</button>
    </div>
    <p className="instruction">{state.calibrating ? '拖动覆盖框对齐游戏画面，再将圆心移到钩爪轴心。锁定后鼠标可直接操作游戏。' : '辅助线已锁定，鼠标可穿透。需要调整位置时，点击「重新校准」。'}</p>
    {(error || state.notice || state.saveError) && <div className="notice" role="status">{[error, state.notice, state.saveError].filter(Boolean).join('\n')}</div>}

    <section className="settings-section">
      <div className="section-heading"><h2>辅助线</h2><span>以竖直向下为 0°</span></div>
      <div className="grid grid-cols-2 gap-x-5 gap-y-4">
        <Field label="数量" value={s.count} min={1} max={31} suffix="条" onChange={count => patch({ count })} />
        <Field label="展开角度" value={s.spread} min={0} max={180} suffix="°" onChange={spread => patch({ spread })} />
        <Field label="整体偏转" value={s.rotation} min={-90} max={90} suffix="°" onChange={rotation => patch({ rotation })} />
        <Field label="线长" value={Math.round(s.length * 100)} min={5} max={200} suffix="%" onChange={length => patch({ length: length / 100 })} />
      </div>
    </section>
    <section className="settings-section">
      <div className="section-heading"><h2>轴心位置</h2><span>相对游戏画面</span></div>
      <div className="grid grid-cols-2 gap-x-5">
        <Field label="水平" value={Number((s.originX * 100).toFixed(1))} min={0} max={100} step={.1} suffix="%" onChange={originX => patch({ originX: originX / 100 })} />
        <Field label="垂直" value={Number((s.originY * 100).toFixed(1))} min={0} max={100} step={.1} suffix="%" onChange={originY => patch({ originY: originY / 100 })} />
      </div>
    </section>
    <section className="settings-section appearance">
      <div className="section-heading"><h2>显示样式</h2><label className="color-picker"><input aria-label="辅助线颜色" type="color" value={s.color} onChange={e => patch({ color: e.target.value })} /><span>颜色</span></label></div>
      <div className="grid grid-cols-2 gap-x-5">
        <Field label="线宽" value={s.width} min={.5} max={8} step={.5} onChange={width => patch({ width })} />
        <Field label="不透明度" value={Math.round(s.opacity * 100)} min={5} max={100} suffix="%" onChange={opacity => patch({ opacity: opacity / 100 })} />
      </div>
      <label className="checkbox-row"><input type="checkbox" checked={s.showOrigin} onChange={e => patch({ showOrigin: e.target.checked })} />显示圆心标记</label>
    </section>
    <footer>
      <div className="flex justify-between gap-2"><button className="text-button" onClick={() => command('reset_settings')}>恢复默认参数</button><button className="text-button" onClick={() => command('recover_overlay')}>找回覆盖窗口</button></div>
      <div className="shortcuts"><span>显示 / 隐藏 <kbd>Ctrl ⇧ G</kbd></span><span>校准 / 锁定 <kbd>Ctrl ⇧ L</kbd></span></div>
      <p className="footnote">参数自动保存在本机 · 关闭此面板即退出</p>
    </footer>
  </main>;
}

type ResizeDirection = Parameters<ReturnType<typeof getCurrentWindow>['startResizeDragging']>[0];
const resizeHandles: [string, ResizeDirection][] = [
  ['n', 'North'], ['s', 'South'], ['e', 'East'], ['w', 'West'],
  ['ne', 'NorthEast'], ['nw', 'NorthWest'], ['se', 'SouthEast'], ['sw', 'SouthWest'],
];

function Overlay({ model }: { model: Model }) {
  const { state, patch, setError, error } = model;
  const [size, setSize] = useState({ width: innerWidth, height: innerHeight });
  const dragging = useRef(false);
  const frame = useRef(0);
  const pending = useRef<Partial<Settings> | null>(null);
  useEffect(() => {
    const resize = () => setSize({ width: innerWidth, height: innerHeight });
    window.addEventListener('resize', resize);
    return () => { window.removeEventListener('resize', resize); cancelAnimationFrame(frame.current); };
  }, []);
  if (!state) return null;
  const updateOrigin = (e: React.PointerEvent) => {
    pending.current = normalizedPoint(e.clientX, e.clientY, size.width, size.height);
    if (!frame.current) frame.current = requestAnimationFrame(() => {
      if (pending.current) patch(pending.current);
      pending.current = null;
      frame.current = 0;
    });
  };
  const nativeAction = (action: Promise<void>) => { void action.catch(e => setError(String(e))); };
  return <div className={`overlay ${state.calibrating ? 'calibrating' : ''}`}>
    <Fan settings={state.settings} {...size} />
    {state.calibrating && <>
      <div className="drag-bar" onPointerDown={e => { if (e.button === 0) nativeAction(getCurrentWindow().startDragging()); }}>
        <span className="drag-grip">⠿</span><span>拖动对齐游戏画面 · 拖动边缘缩放</span>
      </div>
      <button className="origin-handle" aria-label="拖动圆心到钩爪轴心" title="拖动圆心到钩爪轴心"
        style={{ left: state.settings.originX * size.width, top: state.settings.originY * size.height }}
        onPointerDown={e => { if (e.button !== 0) return; dragging.current = true; e.currentTarget.setPointerCapture(e.pointerId); updateOrigin(e); }}
        onPointerMove={e => { if (dragging.current) updateOrigin(e); }}
        onPointerUp={e => { if (dragging.current) { updateOrigin(e); dragging.current = false; } }}
        onLostPointerCapture={() => { dragging.current = false; }}><span /></button>
      <div className="overlay-caption">覆盖游戏画面，不包含系统标题栏</div>
      {resizeHandles.map(([side, direction]) => <div key={side} className={`resize-handle resize-${side}`}
        onPointerDown={e => { if (e.button === 0) nativeAction(getCurrentWindow().startResizeDragging(direction)); }} />)}
      {error && <div className="overlay-error">{error}</div>}
    </>}
  </div>;
}

function App() {
  const model = useModel();
  return isOverlay ? <Overlay model={model} /> : <Panel model={model} />;
}

createRoot(document.getElementById('root')!).render(<App />);
