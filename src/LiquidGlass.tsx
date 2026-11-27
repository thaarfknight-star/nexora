import React, {
  memo,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";

export type LiquidGlassQuality = "high" | "medium" | "low";

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

/** Global cache of displacement-map PNG data URLs keyed by "wxhxrxbezel". */
const DISPLACEMENT_MAP_CACHE = new Map<string, string>();
const MAX_CACHE_ENTRIES = 64;

/**
 * Generates (or retrieves from cache) a convex-bezel refraction displacement
 * map encoded as a PNG data URL.
 *
 * Inside the bezel band (0 <= depth < bezel), magnitude m = (1 - depth/bezel)^2
 * pushes along the inward normal (-grad SDF):
 *   R = 128 - nx * m * 127
 *   G = 128 - ny * m * 127
 *   B = 128
 * Outside the bezel (center & exterior), stays neutral (128, 128) so the
 * interior glass remains crystal-clear and razor-sharp.
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
    Math.min(
      80,
      Math.round(rawBezel ?? Math.min(80, minSide * 0.45)),
    ),
  );

  const key = `${w}x${h}x${r}x${bezel}`;
  const cached = DISPLACEMENT_MAP_CACHE.get(key);
  if (cached !== undefined) {
    return cached;
  }

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
 * Monitors prefers-reduced-motion, GPU pressure, and sustained frame drops
 * to compute the effective Liquid Glass quality without tanking FPS.
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

  // Lightweight frame-drop detector: only active when Liquid Glass is enabled
  // above "low". If average frame delta over 24 frames exceeds 45ms (<22fps),
  // automatically falls back to "low".
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
      // Ignore background-tab spikes (> 250ms)
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
 * Extracts a vibrant dominant RGB accent color from a media cover-art data URI
 * so the island's internal refractable ambient layer shifts with the playing track.
 */
export function useDominantMediaColor(coverArt?: string): string {
  const [rgb, setRgb] = useState<string>("98, 168, 255");

  useEffect(() => {
    if (!coverArt || typeof document === "undefined") {
      setRgb("98, 168, 255");
      return;
    }
    let alive = true;
    const img = new Image();
    img.crossOrigin = "anonymous";
    img.onload = () => {
      if (!alive) return;
      try {
        const c = document.createElement("canvas");
        c.width = 8;
        c.height = 8;
        const ctx = c.getContext("2d");
        if (!ctx) return;
        ctx.drawImage(img, 0, 0, 8, 8);
        const data = ctx.getImageData(0, 0, 8, 8).data;
        let rSum = 0;
        let gSum = 0;
        let bSum = 0;
        let wSum = 0;
        for (let i = 0; i < data.length; i += 4) {
          const r = data[i];
          const g = data[i + 1];
          const b = data[i + 2];
          const max = Math.max(r, g, b);
          const min = Math.min(r, g, b);
          // Favor saturated, non-black pixels for richer refraction
          const weight = 0.25 + ((max - min) / 255) * 2.0 + (max / 255) * 0.5;
          rSum += r * weight;
          gSum += g * weight;
          bSum += b * weight;
          wSum += weight;
        }
        if (wSum > 0) {
          const r = Math.min(255, Math.round(rSum / wSum + 18));
          const g = Math.min(255, Math.round(gSum / wSum + 18));
          const b = Math.min(255, Math.round(bSum / wSum + 18));
          setRgb(`${r}, ${g}, ${b}`);
        }
      } catch {
        // Keep fallback accent color
      }
    };
    img.src = coverArt;
    return () => {
      alive = false;
    };
  }, [coverArt]);

  return rgb;
}

/**
 * Reusable hook that measures an element (debounced on resize only), caches
 * its SDF displacement map, and returns the filter ID + SVG element to render.
 */
export function useLiquidGlassFilter({
  enabled,
  quality,
  radius = 24,
  bezel,
  scale = 115,
  round = false,
  initialWidth = 120,
  initialHeight = 48,
}: {
  enabled: boolean;
  quality: LiquidGlassQuality;
  radius?: number;
  bezel?: number;
  scale?: number;
  round?: boolean;
  initialWidth?: number;
  initialHeight?: number;
}) {
  const rawId = useId();
  const filterId = `lg-flt-${rawId.replace(/[^a-zA-Z0-9_-]/g, "")}`;
  const ref = useRef<HTMLElement | null>(null);
  const [size, setSize] = useState<{ w: number; h: number }>({
    w: initialWidth,
    h: initialHeight,
  });

  useEffect(() => {
    if (!enabled || quality === "low") return;
    const el = ref.current;
    if (!el) return;

    let timer: number | undefined;
    const measure = () => {
      const rect = el.getBoundingClientRect();
      const nw = Math.round(rect.width);
      const nh = Math.round(rect.height);
      if (nw >= 12 && nh >= 12) {
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
      // Debounce resize so expand/collapse transitions keep the previous
      // filter smoothly until the animation settles.
      timer = window.setTimeout(measure, 180);
    });
    ro.observe(el);
    return () => {
      window.clearTimeout(timer);
      ro.disconnect();
    };
  }, [enabled, quality]);

  const effectiveRadius = round
    ? Math.max(1, Math.floor(Math.min(size.w, size.h) / 2))
    : radius;

  const mapUrl = useMemo(() => {
    if (!enabled || quality === "low") return "";
    return makeLiquidGlassMap(size.w, size.h, effectiveRadius, bezel);
  }, [enabled, quality, size.w, size.h, effectiveRadius, bezel]);

  const sR = scale;
  const sG = Math.round(scale * 0.93 * 10) / 10;
  const sB = Math.round(scale * 0.86 * 10) / 10;

  const svgFilter =
    enabled && quality !== "low" && mapUrl ? (
      <svg
        width="0"
        height="0"
        aria-hidden="true"
        className="lg-svg-defs"
      >
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
    ) : null;

  const backdropFilter = !enabled
    ? undefined
    : quality === "low" || !mapUrl
      ? "blur(1.5px) saturate(1.25)"
      : `url(#${filterId}) saturate(1.25)`;

  return { ref, filterId, svgFilter, backdropFilter };
}

export interface LiquidGlassProps
  extends React.HTMLAttributes<HTMLElement> {
  as?: "div" | "section" | "button" | "span";
  enabled: boolean;
  quality: LiquidGlassQuality;
  radius?: number;
  bezel?: number;
  scale?: number;
  round?: boolean;
  initialWidth?: number;
  initialHeight?: number;
  type?: "button" | "submit" | "reset";
  disabled?: boolean;
}

/**
 * Reusable `<LiquidGlass>` surface wrapper. Applies optical SDF bezel
 * refraction, chromatic aberration (High), and specular rim/highlights
 * when `enabled` is true; renders cleanly as a normal element otherwise.
 */
export const LiquidGlass = memo(function LiquidGlass({
  as = "div",
  enabled,
  quality,
  radius = 24,
  bezel,
  scale,
  round = false,
  initialWidth,
  initialHeight,
  className = "",
  style,
  children,
  ...rest
}: LiquidGlassProps) {
  const effectiveScale = scale ?? (round ? 94 : 118);
  const { ref, svgFilter, backdropFilter } = useLiquidGlassFilter({
    enabled,
    quality,
    radius,
    bezel,
    scale: effectiveScale,
    round,
    initialWidth: initialWidth ?? (round ? 34 : 160),
    initialHeight: initialHeight ?? (round ? 34 : 52),
  });

  const combinedClass = enabled
    ? `${className} liquid-glass${round ? " liquid-glass-round" : ""}`.trim()
    : className;

  const combinedStyle: React.CSSProperties | undefined = enabled
    ? {
        ...style,
        backdropFilter,
        WebkitBackdropFilter: backdropFilter,
      }
    : style;

  const Tag = as as React.ElementType;

  return (
    <>
      {svgFilter}
      <Tag
        ref={ref}
        className={combinedClass}
        style={combinedStyle}
        {...rest}
      >
        {children}
      </Tag>
    </>
  );
});
