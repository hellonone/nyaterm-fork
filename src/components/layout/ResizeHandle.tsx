import { useCallback, useEffect, useRef } from "react";

interface ResizeHandleProps {
  direction: "horizontal" | "vertical";
  onResize: (delta: number) => void;
  className?: string;
}

/** Draggable handle for horizontal or vertical resize. Calls onResize(delta) on drag. */
export default function ResizeHandle({ direction, onResize, className = "" }: ResizeHandleProps) {
  const startPos = useRef(0);
  const onResizeRef = useRef(onResize);

  // Keep the ref up to date
  useEffect(() => {
    onResizeRef.current = onResize;
  }, [onResize]);

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      startPos.current = direction === "horizontal" ? e.clientX : e.clientY;

      const handleMouseMove = (ev: MouseEvent) => {
        const current = direction === "horizontal" ? ev.clientX : ev.clientY;
        const delta = current - startPos.current;
        startPos.current = current;
        onResizeRef.current(delta);
      };

      const handleMouseUp = () => {
        document.removeEventListener("mousemove", handleMouseMove);
        document.removeEventListener("mouseup", handleMouseUp);
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
      };

      document.body.style.cursor = direction === "horizontal" ? "col-resize" : "row-resize";
      document.body.style.userSelect = "none";
      document.addEventListener("mousemove", handleMouseMove);
      document.addEventListener("mouseup", handleMouseUp);
    },
    [direction],
  );

  const isHorizontal = direction === "horizontal";

  return (
    <div
      className={`
        ${
          isHorizontal
            ? "group relative z-20 w-px shrink-0 cursor-col-resize overflow-visible"
            : "group relative z-20 h-px shrink-0 cursor-row-resize overflow-visible"
        } ${className}
      `}
      onMouseDown={handleMouseDown}
    >
      <div
        className={
          isHorizontal
            ? "absolute inset-y-0 left-1/2 w-[3px] -translate-x-1/2"
            : "absolute top-1/2 inset-x-0 h-[3px] -translate-y-1/2"
        }
      />
      <div
        className={
          isHorizontal
            ? "absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-[var(--df-border)] transition-[width,background-color] group-hover:w-[3px] group-hover:bg-[var(--df-primary)] group-active:w-[3px] group-active:bg-[var(--df-primary)]"
            : "absolute top-1/2 inset-x-0 h-px -translate-y-1/2 bg-[var(--df-border)] transition-[height,background-color] group-hover:h-[3px] group-hover:bg-[var(--df-primary)] group-active:h-[3px] group-active:bg-[var(--df-primary)]"
        }
      />
    </div>
  );
}
