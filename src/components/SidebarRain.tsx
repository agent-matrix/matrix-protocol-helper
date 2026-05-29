/* ============================================================
   SidebarRain.tsx — subtle Matrix code-rain at the top of the
   sidebar (premium, low-key). Ported from the design.
   ============================================================ */
import { useEffect, useRef } from "react";

export function SidebarRain() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const GLY = "ﾊﾐﾋｰｳｼﾅﾓﾆ01<>=*+ｱｲｳｴ";
    let raf = 0;
    let w = 0;
    let h = 0;
    let cols = 0;
    let drops: number[] = [];
    let dpr = 1;
    let last = 0;

    function resize() {
      if (!canvas || !ctx) return;
      dpr = Math.min(window.devicePixelRatio || 1, 2);
      w = canvas.clientWidth;
      h = canvas.clientHeight;
      canvas.width = w * dpr;
      canvas.height = h * dpr;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      cols = Math.ceil(w / 12);
      drops = new Array(cols).fill(0).map(() => Math.random() * -20);
      ctx.font = '11px "JetBrains Mono", monospace';
      ctx.textBaseline = "top";
    }

    function frame(t: number) {
      raf = requestAnimationFrame(frame);
      if (t - last < 90) return; // slow it down
      last = t;
      if (!ctx) return;
      ctx.fillStyle = "rgba(12, 18, 15, 0.32)";
      ctx.fillRect(0, 0, w, h);
      for (let i = 0; i < cols; i++) {
        const x = i * 12;
        const y = drops[i] * 12;
        if (y > 0 && y < h) {
          ctx.fillStyle = "rgba(180, 255, 214, 0.85)";
          ctx.fillText(GLY[(Math.random() * GLY.length) | 0], x, y);
          ctx.fillStyle = "rgba(41, 224, 122, 0.45)";
          ctx.fillText(GLY[(Math.random() * GLY.length) | 0], x, y - 12);
        }
        drops[i] += 0.5;
        if (y > h && Math.random() > 0.97) drops[i] = Math.random() * -8;
      }
    }

    resize();
    raf = requestAnimationFrame(frame);
    const ro = new ResizeObserver(resize);
    ro.observe(canvas);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
    };
  }, []);

  return <canvas ref={ref} className="side-rain" aria-hidden="true" />;
}
