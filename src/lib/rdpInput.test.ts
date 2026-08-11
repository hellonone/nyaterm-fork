import { describe, expect, it } from "vitest";
import { mapKeyboardCodeToRdp } from "./rdpInput";

describe("rdpInput", () => {
  it("maps left and right modifiers distinctly", () => {
    expect(mapKeyboardCodeToRdp("ControlLeft")).toEqual({ scanCode: 0x1d });
    expect(mapKeyboardCodeToRdp("ControlRight")).toEqual({ scanCode: 0x1d, extended: true });
    expect(mapKeyboardCodeToRdp("AltRight")).toEqual({ scanCode: 0x38, extended: true });
  });

  it("maps navigation keys as extended keys", () => {
    expect(mapKeyboardCodeToRdp("Delete")).toEqual({ scanCode: 0x53, extended: true });
    expect(mapKeyboardCodeToRdp("ArrowLeft")).toEqual({ scanCode: 0x4b, extended: true });
  });
});
