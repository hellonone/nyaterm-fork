import type { RdpInputEvent } from "./rdpInput";

const PHYSICAL_TEXT_KEYS = new Set([
  "Backspace",
  "Enter",
  "Tab",
  "Escape",
  "Delete",
  "Insert",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "ArrowDown",
]);

const MODIFIER_KEYS = new Set(["Shift", "Control", "Alt", "Meta"]);

export function shouldUsePhysicalRdpKey(
  event: Pick<KeyboardEvent, "key" | "ctrlKey" | "altKey" | "metaKey">,
) {
  return (
    event.ctrlKey ||
    event.altKey ||
    event.metaKey ||
    MODIFIER_KEYS.has(event.key) ||
    PHYSICAL_TEXT_KEYS.has(event.key)
  );
}

export function shouldFallbackToPrintableRdpKey(
  event: Pick<KeyboardEvent, "key" | "ctrlKey" | "altKey" | "metaKey" | "isComposing">,
) {
  if (event.isComposing || shouldUsePhysicalRdpKey(event)) return false;
  return event.key.length === 1;
}

export function buildRdpUnicodeInput(text: string): RdpInputEvent[] {
  return text.length > 0 ? [{ type: "unicode", text }] : [];
}

export function rdpBeforeInputText(
  event: Pick<InputEvent, "data" | "inputType" | "isComposing">,
): string | null {
  if (event.isComposing) return null;
  if (event.inputType !== "insertText" && event.inputType !== "insertCompositionText") return null;
  return event.data || null;
}

export function rdpCompositionCommitText(text: string): string | null {
  return text.length > 0 ? text : null;
}

export function rdpInputFallbackText(value: string, composing: boolean): string | null {
  if (composing) return null;
  return value.length > 0 ? value : null;
}
