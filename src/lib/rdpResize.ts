export const RDP_MIN_WIDTH = 640;
export const RDP_MIN_HEIGHT = 480;
export const RDP_MAX_WIDTH = 7680;
export const RDP_MAX_HEIGHT = 4320;

export interface RdpResizeDecisionInput {
  mode?: string | null;
  visible: boolean;
  containerWidth: number;
  containerHeight: number;
  lastWidth?: number | null;
  lastHeight?: number | null;
}

export interface RdpResizeDecision {
  shouldResize: boolean;
  width: number;
  height: number;
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}

export function normalizeRdpDisplayMode(mode?: string | null): "fit-window" | "fixed" {
  return mode === "fit-window" ? "fit-window" : "fixed";
}

export function decideFitWindowResize(input: RdpResizeDecisionInput): RdpResizeDecision {
  if (normalizeRdpDisplayMode(input.mode) !== "fit-window" || !input.visible) {
    return { shouldResize: false, width: input.lastWidth ?? 0, height: input.lastHeight ?? 0 };
  }

  const width = clamp(Math.round(input.containerWidth), RDP_MIN_WIDTH, RDP_MAX_WIDTH);
  const height = clamp(Math.round(input.containerHeight), RDP_MIN_HEIGHT, RDP_MAX_HEIGHT);
  const sameSize = input.lastWidth === width && input.lastHeight === height;
  return { shouldResize: !sameSize, width, height };
}

export interface RdpDesktopSize {
  width: number;
  height: number;
}

export function keepDesktopSizeIfUnchanged(
  current: RdpDesktopSize,
  next: RdpDesktopSize,
): RdpDesktopSize {
  return current.width === next.width && current.height === next.height ? current : next;
}
