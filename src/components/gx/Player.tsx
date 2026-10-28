import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import type { Behavior, Scene, Thing } from "../../lib/gx";
import { behaviorsUsed, createWorld, step, type Actor, type Control, type Input, type World } from "../../lib/gxengine";
import { Assets, build, dispose } from "./SceneView";

interface Props {
  scene: Scene;
  things: Record<string, Thing>;
  pixelsPerUnit: number;
  readAsset: (asset: string) => Promise<ArrayBuffer>;
  loadBehavior: (id: string) => Promise<Behavior>;
  /** A scene by its ID or name (for "go to scene"); null if there's none. */
  findScene: (idOrName: string) => Promise<Scene | null>;
  onStop: () => void;
}

interface Hud {
  ready: boolean;
  hasPlayer: boolean;
  control: Control;
  score: number;
  health: number;
  maxHealth: number;
  items: number;
  collected: number;
  over: World["over"];
  message: string;
}

const STEP = 1 / 60;

const START: Hud = { ready: false, hasPlayer: false, control: "third", score: 0, health: 0, maxHealth: 0, items: 0, collected: 0, over: null, message: "" };

const HINTS: Record<Control, string> = {
  platformer: "A/D or ←/→ move · Space, W or ↑ jump",
  topdown: "WASD or arrows move",
  third: "WASD move · Space jump · drag to look · scroll to zoom",
};

function audioType(asset: string): string {
  const ext = asset.toLowerCase().split(".").pop();
  return ext === "ogg" ? "audio/ogg" : ext === "mp3" ? "audio/mpeg" : ext === "wav" ? "audio/wav" : "";
}

/**
 * The GameX engine playing a scene: the layer runs on its own, without the
 * original game. Esc goes back to editing; R starts again.
 */
export default function Player({ scene: first, things, pixelsPerUnit, readAsset, loadBehavior, findScene, onStop }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const [scene, setScene] = useState(first);
  const [run, setRun] = useState(0);
  const [hud, setHud] = useState<Hud>(START);
  const [error, setError] = useState<string | null>(null);
  const calls = useRef({ readAsset, loadBehavior, findScene, onStop });
  calls.current = { readAsset, loadBehavior, findScene, onStop };

  // Esc and R, wherever the focus is.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        calls.current.onStop();
      } else if (e.key.toLowerCase() === "r" && !e.ctrlKey && !e.metaKey) {
        e.preventDefault();
        setRun((n) => n + 1);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    let stopped = false;
    let stop: (() => void) | null = null;
    setHud(START);
    setError(null);
    (async () => {
      const ids = behaviorsUsed(scene, things);
      const loaded = await Promise.all(ids.map((id) => calls.current.loadBehavior(id).catch(() => null)));
      if (stopped) return;
      const behaviors = Object.fromEntries(loaded.filter((b): b is Behavior => b !== null).map((b) => [b.id, b]));
      stop = play(host.current!, scene, things, behaviors, pixelsPerUnit, {
        readAsset: (a) => calls.current.readAsset(a),
        onHud: setHud,
        onScene: (name) =>
          calls.current
            .findScene(name)
            .then((next) => (next ? setScene(next) : setError(`There's no scene called "${name}".`)))
            .catch((e) => setError(String(e))),
      });
    })().catch((e) => setError(String(e)));
    return () => {
      stopped = true;
      stop?.();
    };
  }, [scene, things, pixelsPerUnit, run]);

  const restart = () => setRun((n) => n + 1);

  return (
    <div className="gx-player">
      <div ref={host} className="gx-canvas" tabIndex={0} />
      <div className="gx-hud">
        <div className="gx-hud-stats">
          {hud.hasPlayer && (
            <div className="gx-health" title={`Health ${Math.ceil(hud.health)} of ${hud.maxHealth}`}>
              <div style={{ width: `${hud.maxHealth ? (100 * hud.health) / hud.maxHealth : 0}%` }} />
            </div>
          )}
          <span>
            Score <strong>{hud.score}</strong>
            {hud.items > 0 && (
              <span className="muted">
                {" "}
                · {hud.collected}/{hud.items} collected
              </span>
            )}
          </span>
          {hud.ready && !hud.hasPlayer && <span className="muted">No player found in this scene: watching it run.</span>}
        </div>
        <span className="muted small">
          {hud.hasPlayer ? HINTS[hud.control] : "Drag to look around"} · <strong>R</strong> restart · <strong>Esc</strong> back to editing
        </span>
      </div>
      {!hud.ready && !error && <div className="gx-over muted">Starting…</div>}
      {error && (
        <div className="gx-over">
          <p className="bad">{error}</p>
          <button onClick={onStop}>Back to editing</button>
        </div>
      )}
      {hud.over && (
        <div className="gx-over">
          <h2>{hud.message || (hud.over === "won" ? "You win!" : "Game over")}</h2>
          <p>Score {hud.score}</p>
          <div className="row">
            <button className="primary" onClick={restart}>
              Play again (R)
            </button>
            <button onClick={onStop}>Back to editing (Esc)</button>
          </div>
        </div>
      )}
    </div>
  );
}

