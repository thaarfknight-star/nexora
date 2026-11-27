import React, {
  memo,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";

export type LiquidGlassQuality = "high" | "medium" | "low";
export type LiquidGlassStyle = "clear" | "refraction";

export interface DesktopWallpaperInfo {
  data_url: string | null;
  fit_style: string;
  monitor_x: number;
  monitor_y: number;
  monitor_w: number;
  monitor_h: number;
  window_x: number;
  window_y: number;
  scale_factor: number;
}

/**
 * Signed distance to a centered rounded rectangle of half-size (hw, hh)
 * and corner radius r. Negative inside, 0 on border, positive outside.
 */
export function roundedRectSdf(
  x: number,
  y: number,
  hw: number,
  hh: number,
  r: number,
): number {
  const qx = Math.abs(x) - (hw - r);
  const qy = Math.abs(y) - (hh - r);
  return (
    Math.hypot(Math.max(qx, 0), Math.max(qy, 0)) +
    Math.min(Math.max(qx, qy), 0) -
    r
  );
}

const DISPLACEMENT_MAP_CACHE = new Map<string, string>();
const RING_MASK_CACHE = new Map<string, string>();
const MAX_CACHE_ENTRIES = 64;

/**
 * Generates (or retrieves from cache) a convex-bezel refraction displacement
 * map encoded as a PNG data URL.
 */
export function makeLiquidGlassMap(
  rawW: number,
  rawH: number,
  rawR: number,
  rawBezel?: number,
): string {
  if (typeof document === "undefined") return "";
  const w = Math.max(16, Math.min(720, Math.round(rawW)));
  const h = Math.max(16, Math.min(540, Math.round(rawH)));
  const minSide = Math.min(w, h);
  const r = Math.max(1, Math.min(Math.floor(minSide / 2), Math.round(rawR)));
  const bezel = Math.max(
    4,
    Math.min(80, Math.round(rawBezel ?? Math.min(80, minSide * 0.45))),
  );

  const key = `${w}x${h}x${r}x${bezel}`;
  const cached = DISPLACEMENT_MAP_CACHE.get(key);
  if (cached !== undefined) return cached;

  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) return "";

  const img = ctx.createImageData(w, h);
  const d = img.data;
  const hw = w / 2;
  const hh = h / 2;

  for (let y = 0; y < h; y++) {
    const py = y + 0.5 - hh;
    for (let x = 0; x < w; x++) {
      const px = x + 0.5 - hw;
      const depth = -roundedRectSdf(px, py, hw, hh, r);
      const idx = (y * w + x) * 4;
      let R = 128;
      let G = 128;
      if (depth >= 0 && depth < bezel) {
        const m = (1 - depth / bezel) ** 2;
        const gx =
          roundedRectSdf(px + 1, py, hw, hh, r) -
          roundedRectSdf(px - 1, py, hw, hh, r);
        const gy =
          roundedRectSdf(px, py + 1, hw, hh, r) -
          roundedRectSdf(px, py - 1, hw, hh, r);
        const len = Math.hypot(gx, gy) || 1;
        R = 128 - (gx / len) * m * 127;
        G = 128 - (gy / len) * m * 127;
      }
      d[idx] = Math.max(0, Math.min(255, Math.round(R)));
      d[idx + 1] = Math.max(0, Math.min(255, Math.round(G)));
      d[idx + 2] = 128;
      d[idx + 3] = 255;
    }
  }

  ctx.putImageData(img, 0, 0);
  let url = "";
  try {
    url = canvas.toDataURL("image/png");
  } catch {
    return "";
  }

  if (DISPLACEMENT_MAP_CACHE.size >= MAX_CACHE_ENTRIES) {
    const oldest = DISPLACEMENT_MAP_CACHE.keys().next().value;
    if (oldest !== undefined) DISPLACEMENT_MAP_CACHE.delete(oldest);
  }
  DISPLACEMENT_MAP_CACHE.set(key, url);
  return url;
}

/**
 * Generates a rounded-rectangle bezel ring alpha mask (width ~45% of smaller
 * side, max 80px, soft feathered inner edge) so Desktop refraction (beta)
 * appears ONLY along the curved glass rim while keeping the center 100%
 * transparent to the real desktop.
 */
