import { describe, expect, it } from "vitest";
import { mapClientPointToRdpPixel } from "./rdpViewport";

describe("rdpViewport", () => {
  it("maps one-to-one canvas coordinates", () => {
    expect(
      mapClientPointToRdpPixel(
        { left: 0, top: 0, width: 1920, height: 1080 },
        1920,
        1080,
        640,
        360,
      ),
    ).toEqual({ x: 640, y: 360 });
  });

  it("maps scaled-down CSS coordinates to remote pixels", () => {
    expect(
      mapClientPointToRdpPixel(
        { left: 10, top: 20, width: 960, height: 540 },
        1920,
        1080,
        490,
        290,
      ),
    ).toEqual({ x: 960, y: 540 });
  });

  it("maps scaled-up CSS coordinates to remote pixels", () => {
    expect(
      mapClientPointToRdpPixel(
        { left: 0, top: 0, width: 2560, height: 1440 },
        1280,
        720,
        1280,
        720,
      ),
    ).toEqual({ x: 640, y: 360 });
  });

  it("clamps to the remote desktop bounds", () => {
    expect(
      mapClientPointToRdpPixel(
        { left: 10, top: 10, width: 100, height: 100 },
        1920,
        1080,
        500,
        -50,
      ),
    ).toEqual({ x: 1919, y: 0 });
  });
});
