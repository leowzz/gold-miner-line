import { useEffect, useRef, useState } from 'react';
import type { activePreview } from './tracking';

type Preview = ReturnType<typeof activePreview>;
function loadImage(source: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error('无法显示预览画面'));
    image.src = source;
  });
}
const labels = { invalid: '识别框过小', noMetal: '未找到灰色区域', noMatch: '未匹配夹子', ambiguous: '多个候选', detected: '已识别', confirming: '确认中', captureError: '采集失败', previewError: '预览失败', slow: '画面过慢' };

export function RecognitionPreview({ preview, visible, calibrating }: { preview: Preview; visible: boolean; calibrating: boolean }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [annotations, setAnnotations] = useState(true);
  const [loadError, setLoadError] = useState(false);
  const frame = preview?.update.frame;
  const stale = preview?.stale ?? false;
  useEffect(() => {
    let cancelled = false;
    setLoadError(false);
    if (!frame) return;
    // Decode both layers before painting so image, contours and landmarks always
    // belong to one frame, even if a slower decode completes out of order.
    void Promise.all([loadImage(frame.image), loadImage(frame.contours)]).then(([image, contours]) => {
      if (cancelled || !canvas.current) return;
      const element = canvas.current;
      element.width = image.naturalWidth; element.height = image.naturalHeight;
      const ctx = element.getContext('2d');
      if (!ctx) { setLoadError(true); return; }
      ctx.drawImage(image, 0, 0);
      if (!annotations || stale) return;
      ctx.imageSmoothingEnabled = false;
      ctx.drawImage(contours, 0, 0, element.width, element.height);
      const d = frame.detection;
      if (!d) return;
      const sx = element.width / frame.width, sy = element.height / frame.height;
      const line = (x1: number, y1: number, x2: number, y2: number, color: string) => {
        ctx.beginPath(); ctx.strokeStyle = color; ctx.lineWidth = Math.max(1, element.width / 300);
        ctx.moveTo(x1 * sx, y1 * sy); ctx.lineTo(x2 * sx, y2 * sy); ctx.stroke();
      };
      line(d.jawLeft.x, d.jawLeft.y, d.jawRight.x, d.jawRight.y, '#ff7b87');
      const angle = d.angle * Math.PI / 180, length = Math.hypot(frame.width, frame.height);
      line(d.start.x, d.start.y, d.start.x + Math.sin(angle) * length, d.start.y + Math.cos(angle) * length, '#fff1a0');
      for (const point of [d.jawLeft, d.jawRight, d.start]) {
        ctx.beginPath(); ctx.fillStyle = point === d.start ? '#fff1a0' : '#ff7b87';
        ctx.arc(point.x * sx, point.y * sy, Math.max(1.5, element.width / 160), 0, Math.PI * 2); ctx.fill();
      }
    }).catch(() => { if (!cancelled) setLoadError(true); });
    return () => { cancelled = true; };
  }, [frame, annotations, stale]);
  const status = !visible ? '已暂停' : stale ? '画面已过期' : loadError ? '显示失败' : preview ? labels[preview.update.status] : '等待画面';
  const message = !visible ? '显示辅助窗口后恢复识别预览。'
    : stale ? '当前是上一帧画面，标注已隐藏；正在等待新画面。'
    : loadError ? '预览画面未能解码，正在等待下一帧。'
    : preview?.update.message || '正在读取识别框内的画面…';
  const detection = !stale ? frame?.detection : null;
  return <section className="recognition-preview" aria-label="识别预览">
    <div className="recognition-heading"><h2>识别预览</h2><span className={`recognition-status ${preview?.update.status === 'detected' && !stale ? 'matched' : ''}`} role="status">{status}</span></div>
    <div className={`recognition-image ${stale ? 'stale' : ''}`}>
      {frame && !loadError ? <canvas ref={canvas} aria-label={annotations && !stale ? '识别区域画面及夹子轮廓、夹口和中垂线' : '识别区域原始画面'} /> : <p>{!visible ? '识别已暂停' : preview?.update.status === 'captureError' ? '未能取得画面' : preview?.update.status === 'previewError' ? '未能生成预览' : '等待识别区域画面'}</p>}
      {stale && <span className="stale-badge">上一帧 · 已过期</span>}
    </div>
    <div className="recognition-toolbar"><span>{calibrating ? '校准预览 · 锁定后绘制方向线' : '识别框内的实时画面'}</span>
      <button type="button" aria-pressed={annotations} onClick={() => setAnnotations(value => !value)}>{annotations ? '查看原图' : '显示标注'}</button>
    </div>
    {annotations && <div className="recognition-legend"><span><i className="contour-dot" />匹配轮廓</span><span><i className="candidate-dot" />灰色候选</span><span><i className="jaw-dot" />夹口</span><span><i className="axis-dot" />中垂线</span></div>}
    {frame && !stale && <div className="recognition-metrics">
      <span>角度 <b>{detection ? `${detection.angle.toFixed(1)}°` : '—'}</b></span>
      <span title="算法匹配分数，不代表正确率">匹配分数 <b>{detection ? detection.confidence.toFixed(2) : '—'}</b></span>
      <span>灰色区域 <b>{frame.componentCount}</b></span>
      <span>采集 / 识别 <b>{frame.processingMs} ms</b></span>
    </div>}
    <p className="recognition-message">{message}</p>
  </section>;
}
