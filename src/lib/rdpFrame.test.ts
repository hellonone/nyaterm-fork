import { describe, expect, it } from "vitest";
import { createRdpFramePatchBuffer, decodeRdpFramePatch } from "./rdpFrame";

describe("rdpFrame", () => {
  it("roundtrips the binary frame patch header", () => {
    const payload = new Uint8Array([1, 2, 3, 255, 4, 5, 6, 255]);
    const buffer = createRdpFramePatchBuffer({
      sequence: 42n,
      desktopWidth: 1920,
      desktopHeight: 1080,
      x: 10,
      y: 20,
      width: 2,
      height: 1,
      stride: 8,
      pixelFormat: "BGRA8888",
      payload,
    });

    const patch = decodeRdpFramePatch(buffer);

    expect(patch.sequence).toBe(42n);
    expect(patch.desktopWidth).toBe(1920);
    expect(patch.desktopHeight).toBe(1080);
    expect(patch.x).toBe(10);
    expect(patch.y).toBe(20);
    expect(patch.pixelFormat).toBe("BGRA8888");
    expect([...patch.payload]).toEqual([...payload]);
  });

  it("rejects mismatched payload lengths", () => {
    const buffer = createRdpFramePatchBuffer({
      sequence: 1n,
      desktopWidth: 1,
      desktopHeight: 1,
      x: 0,
      y: 0,
      width: 1,
      height: 1,
      stride: 4,
      pixelFormat: "RGBA8888",
      payload: new Uint8Array([0, 0, 0, 255]),
    });

    expect(() => decodeRdpFramePatch(buffer.slice(0, buffer.byteLength - 1))).toThrow(
      /payload length/,
    );
  });

  it("rejects payloads that cannot cover the declared stride", () => {
    const buffer = createRdpFramePatchBuffer({
      sequence: 1n,
      desktopWidth: 2,
      desktopHeight: 2,
      x: 0,
      y: 0,
      width: 2,
      height: 2,
      stride: 12,
      pixelFormat: "RGBA8888",
      payload: new Uint8Array(16),
    });

    expect(() => decodeRdpFramePatch(buffer)).toThrow(/stride and height/);
  });

  it("decodes local dirty rectangle patches", () => {
    const payload = new Uint8Array([17, 34, 51, 255, 68, 85, 102, 255]);
    const buffer = createRdpFramePatchBuffer({
      sequence: 9n,
      desktopWidth: 1920,
      desktopHeight: 1080,
      x: 320,
      y: 240,
      width: 2,
      height: 1,
      stride: 8,
      pixelFormat: "RGBA8888",
      payload,
    });

    const patch = decodeRdpFramePatch(buffer);

    expect(patch.desktopWidth).toBe(1920);
    expect(patch.desktopHeight).toBe(1080);
    expect(patch.x).toBe(320);
    expect(patch.y).toBe(240);
    expect(patch.width).toBe(2);
    expect(patch.height).toBe(1);
    expect(patch.stride).toBe(8);
    expect(patch.pixelFormat).toBe("RGBA8888");
    expect([...patch.payload]).toEqual([...payload]);
  });
});
