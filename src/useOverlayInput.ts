import { useEffect, type RefObject } from 'react';
import { invoke } from '@tauri-apps/api/core';

// Send actual control bounds, including child resize handles that extend outside
// the recognition rectangle. Text, guide lines and other decoration stay passive.
export function useOverlayInput(root: RefObject<HTMLDivElement | null>, active: boolean, onError: (error: string) => void) {
  useEffect(() => {
    const element = root.current;
    if (!element) return;
    let frame = 0, previous = '', disposed = false;
    const report = () => {
      frame = 0;
      const regions = active ? Array.from(element.querySelectorAll<HTMLElement>('[data-overlay-control]')).map(control => {
        const { x, y, width, height } = control.getBoundingClientRect();
        return { x, y, width, height };
      }).filter(r => r.width > 0 && r.height > 0) : [];
      const args = { regions, width: innerWidth, height: innerHeight };
      const next = JSON.stringify(args);
      if (next === previous) return;
      previous = next;
      void invoke('set_input_regions', args).catch(error => { if (!disposed) onError(String(error)); });
    };
    const schedule = () => { if (!frame) frame = requestAnimationFrame(report); };
    const observer = new MutationObserver(schedule);
    observer.observe(element, { subtree: true, childList: true, attributes: true, attributeFilter: ['style', 'class'] });
    const resize = new ResizeObserver(schedule);
    resize.observe(element);
    window.addEventListener('resize', schedule);
    schedule();
    return () => { disposed = true; observer.disconnect(); resize.disconnect(); window.removeEventListener('resize', schedule); cancelAnimationFrame(frame); };
  }, [root, active, onError]);
}
