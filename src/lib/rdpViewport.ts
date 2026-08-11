export interface RdpViewportRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

function clamp(value: number, min: number, max: number) {
  return Math.max(min, Math.min(max, value));
}

export function mapClientPointToRdpPixel(
  rect: RdpViewportRect,
  desktopWidth: number,
  desktopHeight: number,
  clientX: number,
  clientY: number,
) {
  const maxX = Math.max(0, desktopWidth - 1);
  const maxY = Math.max(0, desktopHeight - 1);
  if (rect.width <= 0 || rect.height <= 0 || desktopWidth <= 0 || desktopHeight <= 0) {
    return { x: 0, y: 0 };
  }

  const cssX = clamp(clientX - rect.left, 0, rect.width);
  const cssY = clamp(clientY - rect.top, 0, rect.height);
  return {
    x: clamp(Math.floor((cssX / rect.width) * desktopWidth), 0, maxX),
    y: clamp(Math.floor((cssY / rect.height) * desktopHeight), 0, maxY),
  };
}
