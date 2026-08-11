import { describe, expect, it } from "vitest";
import {
  buildRdpUnicodeInput,
  rdpBeforeInputText,
  rdpCompositionCommitText,
  rdpInputFallbackText,
  shouldFallbackToPrintableRdpKey,
  shouldUsePhysicalRdpKey,
} from "./rdpIme";

describe("rdpIme", () => {
  it("emits committed composition text once", () => {
    expect(rdpCompositionCommitText("你好")).toBe("你好");
    expect(buildRdpUnicodeInput("你好")).toEqual([{ type: "unicode", text: "你好" }]);
  });

  it("uses beforeinput for plain text but ignores composing updates", () => {
    expect(
      rdpBeforeInputText({
        data: "a",
        inputType: "insertText",
        isComposing: false,
      }),
    ).toBe("a");
    expect(
      rdpBeforeInputText({
        data: "ni",
        inputType: "insertCompositionText",
        isComposing: true,
      }),
    ).toBeNull();
  });

  it("keeps shortcuts and navigation on the physical key path", () => {
    expect(
      shouldUsePhysicalRdpKey({
        key: "c",
        ctrlKey: true,
        altKey: false,
        metaKey: false,
      }),
    ).toBe(true);
    expect(
      shouldUsePhysicalRdpKey({
        key: "ArrowLeft",
        ctrlKey: false,
        altKey: false,
        metaKey: false,
      }),
    ).toBe(true);
    expect(
      shouldUsePhysicalRdpKey({
        key: "Shift",
        ctrlKey: false,
        altKey: false,
        metaKey: false,
      }),
    ).toBe(true);
    expect(
      shouldUsePhysicalRdpKey({
        key: "a",
        ctrlKey: false,
        altKey: false,
        metaKey: false,
      }),
    ).toBe(false);
  });

  it("uses input fallback text only outside composition", () => {
    expect(rdpInputFallbackText("abc", false)).toBe("abc");
    expect(rdpInputFallbackText("", false)).toBeNull();
    expect(rdpInputFallbackText("ni", true)).toBeNull();
  });

  it("falls back to printable physical keys only for plain text keys", () => {
    expect(
      shouldFallbackToPrintableRdpKey({
        key: "a",
        ctrlKey: false,
        altKey: false,
        metaKey: false,
        isComposing: false,
      }),
    ).toBe(true);
    expect(
      shouldFallbackToPrintableRdpKey({
        key: "ArrowLeft",
        ctrlKey: false,
        altKey: false,
        metaKey: false,
        isComposing: false,
      }),
    ).toBe(false);
    expect(
      shouldFallbackToPrintableRdpKey({
        key: "Shift",
        ctrlKey: false,
        altKey: false,
        metaKey: false,
        isComposing: false,
      }),
    ).toBe(false);
    expect(
      shouldFallbackToPrintableRdpKey({
        key: "c",
        ctrlKey: true,
        altKey: false,
        metaKey: false,
        isComposing: false,
      }),
    ).toBe(false);
  });
});