export function makeBezelRingMask(
  rawW: number,
  rawH: number,
  rawR: number,
  rawBezel?: number,
): string {
  if (typeof document === "undefined") return "";
  const w = Math.max(16, Math.min(720, Math.round(rawW)));
  const h = Math.max(16, Math.min(540, Math.round(rawH)));
  const minSide = Math.min(w, h);
  const r = Math.max(1, Math.min(Math.floor(minSide / 2), Math.round(rawR)));
  const bezel = Math.max(
    6,
    Math.min(80, Math.round(rawBezel ?? Math.min(80, minSide * 0.45))),
  );

  const key = `${w}x${h}x${r}x${bezel}`;
  const cached = RING_MASK_CACHE.get(key);
  if (cached !== undefined) return cached;

  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext("2d");
  if (!ctx) return "";

  const img = ctx.createImageData(w, h);
  const d = img.data;
  const hw = w / 2;
  const hh = h / 2;

  for (let y = 0; y < h; y++) {
    const py = y + 0.5 - hh;
    for (let x = 0; x < w; x++) {
      const px = x + 0.5 - hw;
      const depth = -roundedRectSdf(px, py, hw, hh, r);
      const idx = (y * w + x) * 4;
      let alpha = 0;
      if (depth >= 0 && depth < bezel) {
        // Full strength at outer rim, smooth cubic feather to 0 at inner bezel boundary
        const t = depth / bezel;
        const fade = 1 - t * t * (3 - 2 * t);
        alpha = Math.round(fade * 255);
      }
      d[idx] = 255;
      d[idx + 1] = 255;
      d[idx + 2] = 255;
      d[idx + 3] = alpha;
    }
  }

  ctx.putImageData(img, 0, 0);
  let url = "";
  try {
    url = canvas.toDataURL("image/png");
  } catch {
    return "";
  }

  if (RING_MASK_CACHE.size >= MAX_CACHE_ENTRIES) {
    const oldest = RING_MASK_CACHE.keys().next().value;
    if (oldest !== undefined) RING_MASK_CACHE.delete(oldest);
  }
  RING_MASK_CACHE.set(key, url);
  return url;
}

/**
 * Monitors prefers-reduced-motion, GPU pressure, and sustained frame drops
 * to compute the effective Liquid Glass quality.
 */
export function useEffectiveLiquidGlassQuality(
  requested: LiquidGlassQuality,
  enabled: boolean,
  gpuPercent?: number | null,
): { quality: LiquidGlassQuality; reducedMotion: boolean; autoFallback: boolean } {
  const [reducedMotion, setReducedMotion] = useState<boolean>(() => {
    if (typeof window === "undefined" || !window.matchMedia) return false;
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  });
  const [frameDropFallback, setFrameDropFallback] = useState(false);

  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) return;
    const mq = window.matchMedia("(prefers-reduced-motion: reduce)");
    const onChange = (e: MediaQueryListEvent) => setReducedMotion(e.matches);
    mq.addEventListener?.("change", onChange);
    return () => mq.removeEventListener?.("change", onChange);
  }, []);

  useEffect(() => {
    if (!enabled || requested === "low") {
      setFrameDropFallback(false);
      return;
    }
    let rafId = 0;
    let last = performance.now();
    let slowFrames = 0;
    let totalFrames = 0;
    let cancelled = false;

    const tick = (now: number) => {
      if (cancelled) return;
      const dt = now - last;
      last = now;
      if (dt > 0 && dt < 250) {
        totalFrames++;
        if (dt > 45) slowFrames++;
        if (totalFrames >= 24) {
          if (slowFrames >= 14) {
            setFrameDropFallback(true);
            return;
          }
          totalFrames = 0;
          slowFrames = 0;
        }
      }
      rafId = window.requestAnimationFrame(tick);
    };
    rafId = window.requestAnimationFrame(tick);
    return () => {
      cancelled = true;
      window.cancelAnimationFrame(rafId);
    };
  }, [enabled, requested]);

  const highGpu = gpuPercent != null && gpuPercent >= 88;
  const autoFallback = highGpu || frameDropFallback;
  const quality: LiquidGlassQuality = autoFallback ? "low" : requested;
  return { quality, reducedMotion, autoFallback };
}

/**
 * Optional "Desktop refraction (beta)" bezel-ring layer.
 * Reads the real Windows desktop wallpaper via `get_desktop_wallpaper`, draws
 * the exact sub-region behind the island aligned to screen coordinates,
 * applies the 3-pass SDF displacement SVG filter as a normal `filter` (not
 * `backdrop-filter`), and masks it with a rounded-rect bezel ring so the
 * center stays 100% see-through to the real desktop.
 */
