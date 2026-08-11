import { Channel } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Maximize2, Monitor, Power, RotateCcw, Send, ShieldAlert } from "lucide-react";
import {
  memo,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Button } from "@/components/ui/button";
import { invoke } from "@/lib/invoke";
import { decodeRdpFramePatch, type RdpFramePatch } from "@/lib/rdpFrame";
import { buildRdpKeyEvent, type RdpInputEvent } from "@/lib/rdpInput";
import { mapClientPointToRdpPixel } from "@/lib/rdpViewport";
import type { RdpSessionPane } from "@/types/global";

type RdpSessionState =
  | "connecting"
  | "certificate_verification"
  | "authenticating"
  | "negotiating"
  | "active"
  | "reconnecting"
  | "disconnected"
  | "failed";

interface RdpStatePayload {
  sessionId: string;
  state: RdpSessionState;
  message?: string | null;
}

interface RdpPaneHostProps {
  pane: RdpSessionPane;
  active: boolean;
  visible: boolean;
  onDisconnectedCloseRequested?: () => void;
  onConnectionError?: (sessionId: string, error: string) => void;
}

function getCanvasPoint(canvas: HTMLCanvasElement, event: PointerEvent | WheelEvent) {
  const rect = canvas.getBoundingClientRect();
  return mapClientPointToRdpPixel(rect, canvas.width, canvas.height, event.clientX, event.clientY);
}

function buttonName(button: number): Extract<RdpInputEvent, { type: "mouse-button" }>["button"] {
  if (button === 1) return "middle";
  if (button === 2) return "right";
  if (button === 3) return "back";
  if (button === 4) return "forward";
  return "left";
}

function ensureCanvasSize(canvas: HTMLCanvasElement, width: number, height: number) {
  if (canvas.width === width && canvas.height === height) return false;
  canvas.width = width;
  canvas.height = height;
  return true;
}

function copyPatchToRgba(patch: RdpFramePatch, target: Uint8ClampedArray | Uint8Array) {
  const rowBytes = patch.width * 4;
  for (let row = 0; row < patch.height; row += 1) {
    const srcRow = row * patch.stride;
    const dstRow = row * rowBytes;
    if (patch.pixelFormat === "RGBA8888") {
      target.set(patch.payload.subarray(srcRow, srcRow + rowBytes), dstRow);
      continue;
    }
    for (let src = srcRow, dst = dstRow; src < srcRow + rowBytes; src += 4, dst += 4) {
      target[dst] = patch.payload[src + 2] ?? 0;
      target[dst + 1] = patch.payload[src + 1] ?? 0;
      target[dst + 2] = patch.payload[src] ?? 0;
      target[dst + 3] = patch.payload[src + 3] ?? 255;
    }
  }
}

interface RdpCanvasRenderer {
  draw(patch: RdpFramePatch): void;
  dispose(): void;
}

class Canvas2dRdpRenderer implements RdpCanvasRenderer {
  private readonly ctx: CanvasRenderingContext2D;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    ctx: CanvasRenderingContext2D,
  ) {
    this.ctx = ctx;
  }

  draw(patch: RdpFramePatch) {
    ensureCanvasSize(this.canvas, patch.desktopWidth, patch.desktopHeight);
    const imageData = this.ctx.createImageData(patch.width, patch.height);
    copyPatchToRgba(patch, imageData.data);
    this.ctx.putImageData(imageData, patch.x, patch.y);
  }

  dispose() {}
}