interface Hooks {
  readAsset: (asset: string) => Promise<ArrayBuffer>;
  onHud: (hud: Hud) => void;
  onScene: (name: string) => void;
}

/** Runs the scene in `el` until the returned function is called. */
function play(el: HTMLElement, scene: Scene, things: Record<string, Thing>, behaviors: Record<string, Behavior>, ppu: number, hooks: Hooks): () => void {
  const flat = scene.dimensions === "2d";
  const { world: w, statics } = createWorld(scene, things, behaviors, ppu);
  const assets = new Assets(hooks.readAsset);

  const renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setPixelRatio(window.devicePixelRatio);
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  el.appendChild(renderer.domElement);
  const root = new THREE.Scene();
  root.background = new THREE.Color(scene.environment.background);
  root.add(new THREE.AmbientLight(new THREE.Color(scene.environment.ambient), 2.5));
  const lit = scene.entities.some((e) => [e, ...(e.children ?? [])].some((x) => x.parts?.some((p) => p.type === "light")));
  root.add(new THREE.HemisphereLight(0xffffff, 0x404858, lit ? 0.8 : 1.5));
  const camera = flat ? new THREE.OrthographicCamera(-8, 8, 6, -6, -100, 100) : new THREE.PerspectiveCamera(60, 1, 0.05, 2000);

  // The level stays put; each actor is drawn on its own so it can move.
  const index = new Map<string, THREE.Object3D>();
  for (const e of statics) root.add(build(e, things, flat, false, index, false, assets));
  const groups = new Map<string, THREE.Object3D>();
  const addActor = (a: Actor) => {
    const g = build(a.entity, things, flat, false, index, false, assets);
    g.userData.base = { quaternion: g.quaternion.clone(), scaleX: g.scale.x };
    groups.set(a.id, g);
    root.add(g);
  };
  w.actors.forEach(addActor);

  // Input.
  const held = new Set<string>();
  let pressed: string[] = [];
  const GAME_KEYS = new Set(["arrowleft", "arrowright", "arrowup", "arrowdown", " "]);
  const onDown = (e: KeyboardEvent) => {
    if ((e.target as HTMLElement).closest?.("input, textarea, select")) return;
    const k = e.key.toLowerCase();
    if (GAME_KEYS.has(k)) e.preventDefault();
    if (!e.repeat) pressed.push(k);
    held.add(k);
  };
  const onUp = (e: KeyboardEvent) => held.delete(e.key.toLowerCase());
  const onBlur = () => held.clear();
  window.addEventListener("keydown", onDown);
  window.addEventListener("keyup", onUp);
  window.addEventListener("blur", onBlur);

  // The 3D camera: behind the player, turned by dragging; free to orbit with no player.
  let yaw = 0;
  let pitch = 0.35;
  let distance = w.player ? Math.max(4, w.player.half[1] * 2 * 3.2) : 10;
  let orbit: OrbitControls | null = null;
  let drag: { x: number; y: number } | null = null;
  const onPointerDown = (e: PointerEvent) => {
    drag = { x: e.clientX, y: e.clientY };
    renderer.domElement.setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: PointerEvent) => {
    if (!drag) return;
    yaw -= (e.clientX - drag.x) * 0.006;
    pitch = Math.min(1.3, Math.max(-0.25, pitch + (e.clientY - drag.y) * 0.004));
    drag = { x: e.clientX, y: e.clientY };
  };
  const onPointerUp = () => (drag = null);
  const onWheel = (e: WheelEvent) => {
    e.preventDefault();
    distance = Math.min(60, Math.max(1.5, distance * (e.deltaY > 0 ? 1.1 : 0.9)));
  };
  if (!flat && w.player) {
    renderer.domElement.addEventListener("pointerdown", onPointerDown);
    renderer.domElement.addEventListener("pointermove", onPointerMove);
    renderer.domElement.addEventListener("pointerup", onPointerUp);
    renderer.domElement.addEventListener("wheel", onWheel, { passive: false });
  } else if (!flat) {
    orbit = new OrbitControls(camera, renderer.domElement);
    orbit.enableDamping = true;
    const box = new THREE.Box3().setFromObject(root);
    const center = box.isEmpty() ? new THREE.Vector3() : box.getCenter(new THREE.Vector3());
    const radius = box.isEmpty() ? 5 : Math.max(box.getSize(new THREE.Vector3()).length() / 2, 1);
    camera.position.copy(center).add(new THREE.Vector3(0.55, 0.6, 1).normalize().multiplyScalar(radius * 2));
    orbit.target.copy(center);
  }

  const input = (): Input => {
    const has = (...keys: string[]) => keys.some((k) => held.has(k));
    const x = (has("d", "arrowright") ? 1 : 0) - (has("a", "arrowleft") ? 1 : 0);
    const y = (has("w", "arrowup") ? 1 : 0) - (has("s", "arrowdown") ? 1 : 0);
    const jumpKeys = w.control === "platformer" ? [" ", "w", "arrowup"] : [" "];
    const jump = has(...jumpKeys);
    const jumpPressed = pressed.some((k) => jumpKeys.includes(k));
    let move: [number, number, number] = [x, 0, 0];
    if (w.control === "topdown") move = [x, y, 0];
    if (w.control === "third") {
      // Forward is away from the camera.
      const fx = -Math.sin(yaw) * y + Math.cos(yaw) * x;
      const fz = -Math.cos(yaw) * y - Math.sin(yaw) * x;
      move = [fx, 0, fz];
    }
    const len = Math.hypot(...move);
    if (len > 1) move = move.map((v) => v / len) as [number, number, number];
    return { move, jump, jumpPressed, pressed: [...pressed] };
  };

  let stopped = false;

  // Sounds.
  const playing = new Set<HTMLAudioElement>();
  const urls = new Map<string, Promise<string>>();
  const sound = (asset: string, volume: number, looped: boolean) => {
    let url = urls.get(asset);
    if (!url) {
      url = hooks.readAsset(asset).then((bytes) => URL.createObjectURL(new Blob([bytes], { type: audioType(asset) })));
      urls.set(asset, url);
    }
    url
      .then((u) => {
        if (stopped) return;
        const a = new Audio(u);
        a.volume = Math.min(1, Math.max(0, volume));
        a.loop = looped;
        playing.add(a);
        a.onended = () => playing.delete(a);
        a.play().catch(() => playing.delete(a));
      })
      .catch(() => {});
  };

  const resize = () => {
    const width = Math.max(1, el.clientWidth);
    const height = Math.max(1, el.clientHeight);
    renderer.setSize(width, height);
    if (camera instanceof THREE.PerspectiveCamera) camera.aspect = width / height;
    camera.updateProjectionMatrix();
  };
  const observer = new ResizeObserver(resize);
  observer.observe(el);
  resize();

  const up = new THREE.Vector3(0, 1, 0);
  const turn = new THREE.Quaternion();
  const placeCamera = () => {
    const aspect = el.clientWidth / Math.max(1, el.clientHeight);
    if (camera instanceof THREE.OrthographicCamera) {
      const v = w.view;
      const half = v.halfHeight ?? (v.size ? Math.max(v.size[1] / 2, v.size[0] / 2 / aspect, 1) * 1.05 : 5);
      camera.top = half;
      camera.bottom = -half;
      camera.left = -half * aspect;
      camera.right = half * aspect;
      camera.position.set(v.center[0], v.center[1], 50);
      camera.updateProjectionMatrix();
      return;
    }
    if (orbit) return orbit.update();
    const p = w.player!;
    const target = new THREE.Vector3(p.pos[0] + p.offset[0], p.pos[1] + p.offset[1] + p.half[1] * 0.6, p.pos[2] + p.offset[2]);
    camera.position.set(
      target.x + Math.sin(yaw) * Math.cos(pitch) * distance,
      target.y + Math.sin(pitch) * distance,
      target.z + Math.cos(yaw) * Math.cos(pitch) * distance,
    );
    camera.lookAt(target);
  };

  const sync = () => {
    for (const a of w.actors) {
      const g = groups.get(a.id);
      if (!g) continue;
      // Blinks while it can't be hurt again.
      const blink = a === w.player && w.hurt > 0 && Math.floor(w.time * 12) % 2 === 0;
      g.visible = a.alive && !blink;
      if (!a.alive) continue;
      g.position.fromArray(a.pos);
      const base = g.userData.base as { quaternion: THREE.Quaternion; scaleX: number };
      if (flat) g.scale.x = base.scaleX * (a.facing < 0 ? -1 : 1);
      else g.quaternion.copy(turn.setFromAxisAngle(up, a.facing)).multiply(base.quaternion);
    }
  };

  let last: Hud | null = null;
  const report = () => {
    const hud: Hud = {
      ready: true,
      hasPlayer: !!w.player,
      control: w.control,
      score: w.score,
      health: w.health,
      maxHealth: w.maxHealth,
      items: w.items,
      collected: w.collected,
      over: w.over,
      message: w.message,
    };
    if (last && (Object.keys(hud) as (keyof Hud)[]).every((k) => hud[k] === last![k])) return;
    last = hud;
    hooks.onHud(hud);
  };

  const events = () => {
    for (const e of w.events) {
      if (e.type === "sound") sound(e.asset, e.volume, e.looped);
      else if (e.type === "spawned") addActor(e.actor);
      else if (e.type === "removed") {
        const g = groups.get(e.id);
        if (g) {
          root.remove(g);
          dispose(g);
          groups.delete(e.id);
        }
      } else if (e.type === "scene") hooks.onScene(e.scene);
    }
    w.events.length = 0;
  };

  let then = performance.now();
  let pending = 0;
  renderer.setAnimationLoop((now) => {
    pending += Math.min(0.1, (now - then) / 1000);
    then = now;
    let steps = 0;
    while (pending >= STEP && steps < 5) {
      step(w, input(), STEP);
      pressed = [];
      pending -= STEP;
      steps++;
    }
    if (steps === 5) pending = 0;
    events();
    sync();
    placeCamera();
    renderer.render(root, camera);
    report();
  });

  return () => {
    stopped = true;
    renderer.setAnimationLoop(null);
    observer.disconnect();
    window.removeEventListener("keydown", onDown);
    window.removeEventListener("keyup", onUp);
    window.removeEventListener("blur", onBlur);
    renderer.domElement.removeEventListener("pointerdown", onPointerDown);
    renderer.domElement.removeEventListener("pointermove", onPointerMove);
    renderer.domElement.removeEventListener("pointerup", onPointerUp);
    renderer.domElement.removeEventListener("wheel", onWheel);
    orbit?.dispose();
    for (const a of playing) a.pause();
    urls.forEach((u) => u.then((x) => URL.revokeObjectURL(x)).catch(() => {}));
    dispose(root);
    assets.dispose();
    renderer.dispose();
    el.removeChild(renderer.domElement);
  };
}