export const DesktopWallpaperRefraction = memo(function DesktopWallpaperRefraction({
  enabled,
  quality,
  radius = 28,
  scale = 118,
  containerRef,
  layoutKey,
}: {
  enabled: boolean;
  quality: LiquidGlassQuality;
  radius?: number;
  scale?: number;
  containerRef: React.RefObject<HTMLElement | null>;
  layoutKey: string;
}) {
  const rawId = useId();
  const filterId = `lg-wp-${rawId.replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [wpInfo, setWpInfo] = useState<DesktopWallpaperInfo | null>(null);
  const [size, setSize] = useState<{ w: number; h: number }>({ w: 540, h: 54 });

  // Fetch wallpaper & live screen position when beta mode is enabled or layout changes
  useEffect(() => {
    if (!enabled || quality === "low") return;
    let alive = true;
    invoke<DesktopWallpaperInfo>("get_desktop_wallpaper")
      .then((info) => {
        if (alive && info?.data_url) setWpInfo(info);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [enabled, quality, layoutKey]);

  // Observe island size (debounced so expand/collapse transitions stay smooth)
  useEffect(() => {
    if (!enabled || quality === "low") return;
    const el = containerRef.current;
    if (!el) return;
    let timer: number | undefined;
    const measure = () => {
      const rect = el.getBoundingClientRect();
      const nw = Math.round(rect.width);
      const nh = Math.round(rect.height);
      if (nw >= 24 && nh >= 24) {
        setSize((prev) =>
          Math.abs(prev.w - nw) <= 2 && Math.abs(prev.h - nh) <= 2
            ? prev
            : { w: nw, h: nh },
        );
      }
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => {
      window.clearTimeout(timer);
      timer = window.setTimeout(measure, 180);
    });
    ro.observe(el);
    return () => {
      window.clearTimeout(timer);
      ro.disconnect();
    };
  }, [enabled, quality, containerRef]);

  // Draw the aligned wallpaper slice onto the bezel canvas
  useEffect(() => {
    if (!enabled || quality === "low" || !wpInfo?.data_url) return;
    const canvas = canvasRef.current;
    const container = containerRef.current;
    if (!canvas || !container) return;

    let alive = true;
    const img = new Image();
    img.onload = () => {
      if (!alive) return;
      const w = size.w;
      const h = size.h;
      canvas.width = w;
      canvas.height = h;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      ctx.clearRect(0, 0, w, h);

      const sf = wpInfo.scale_factor || 1;
      const monW = wpInfo.monitor_w / sf;
      const monH = wpInfo.monitor_h / sf;
      const rect = container.getBoundingClientRect();
      const pillScreenX = (wpInfo.window_x - wpInfo.monitor_x) / sf + rect.left;
      const pillScreenY = (wpInfo.window_y - wpInfo.monitor_y) / sf + rect.top;

      // Compute Windows wallpaper placement in logical monitor coordinates
      let drawW = monW;
      let drawH = monH;
      let drawX = 0;
      let drawY = 0;
      const imgAspect = img.naturalWidth / Math.max(1, img.naturalHeight);
      const monAspect = monW / Math.max(1, monH);

      if (wpInfo.fit_style === "fit") {
        if (imgAspect > monAspect) {
          drawW = monW;
          drawH = monW / imgAspect;
          drawY = (monH - drawH) / 2;
        } else {
          drawH = monH;
          drawW = monH * imgAspect;
          drawX = (monW - drawW) / 2;
        }
      } else if (wpInfo.fit_style === "center") {
        drawW = img.naturalWidth / sf;
        drawH = img.naturalHeight / sf;
        drawX = (monW - drawW) / 2;
        drawY = (monH - drawH) / 2;
      } else if (wpInfo.fit_style === "stretch") {
        drawW = monW;
        drawH = monH;
      } else {
        // Default Windows "fill" (cover)
        if (imgAspect > monAspect) {
          drawH = monH;
          drawW = monH * imgAspect;
          drawX = (monW - drawW) / 2;
        } else {
          drawW = monW;
          drawH = monW / imgAspect;
          drawY = (monH - drawH) / 2;
        }
      }

      ctx.save();
      ctx.translate(-pillScreenX, -pillScreenY);
      if (wpInfo.fit_style === "tile") {
        const pat = ctx.createPattern(img, "repeat");
        if (pat) {
          ctx.fillStyle = pat;
          ctx.fillRect(pillScreenX, pillScreenY, w, h);
        }
      } else {
        ctx.drawImage(img, drawX, drawY, drawW, drawH);
      }
      ctx.restore();
    };
    img.src = wpInfo.data_url;
    return () => {
      alive = false;
    };
  }, [enabled, quality, wpInfo, size.w, size.h, containerRef]);

  const mapUrl = useMemo(() => {
    if (!enabled || quality === "low") return "";
    return makeLiquidGlassMap(size.w, size.h, radius);
  }, [enabled, quality, size.w, size.h, radius]);

  const ringMaskUrl = useMemo(() => {
    if (!enabled || quality === "low") return "";
    return makeBezelRingMask(size.w, size.h, radius);
  }, [enabled, quality, size.w, size.h, radius]);

  if (!enabled || quality === "low" || !wpInfo?.data_url || !mapUrl || !ringMaskUrl) {
    return null;
  }

  const sR = scale;
  const sG = Math.round(scale * 0.93 * 10) / 10;
  const sB = Math.round(scale * 0.86 * 10) / 10;

  return (
    <>
      <svg width="0" height="0" aria-hidden="true" className="lg-svg-defs">
        <defs>
          <filter
            id={filterId}
            x="0%"
            y="0%"
            width="100%"
            height="100%"
            colorInterpolationFilters="sRGB"
          >
            <feImage
              href={mapUrl}
              preserveAspectRatio="none"
              result="dispMap"
            />
            {quality === "high" ? (
              <>
                <feDisplacementMap
                  in="SourceGraphic"
                  in2="dispMap"
                  scale={sR}
                  xChannelSelector="R"
                  yChannelSelector="G"
                  result="dispR"
                />
                <feColorMatrix
                  in="dispR"
                  type="matrix"
                  values="1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0"
                  result="chR"
                />
                <feDisplacementMap
                  in="SourceGraphic"
                  in2="dispMap"
                  scale={sG}
                  xChannelSelector="R"
                  yChannelSelector="G"
                  result="dispG"
                />
                <feColorMatrix
                  in="dispG"
                  type="matrix"
                  values="0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 1 0"
                  result="chG"
                />
                <feDisplacementMap
                  in="SourceGraphic"
                  in2="dispMap"
                  scale={sB}
                  xChannelSelector="R"
                  yChannelSelector="G"
                  result="dispB"
                />
                <feColorMatrix
                  in="dispB"
                  type="matrix"
                  values="0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0"
                  result="chB"
                />
                <feBlend in="chR" in2="chG" mode="screen" result="rg" />
                <feBlend in="rg" in2="chB" mode="screen" result="refract" />
              </>
            ) : (
              <feDisplacementMap
                in="SourceGraphic"
                in2="dispMap"
                scale={sG}
                xChannelSelector="R"
                yChannelSelector="G"
                result="refract"
              />
            )}
          </filter>
        </defs>
      </svg>
      <div
        className="lg-desktop-refract-ring"
        aria-hidden="true"
        style={{
          maskImage: `url(${ringMaskUrl})`,
          WebkitMaskImage: `url(${ringMaskUrl})`,
          maskSize: "100% 100%",
          WebkitMaskSize: "100% 100%",
          maskRepeat: "no-repeat",
          WebkitMaskRepeat: "no-repeat",
        }}
      >
        <canvas
          ref={canvasRef}
          className="lg-desktop-refract-canvas"
          style={{ filter: `url(#${filterId}) saturate(1.22)` }}
        />
      </div>
    </>
  );
});