class WebGl2RdpRenderer implements RdpCanvasRenderer {
  private readonly program: WebGLProgram;
  private readonly texture: WebGLTexture;
  private readonly positionBuffer: WebGLBuffer;
  private readonly texCoordBuffer: WebGLBuffer;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly gl: WebGL2RenderingContext,
  ) {
    const vertexShader = compileShader(
      gl,
      gl.VERTEX_SHADER,
      `#version 300 es
      in vec2 a_position;
      in vec2 a_texCoord;
      out vec2 v_texCoord;
      void main() {
        gl_Position = vec4(a_position, 0.0, 1.0);
        v_texCoord = a_texCoord;
      }`,
    );
    const fragmentShader = compileShader(
      gl,
      gl.FRAGMENT_SHADER,
      `#version 300 es
      precision mediump float;
      in vec2 v_texCoord;
      uniform sampler2D u_texture;
      out vec4 outColor;
      void main() {
        outColor = texture(u_texture, v_texCoord);
      }`,
    );

    const program = gl.createProgram();
    const texture = gl.createTexture();
    const positionBuffer = gl.createBuffer();
    const texCoordBuffer = gl.createBuffer();
    if (!program || !texture || !positionBuffer || !texCoordBuffer) {
      throw new Error("Unable to create RDP WebGL resources");
    }

    gl.attachShader(program, vertexShader);
    gl.attachShader(program, fragmentShader);
    gl.linkProgram(program);
    gl.deleteShader(vertexShader);
    gl.deleteShader(fragmentShader);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      const message = gl.getProgramInfoLog(program) || "Unable to link RDP WebGL program";
      gl.deleteProgram(program);
      throw new Error(message);
    }

    this.program = program;
    this.texture = texture;
    this.positionBuffer = positionBuffer;
    this.texCoordBuffer = texCoordBuffer;

    activateWebGlProgram(gl, this.program);
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);

    const positionLocation = gl.getAttribLocation(this.program, "a_position");
    gl.bindBuffer(gl.ARRAY_BUFFER, this.positionBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(positionLocation);
    gl.vertexAttribPointer(positionLocation, 2, gl.FLOAT, false, 0, 0);

    const texCoordLocation = gl.getAttribLocation(this.program, "a_texCoord");
    gl.bindBuffer(gl.ARRAY_BUFFER, this.texCoordBuffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([0, 1, 1, 1, 0, 0, 1, 0]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(texCoordLocation);
    gl.vertexAttribPointer(texCoordLocation, 2, gl.FLOAT, false, 0, 0);
  }

  draw(patch: RdpFramePatch) {
    const resized = ensureCanvasSize(this.canvas, patch.desktopWidth, patch.desktopHeight);
    const gl = this.gl;
    activateWebGlProgram(gl, this.program);
    gl.viewport(0, 0, patch.desktopWidth, patch.desktopHeight);
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    if (resized) {
      gl.texImage2D(
        gl.TEXTURE_2D,
        0,
        gl.RGBA,
        patch.desktopWidth,
        patch.desktopHeight,
        0,
        gl.RGBA,
        gl.UNSIGNED_BYTE,
        null,
      );
    }

    const rowBytes = patch.width * 4;
    const requiredBytes = rowBytes * patch.height;
    const canUploadDirectly = patch.pixelFormat === "RGBA8888" && patch.stride === rowBytes;
    const patchBytes = canUploadDirectly
      ? patch.payload.subarray(0, requiredBytes)
      : new Uint8Array(requiredBytes);
    if (!canUploadDirectly) copyPatchToRgba(patch, patchBytes);

    gl.texSubImage2D(
      gl.TEXTURE_2D,
      0,
      patch.x,
      patch.y,
      patch.width,
      patch.height,
      gl.RGBA,
      gl.UNSIGNED_BYTE,
      patchBytes,
    );
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }

  dispose() {
    const gl = this.gl;
    gl.deleteTexture(this.texture);
    gl.deleteBuffer(this.positionBuffer);
    gl.deleteBuffer(this.texCoordBuffer);
    gl.deleteProgram(this.program);
  }
}

function compileShader(gl: WebGL2RenderingContext, type: number, source: string) {
  const shader = gl.createShader(type);
  if (!shader) throw new Error("Unable to create RDP WebGL shader");
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const message = gl.getShaderInfoLog(shader) || "Unable to compile RDP WebGL shader";
    gl.deleteShader(shader);
    throw new Error(message);
  }
  return shader;
}

function activateWebGlProgram(gl: WebGL2RenderingContext, program: WebGLProgram) {
  const activateProgram = gl.useProgram.bind(gl);
  activateProgram(program);
}

function createRdpRenderer(canvas: HTMLCanvasElement): RdpCanvasRenderer | null {
  const gl = canvas.getContext("webgl2", {
    alpha: false,
    antialias: false,
    depth: false,
    stencil: false,
  });
  if (gl) {
    try {
      return new WebGl2RdpRenderer(canvas, gl);
    } catch {
      // Fall through to Canvas 2D when WebGL is unavailable or blocked by the host.
    }
  }
  const ctx = canvas.getContext("2d", { alpha: false });
  return ctx ? new Canvas2dRdpRenderer(canvas, ctx) : null;
}

