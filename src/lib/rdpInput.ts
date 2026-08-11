export type RdpInputEvent =
  | {
      type: "key-down" | "key-up";
      scanCode: number;
      extended: boolean;
      repeat: boolean;
    }
  | {
      type: "mouse-move";
      x: number;
      y: number;
    }
  | {
      type: "mouse-button";
      button: "left" | "middle" | "right" | "back" | "forward";
      pressed: boolean;
      x: number;
      y: number;
    }
  | {
      type: "mouse-wheel";
      deltaX: number;
      deltaY: number;
      x: number;
      y: number;
    }
  | {
      type: "unicode";
      text: string;
    }
  | {
      type: "release-all-keys";
    };

export interface RdpKeyMapping {
  scanCode: number;
  extended?: boolean;
}

const KEY_CODE_TO_SCANCODE: Record<string, RdpKeyMapping> = {
  Escape: { scanCode: 0x01 },
  Digit1: { scanCode: 0x02 },
  Digit2: { scanCode: 0x03 },
  Digit3: { scanCode: 0x04 },
  Digit4: { scanCode: 0x05 },
  Digit5: { scanCode: 0x06 },
  Digit6: { scanCode: 0x07 },
  Digit7: { scanCode: 0x08 },
  Digit8: { scanCode: 0x09 },
  Digit9: { scanCode: 0x0a },
  Digit0: { scanCode: 0x0b },
  Minus: { scanCode: 0x0c },
  Equal: { scanCode: 0x0d },
  Backspace: { scanCode: 0x0e },
  Tab: { scanCode: 0x0f },
  KeyQ: { scanCode: 0x10 },
  KeyW: { scanCode: 0x11 },
  KeyE: { scanCode: 0x12 },
  KeyR: { scanCode: 0x13 },
  KeyT: { scanCode: 0x14 },
  KeyY: { scanCode: 0x15 },
  KeyU: { scanCode: 0x16 },
  KeyI: { scanCode: 0x17 },
  KeyO: { scanCode: 0x18 },
  KeyP: { scanCode: 0x19 },
  BracketLeft: { scanCode: 0x1a },
  BracketRight: { scanCode: 0x1b },
  Enter: { scanCode: 0x1c },
  ControlLeft: { scanCode: 0x1d },
  ControlRight: { scanCode: 0x1d, extended: true },
  KeyA: { scanCode: 0x1e },
  KeyS: { scanCode: 0x1f },
  KeyD: { scanCode: 0x20 },
  KeyF: { scanCode: 0x21 },
  KeyG: { scanCode: 0x22 },
  KeyH: { scanCode: 0x23 },
  KeyJ: { scanCode: 0x24 },
  KeyK: { scanCode: 0x25 },
  KeyL: { scanCode: 0x26 },
  Semicolon: { scanCode: 0x27 },
  Quote: { scanCode: 0x28 },
  Backquote: { scanCode: 0x29 },
  ShiftLeft: { scanCode: 0x2a },
  Backslash: { scanCode: 0x2b },
  KeyZ: { scanCode: 0x2c },
  KeyX: { scanCode: 0x2d },
  KeyC: { scanCode: 0x2e },
  KeyV: { scanCode: 0x2f },
  KeyB: { scanCode: 0x30 },
  KeyN: { scanCode: 0x31 },
  KeyM: { scanCode: 0x32 },
  Comma: { scanCode: 0x33 },
  Period: { scanCode: 0x34 },
  Slash: { scanCode: 0x35 },
  ShiftRight: { scanCode: 0x36 },
  NumpadMultiply: { scanCode: 0x37 },
  AltLeft: { scanCode: 0x38 },
  AltRight: { scanCode: 0x38, extended: true },
  Space: { scanCode: 0x39 },
  CapsLock: { scanCode: 0x3a },
  F1: { scanCode: 0x3b },
  F2: { scanCode: 0x3c },
  F3: { scanCode: 0x3d },
  F4: { scanCode: 0x3e },
  F5: { scanCode: 0x3f },
  F6: { scanCode: 0x40 },
  F7: { scanCode: 0x41 },
  F8: { scanCode: 0x42 },
  F9: { scanCode: 0x43 },
  F10: { scanCode: 0x44 },
  NumLock: { scanCode: 0x45 },
  ScrollLock: { scanCode: 0x46 },
  Numpad7: { scanCode: 0x47 },
  Numpad8: { scanCode: 0x48 },
  Numpad9: { scanCode: 0x49 },
  NumpadSubtract: { scanCode: 0x4a },
  Numpad4: { scanCode: 0x4b },
  Numpad5: { scanCode: 0x4c },
  Numpad6: { scanCode: 0x4d },
  NumpadAdd: { scanCode: 0x4e },
  Numpad1: { scanCode: 0x4f },
  Numpad2: { scanCode: 0x50 },
  Numpad3: { scanCode: 0x51 },
  Numpad0: { scanCode: 0x52 },
  NumpadDecimal: { scanCode: 0x53 },
  F11: { scanCode: 0x57 },
  F12: { scanCode: 0x58 },
  NumpadEnter: { scanCode: 0x1c, extended: true },
  NumpadDivide: { scanCode: 0x35, extended: true },
  Home: { scanCode: 0x47, extended: true },
  ArrowUp: { scanCode: 0x48, extended: true },
  PageUp: { scanCode: 0x49, extended: true },
  ArrowLeft: { scanCode: 0x4b, extended: true },
  ArrowRight: { scanCode: 0x4d, extended: true },
  End: { scanCode: 0x4f, extended: true },
  ArrowDown: { scanCode: 0x50, extended: true },
  PageDown: { scanCode: 0x51, extended: true },
  Insert: { scanCode: 0x52, extended: true },
  Delete: { scanCode: 0x53, extended: true },
  MetaLeft: { scanCode: 0x5b, extended: true },
  MetaRight: { scanCode: 0x5c, extended: true },
  ContextMenu: { scanCode: 0x5d, extended: true },
};

export function mapKeyboardCodeToRdp(code: string): RdpKeyMapping | null {
  return KEY_CODE_TO_SCANCODE[code] ?? null;
}

export function buildRdpKeyEvent(
  event: KeyboardEvent,
  type: "key-down" | "key-up",
): RdpInputEvent | null {
  const mapping = mapKeyboardCodeToRdp(event.code);
  if (!mapping) return null;
  return {
    type,
    scanCode: mapping.scanCode,
    extended: mapping.extended ?? false,
    repeat: event.repeat,
  };
}
