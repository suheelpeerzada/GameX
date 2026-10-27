import { Channel } from "@tauri-apps/api/core";
import { ArrowLeft, Circle, Crosshair, MonitorPlay, MonitorStop } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "../lib/api";

interface Setup {
  codec: string;
  width: number;
  height: number;
  deviceWidth: number;
  deviceHeight: number;
}

/** An object drawn over the screen in pick mode, in device pixels. */
export interface PickMarker {
  key: string;
  x: number;
  y: number;
  label: string;
  state?: "hover" | "selected";
  /** The marker's color when it isn't hovered or selected (one per class). */
  color?: string;
}

/** Pick mode: hovering and clicking find things instead of touching the game. */
export interface PickMode {
  active: boolean;
  markers: PickMarker[];
  /** A highlighted view's bounds (left, top, right, bottom). */
  box?: [number, number, number, number] | null;
  onToggle: () => void;
  onMove: (p: { x: number; y: number } | null) => void;
  onPick: (p: { x: number; y: number }) => void;
}

/**
 * The running game's screen, streamed from the emulator (H.264 decoded with
 * WebCodecs). Clicks, drags and long presses on it are sent back as touches.
 */
/** `visible`: stream only while the Live tab is on screen. */
export default function GameScreen({ projectId, visible, pick }: { projectId: string; visible: boolean; pick?: PickMode }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [setup, setSetup] = useState<Setup | null>(null);
  const [streaming, setStreaming] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const press = useRef<{ x: number; y: number; t: number } | null>(null);
  const setupRef = useRef<Setup | null>(null);
  // Where the canvas sits in its box, for the pick overlay.
  const [canvasBox, setCanvasBox] = useState<{ left: number; top: number; width: number; height: number } | null>(null);
  const picking = !!pick?.active;

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const measure = () =>
      setCanvasBox({ left: canvas.offsetLeft, top: canvas.offsetTop, width: canvas.offsetWidth, height: canvas.offsetHeight });
    const observer = new ResizeObserver(measure);
    observer.observe(canvas);
    measure();
    return () => observer.disconnect();
  }, [setup]);

  // Ctrl+Shift+C toggles pick mode, as in a browser's developer tools.
  const togglePick = pick?.onToggle;
  useEffect(() => {
    if (!visible || !togglePick) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "c") {
        e.preventDefault();
        togglePick();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [visible, togglePick]);

  const stop = () => {
    api.mirrorStop(projectId);
    setStreaming(false);
  };

  const start = async () => {
    setError(null);
    if (typeof VideoDecoder === "undefined") {
      setError("This window can't decode video (WebCodecs is missing).");
      return;
    }
    let decoder: VideoDecoder | null = null;
    let haveKey = false;
    let timestamp = 0;

    const configure = (s: Setup) => {
      decoder?.close();
      decoder = new VideoDecoder({
        output: (frame) => {
          const canvas = canvasRef.current;
          const ctx = canvas?.getContext("2d");
          if (canvas && ctx) ctx.drawImage(frame, 0, 0, canvas.width, canvas.height);
          frame.close();
        },
        error: (e) => {
          // Recover on the next key frame instead of giving up.
          haveKey = false;
          setError(`Video decoding hiccup: ${e.message}`);
        },
      });
      // No description: chunks are Annex B, as screenrecord sends them.
      decoder.configure({ codec: s.codec, optimizeForLatency: true });
    };

    const channel = new Channel<unknown>();
    channel.onmessage = (message) => {
      if (typeof message === "string" || (message && typeof message === "object" && !(message instanceof ArrayBuffer) && !ArrayBuffer.isView(message) && !Array.isArray(message))) {
        const meta = typeof message === "string" ? JSON.parse(message) : (message as { type: string });
        if (meta.type === "setup") {
          const s = meta as unknown as Setup;
          setupRef.current = s;
          setSetup(s);
          const canvas = canvasRef.current;
          if (canvas) {
            canvas.width = s.width;
            canvas.height = s.height;
          }
          configure(s);
        } else if (meta.type === "ended") {
          setError(String((meta as { reason?: string }).reason ?? "The screen stream ended."));
          setStreaming(false);
        }
        return;
      }
      const bytes =
        message instanceof ArrayBuffer
          ? new Uint8Array(message)
          : ArrayBuffer.isView(message)
            ? new Uint8Array(message.buffer, message.byteOffset, message.byteLength)
            : new Uint8Array(message as number[]);
      if (bytes.length < 2) return;
      const key = bytes[0] === 1;
      if (!key && !haveKey) return; // wait for a key frame
      if (decoder === null || (decoder as VideoDecoder).state === "closed") {
        if (!key || !setupRef.current) return;
        configure(setupRef.current);
      }
      if (key) {
        haveKey = true;
        setError(null);
      }
      timestamp += 33_333;
      (decoder as unknown as VideoDecoder).decode(new EncodedVideoChunk({ type: key ? "key" : "delta", timestamp, data: bytes.subarray(1) }));
    };

    try {
      await api.mirrorStart(projectId, channel);
      setStreaming(true);
    } catch (e) {
      setError(String(e));
    }
  };

  // Stream while the Live tab is visible; stop when it's hidden or closed.
  useEffect(() => {
    if (!visible) return;
    start();
    return () => {
      api.mirrorStop(projectId);
      setStreaming(false);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId, visible]);

  const toDevice = (e: React.PointerEvent) => {
    const canvas = canvasRef.current;
    const s = setupRef.current;
    if (!canvas || !s) return null;
    const rect = canvas.getBoundingClientRect();
    const x = Math.round(((e.clientX - rect.left) / rect.width) * s.deviceWidth);
    const y = Math.round(((e.clientY - rect.top) / rect.height) * s.deviceHeight);
    return { x: Math.max(0, Math.min(s.deviceWidth - 1, x)), y: Math.max(0, Math.min(s.deviceHeight - 1, y)) };
  };

  const onDown = (e: React.PointerEvent) => {
    if (picking) return;
    const p = toDevice(e);
    if (!p) return;
    (e.target as Element).setPointerCapture(e.pointerId);
    press.current = { ...p, t: performance.now() };
  };

  const onUp = (e: React.PointerEvent) => {
    if (picking) {
      const p = toDevice(e);
      if (p) pick!.onPick(p);
      return;
    }
    const start = press.current;
    press.current = null;
    const end = toDevice(e);
    if (!start || !end) return;
    const held = Math.round(performance.now() - start.t);
    const moved = Math.hypot(end.x - start.x, end.y - start.y) > 12;
    const to = moved ? end : start;
    api.mirrorTouch(projectId, start.x, start.y, to.x, to.y, Math.max(held, 60)).catch((err) => setError(String(err)));
  };

  return (
    <div className="game-screen">
      <div className="game-screen-head">
        <strong>Game</strong>
        <span className="status-spacer" />
        {pick && (
          <button
            className={picking ? "icon-button small-icon active" : "icon-button small-icon"}
            title="Select an element in the game to inspect it (Ctrl+Shift+C)"
            onClick={pick.onToggle}
            disabled={!setup}
          >
            <Crosshair size={15} />
          </button>
        )}
        <button className="icon-button small-icon" title="Back" onClick={() => api.mirrorKey(projectId, 4).catch(() => {})}>
          <ArrowLeft size={15} />
        </button>
        <button className="icon-button small-icon" title="Home" onClick={() => api.mirrorKey(projectId, 3).catch(() => {})}>
          <Circle size={14} />
        </button>
        {streaming ? (
          <button className="icon-button small-icon" title="Stop showing the screen" onClick={stop}>
            <MonitorStop size={15} />
          </button>
        ) : (
          <button className="icon-button small-icon" title="Show the screen" onClick={start}>
            <MonitorPlay size={15} />
          </button>
        )}
      </div>
      <div className="game-screen-body">
        <canvas
          ref={canvasRef}
          className={picking ? "game-canvas picking" : "game-canvas"}
          width={setup?.width ?? 360}
          height={setup?.height ?? 640}
          onPointerDown={onDown}
          onPointerUp={onUp}
          onPointerCancel={() => (press.current = null)}
          onPointerMove={picking ? (e) => pick!.onMove(toDevice(e)) : undefined}
          onPointerLeave={picking ? () => pick!.onMove(null) : undefined}
          style={setup ? { aspectRatio: `${setup.width} / ${setup.height}` } : undefined}
        />
        {picking && setup && canvasBox && (
          <svg
            className="pick-overlay"
            style={canvasBox}
            viewBox={`0 0 ${setup.deviceWidth} ${setup.deviceHeight}`}
            preserveAspectRatio="none"
          >
            {pick!.box && (
              <rect
                className="pick-box"
                x={pick!.box[0]}
                y={pick!.box[1]}
                width={pick!.box[2] - pick!.box[0]}
                height={pick!.box[3] - pick!.box[1]}
              />
            )}
            {pick!.markers
              .filter((m) => m.x >= -20 && m.y >= -20 && m.x <= setup.deviceWidth + 20 && m.y <= setup.deviceHeight + 20)
              .map((m) => (
                <g key={m.key} className={m.state ? `pick-marker ${m.state}` : "pick-marker"}>
                  <circle
                    cx={m.x}
                    cy={m.y}
                    r={m.state ? 22 : 12}
                    style={!m.state && m.color ? { stroke: m.color, fill: `${m.color}40` } : undefined}
                  />
                  {m.state && (
                    <text x={m.x} y={m.y - 32} fontSize={setup.deviceWidth / 26}>
                      {m.label}
                    </text>
                  )}
                </g>
              ))}
          </svg>
        )}
        {!setup && !error && <div className="game-screen-note muted small">Starting the screen…</div>}
        {error && <div className="game-screen-note small error-text">{error}</div>}
      </div>
      <div className="muted tiny game-screen-tip">
        {picking
          ? "Pick mode: hover to find objects, click to select one. Taps aren't sent to the game (Ctrl+Shift+C to leave)."
          : "Click to tap, drag to swipe, hold for a long press. The screen updates when the game draws something new."}
      </div>
    </div>
  );
}