function statusLabel(state: RdpSessionState, message?: string | null) {
  if (message) return message;
  switch (state) {
    case "certificate_verification":
      return "Verifying certificate";
    case "authenticating":
      return "Authenticating";
    case "negotiating":
      return "Initializing remote desktop";
    case "active":
      return "Connected";
    case "reconnecting":
      return "Reconnecting";
    case "disconnected":
      return "Disconnected";
    case "failed":
      return "Connection failed";
    default:
      return "Connecting";
  }
}

function RdpPaneHost({
  pane,
  active,
  visible,
  onDisconnectedCloseRequested,
  onConnectionError,
}: RdpPaneHostProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const rendererRef = useRef<RdpCanvasRenderer | null>(null);
  const pressedKeysRef = useRef(new Set<string>());
  const pendingMouseMoveRef = useRef<{ x: number; y: number } | null>(null);
  const mouseRafRef = useRef<number | null>(null);
  const [state, setState] = useState<RdpSessionState>(pane.connectError ? "failed" : "connecting");
  const [message, setMessage] = useState<string | null>(pane.connectError ?? null);
  const [desktopSize, setDesktopSize] = useState({
    width: pane.display?.remoteWidth ?? 1920,
    height: pane.display?.remoteHeight ?? 1080,
  });

  const sendInputBatch = useCallback(
    async (events: RdpInputEvent[]) => {
      if (events.length === 0 || pane.connecting || pane.connectError) return;
      await invoke("rdp_input_batch", { sessionId: pane.sessionId, events }).catch(() => {});
    },
    [pane.connectError, pane.connecting, pane.sessionId],
  );

  const releaseAllKeys = useCallback(() => {
    if (pressedKeysRef.current.size === 0) return;
    pressedKeysRef.current.clear();
    void sendInputBatch([{ type: "release-all-keys" }]);
  }, [sendInputBatch]);

  useEffect(() => {
    const channel = new Channel<ArrayBuffer>((frame) => {
      const patch = decodeRdpFramePatch(frame);
      setDesktopSize({ width: patch.desktopWidth, height: patch.desktopHeight });
      const canvas = canvasRef.current;
      if (!canvas) return;
      rendererRef.current ??= createRdpRenderer(canvas);
      rendererRef.current?.draw(patch);
    });

    if (!pane.connecting && !pane.connectError) {
      void invoke("rdp_attach_frame_channel", {
        sessionId: pane.sessionId,
        frameChannel: channel,
      });
    }
  }, [pane.connectError, pane.connecting, pane.sessionId]);

  useEffect(() => {
    return () => {
      rendererRef.current?.dispose();
      rendererRef.current = null;
    };
  }, []);

  useEffect(() => {
    const unlisten = listen<RdpStatePayload>(`rdp-state-${pane.sessionId}`, (event) => {
      setState(event.payload.state);
      setMessage(event.payload.message ?? null);
      if (event.payload.state === "failed") {
        onConnectionError?.(pane.sessionId, event.payload.message ?? "RDP connection failed");
      }
    });
    return () => {
      void unlisten.then((dispose) => dispose());
    };
  }, [onConnectionError, pane.sessionId]);

  useEffect(() => {
    if (!active || !visible) releaseAllKeys();
  }, [active, releaseAllKeys, visible]);

  useEffect(() => {
    window.addEventListener("blur", releaseAllKeys);
    return () => {
      window.removeEventListener("blur", releaseAllKeys);
      releaseAllKeys();
    };
  }, [releaseAllKeys]);

  const handleKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>) => {
      const inputEvent = buildRdpKeyEvent(event.nativeEvent, "key-down");
      if (!inputEvent) return;
      event.preventDefault();
      event.stopPropagation();
      pressedKeysRef.current.add(event.code);
      void sendInputBatch([inputEvent]);
    },
    [sendInputBatch],
  );

  const handleKeyUp = useCallback(
    (event: ReactKeyboardEvent<HTMLDivElement>) => {
      const inputEvent = buildRdpKeyEvent(event.nativeEvent, "key-up");
      if (!inputEvent) return;
      event.preventDefault();
      event.stopPropagation();
      pressedKeysRef.current.delete(event.code);
      void sendInputBatch([inputEvent]);
    },
    [sendInputBatch],
  );

  const flushMouseMove = useCallback(() => {
    mouseRafRef.current = null;
    const move = pendingMouseMoveRef.current;
    pendingMouseMoveRef.current = null;
    if (move) void sendInputBatch([{ type: "mouse-move", ...move }]);
  }, [sendInputBatch]);

  const queueMouseMove = useCallback(
    (event: ReactPointerEvent<HTMLCanvasElement>) => {
      const canvas = canvasRef.current;
      if (!canvas) return;
      pendingMouseMoveRef.current = getCanvasPoint(canvas, event.nativeEvent);
      if (mouseRafRef.current === null) {
        mouseRafRef.current = requestAnimationFrame(flushMouseMove);
      }
    },
    [flushMouseMove],
  );

  const scaleStyle = useMemo(
    () => ({
      aspectRatio: `${desktopSize.width} / ${desktopSize.height}`,
      maxWidth: "100%",
      maxHeight: "100%",
    }),
    [desktopSize.height, desktopSize.width],
  );

  const sendShortcut = (events: RdpInputEvent[]) => {
    void sendInputBatch(events);
  };

  return (
    <div
      className="group relative flex h-full w-full min-h-0 min-w-0 items-center justify-center overflow-hidden bg-black outline-none"
      tabIndex={active ? 0 : -1}
      onKeyDown={handleKeyDown}
      onKeyUp={handleKeyUp}
      onBlur={releaseAllKeys}
    >
      <canvas
        ref={canvasRef}
        className="block object-contain"
        style={scaleStyle}
        onPointerMove={queueMouseMove}
        onPointerDown={(event) => {
          const canvas = canvasRef.current;
          if (!canvas) return;
          event.currentTarget.setPointerCapture(event.pointerId);
          const point = getCanvasPoint(canvas, event.nativeEvent);
          void sendInputBatch([
            { type: "mouse-button", button: buttonName(event.button), pressed: true, ...point },
          ]);
        }}
        onPointerUp={(event) => {
          const canvas = canvasRef.current;
          if (!canvas) return;
          const point = getCanvasPoint(canvas, event.nativeEvent);
          void sendInputBatch([
            { type: "mouse-button", button: buttonName(event.button), pressed: false, ...point },
          ]);
        }}
        onWheel={(event) => {
          const canvas = canvasRef.current;
          if (!canvas) return;
          event.preventDefault();
          const point = getCanvasPoint(canvas, event.nativeEvent);
          void sendInputBatch([
            { type: "mouse-wheel", deltaX: event.deltaX, deltaY: event.deltaY, ...point },
          ]);
        }}
      />

      <div className="absolute left-2 top-2 flex items-center gap-1 rounded border border-white/15 bg-black/65 px-2 py-1 text-xs text-white opacity-0 transition-opacity group-hover:opacity-100">
        <Monitor className="h-3.5 w-3.5" />
        <span className="max-w-40 truncate">{pane.name}</span>
        <span className="text-white/55">
          {desktopSize.width}x{desktopSize.height}
        </span>
        <Button
          variant="ghost"
          size="icon-xs"
          className="h-6 w-6 text-white"
          onClick={() =>
            sendShortcut([
              { type: "key-down", scanCode: 0x1d, extended: false, repeat: false },
              { type: "key-down", scanCode: 0x38, extended: false, repeat: false },
              { type: "key-down", scanCode: 0x53, extended: true, repeat: false },
              { type: "key-up", scanCode: 0x53, extended: true, repeat: false },
              { type: "key-up", scanCode: 0x38, extended: false, repeat: false },
              { type: "key-up", scanCode: 0x1d, extended: false, repeat: false },
            ])
          }
        >
          <Send className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="icon-xs"
          className="h-6 w-6 text-white"
          onClick={() => void invoke("rdp_reconnect", { sessionId: pane.sessionId })}
        >
          <RotateCcw className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="icon-xs"
          className="h-6 w-6 text-white"
          onClick={onDisconnectedCloseRequested}
        >
          <Power className="h-3.5 w-3.5" />
        </Button>
        <Maximize2 className="h-3.5 w-3.5 text-white/50" />
      </div>

      {state !== "active" && (
        <div className="absolute inset-0 flex items-center justify-center bg-black/45 text-white">
          <div className="flex items-center gap-3 rounded border border-white/15 bg-black/70 px-4 py-3 text-sm">
            <ShieldAlert className="h-5 w-5 text-sky-300" />
            <span>{statusLabel(state, message)}</span>
          </div>
        </div>
      )}
    </div>
  );
}

export default memo(RdpPaneHost);
