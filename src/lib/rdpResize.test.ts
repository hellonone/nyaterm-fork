import { describe, expect, it } from "vitest";
import {
  decideFitWindowResize,
  keepDesktopSizeIfUnchanged,
  normalizeRdpDisplayMode,
} from "./rdpResize";

describe("rdpResize", () => {
  it("normalizes unsupported display modes to fixed", () => {
    expect(normalizeRdpDisplayMode("fit-window")).toBe("fit-window");
    expect(normalizeRdpDisplayMode("native")).toBe("fixed");
    expect(normalizeRdpDisplayMode("fixed")).toBe("fixed");
  });

  it("does not resize fixed or invisible sessions", () => {
    expect(
      decideFitWindowResize({
        mode: "fixed",
        visible: true,
        containerWidth: 1200,
        containerHeight: 800,
      }).shouldResize,
    ).toBe(false);
    expect(
      decideFitWindowResize({
        mode: "fit-window",
        visible: false,
        containerWidth: 1200,
        containerHeight: 800,
      }).shouldResize,
    ).toBe(false);
  });

  it("clamps fit-window resize and skips duplicate sizes", () => {
    expect(
      decideFitWindowResize({
        mode: "fit-window",
        visible: true,
        containerWidth: 320,
        containerHeight: 200,
      }),
    ).toEqual({ shouldResize: true, width: 640, height: 480 });

    expect(
      decideFitWindowResize({
        mode: "fit-window",
        visible: true,
        containerWidth: 640,
        containerHeight: 480,
        lastWidth: 640,
        lastHeight: 480,
      }).shouldResize,
    ).toBe(false);
  });

  it("keeps desktop object identity when the size is unchanged", () => {
    const current = { width: 1920, height: 1080 };
    expect(keepDesktopSizeIfUnchanged(current, { width: 1920, height: 1080 })).toBe(current);
    expect(keepDesktopSizeIfUnchanged(current, { width: 1280, height: 720 })).toEqual({
      width: 1280,
      height: 720,
    });
  });
});