export interface LiquidGlassProps
  extends React.HTMLAttributes<HTMLElement> {
  as?: "div" | "section" | "button" | "span";
  enabled: boolean;
  quality?: LiquidGlassQuality;
  radius?: number;
  bezel?: number;
  scale?: number;
  round?: boolean;
  initialWidth?: number;
  initialHeight?: number;
  type?: "button" | "submit" | "reset";
  disabled?: boolean;
  innerRef?: React.RefObject<HTMLElement | null>;
}

/**
 * Reusable `<LiquidGlass>` wrapper.
 * In Clear mode (default), applies pure edge-only transparent glass
 * (alpha <= 0.03, 0px blur, bright top-left / dim bottom-right rim highlights,
 * no filled round buttons) so the real Windows desktop shines straight through.
 */
export const LiquidGlass = memo(function LiquidGlass({
  as = "div",
  enabled,
  round = false,
  className = "",
  style,
  children,
  innerRef,
  // Destructure optional sizing props so they aren't forwarded to DOM nodes
  quality: _quality,
  radius: _radius,
  bezel: _bezel,
  scale: _scale,
  initialWidth: _initialWidth,
  initialHeight: _initialHeight,
  ...rest
}: LiquidGlassProps) {
  const combinedClass = enabled
    ? `${className} liquid-glass${round ? " liquid-glass-round" : ""}`.trim()
    : className;

  const Tag = as as React.ElementType;

  return (
    <Tag ref={innerRef} className={combinedClass} style={style} {...rest}>
      {children}
    </Tag>
  );
});
