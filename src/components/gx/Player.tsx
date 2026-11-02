import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import type { Behavior, Scene, Thing } from "../../lib/gx";
import { behaviorsUsed, createWorld, step, type Actor, type Control, type Input, type World } from "../../lib/gxengine";
import { Assets, build, dispose, pixelRatio } from "./SceneView";
import { errorText } from "../../lib/errors";
import ErrorText from "../ErrorText";

/** How play runs, set by the buttons beside Play (and their keys). */
export interface PlayControls {
  paused: boolean;
  /** 0.25, 1 or 4. */
  speed: number;
  /** Bumped for each single step while paused. */
  steps: number;
  /** The camera is let go of the player and flies freely; the game gets no keys. */
  free: boolean;
  /** Bumped to start again. */
  restart: number;
}

interface Props {
  scene: Scene;
  things: Record<string, Thing>;
  pixelsPerUnit: number;
  controls: PlayControls;
  readAsset: (asset: string) => Promise<ArrayBuffer>;
  loadBehavior: (id: string) => Promise<Behavior>;
  /** A scene by its ID or name (for "go to scene"); null if there's none. */
  findScene: (idOrName: string) => Promise<Scene | null>;
  onStop: () => void;
  onRestart: () => void;
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

const FREE_HINT = "Free camera: drag to look · WASD fly · Q/E down and up · Shift faster · F8 back to the player";

function audioType(asset: string): string {
  const ext = asset.toLowerCase().split(".").pop();
  return ext === "ogg" ? "audio/ogg" : ext === "mp3" ? "audio/mpeg" : ext === "wav" ? "audio/wav" : "";
}

/**
 * The GameX engine playing a scene: the layer runs on its own, without the
 * original game. Every key goes to the game; the workspace's play keys
 * (Shift+F5 stop, Ctrl+Shift+F5 restart, F6 pause, F10 step, F8 free camera)
 * are the only ones taken.
 */
export default function Player({ scene: first, things, pixelsPerUnit, controls, readAsset, loadBehavior, findScene, onStop, onRestart }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const [scene, setScene] = useState(first);
  const [hud, setHud] = useState<Hud>(START);
  const [error, setError] = useState<string | null>(null);
  const calls = useRef({ readAsset, loadBehavior, findScene, onStop });
  calls.current = { readAsset, loadBehavior, findScene, onStop };
  const ctl = useRef(controls);
  ctl.current = controls;

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
      stop = play(host.current!, scene, things, behaviors, pixelsPerUnit, ctl, {
        readAsset: (a) => calls.current.readAsset(a),
        onHud: setHud,
        onScene: (name) =>
          calls.current
            .findScene(name)
            .then((next) => (next ? setScene(next) : setError(`There's no scene called "${name}".`)))
            .catch((e) => setError(errorText(e))),
      });
    })().catch((e) => setError(errorText(e)));
    return () => {
      stopped = true;
      stop?.();
    };
  }, [scene, things, pixelsPerUnit, controls.restart]);

  const hint = controls.free ? FREE_HINT : hud.hasPlayer ? HINTS[hud.control] : "Drag to look around";

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
          {controls.paused && <span className="gx-paused">Paused (F6) · F10 one step</span>}
          {controls.speed !== 1 && <span className="muted">×{controls.speed}</span>}
        </div>
        <span className="muted small">
          {hint} · <strong>Shift+F5</strong> stop · <strong>Ctrl+Shift+F5</strong> restart
        </span>
      </div>
      {!hud.ready && !error && <div className="gx-over muted">Starting…</div>}
      {error && (
        <div className="gx-over">
          <p className="bad"><ErrorText text={error} /></p>
          <button onClick={onStop}>Back to editing</button>
        </div>
      )}
      {hud.over && (
        <div className="gx-over">
          <h2>{hud.message || (hud.over === "won" ? "You win!" : "Game over")}</h2>
          <p>Score {hud.score}</p>
          <div className="row">
            <button className="primary" onClick={onRestart}>
              Play again (Ctrl+Shift+F5)
            </button>
            <button onClick={onStop}>Back to editing (Shift+F5)</button>
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

/** A model's clips and what it's playing now. */
interface Animated {
  mixer: THREE.AnimationMixer;
  clips: THREE.AnimationClip[];
  actor?: string;
  current?: THREE.AnimationAction;
}

/** A sprite showing its frames in turn. */
interface Flipbook {
  material: THREE.MeshBasicMaterial;
  frames: (THREE.Texture | null)[];
  fps: number;
  actor?: string;
}

/** The clip for what the actor is doing: what blocks asked for, else walking or standing, else the first. */
function clipFor(clips: THREE.AnimationClip[], actor: Actor | undefined, flat: boolean): THREE.AnimationClip {
  const named = (pattern: RegExp) => clips.find((c) => pattern.test(c.name));
  if (actor?.anim) {
    const want = actor.anim.toLowerCase();
    const found = clips.find((c) => c.name.toLowerCase() === want) ?? clips.find((c) => c.name.toLowerCase().includes(want));
    if (found) return found;
  }
  const speed = actor ? (flat ? Math.abs(actor.vel[0]) : Math.hypot(actor.vel[0], actor.vel[2])) : 0;
  if (speed > 0.3) return named(/run|walk|move|locomotion/i) ?? named(/idle|stand|breath/i) ?? clips[0];
  return named(/idle|stand|breath/i) ?? clips[0];
}

/** Runs the scene in `el` until the returned function is called. */
function play(
  el: HTMLElement,
  scene: Scene,
  things: Record<string, Thing>,
  behaviors: Record<string, Behavior>,
  ppu: number,
  ctl: { current: PlayControls },
  hooks: Hooks,
): () => void {
  const flat = scene.dimensions === "2d";
  const { world: w, statics } = createWorld(scene, things, behaviors, ppu);
  const assets = new Assets(hooks.readAsset);

  const renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setPixelRatio(pixelRatio());
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  el.appendChild(renderer.domElement);
  const root = new THREE.Scene();
  root.background = new THREE.Color(scene.environment.background);
  root.add(new THREE.AmbientLight(new THREE.Color(scene.environment.ambient), 2.5));
  const lit = scene.entities.some((e) => [e, ...(e.children ?? [])].some((x) => x.parts?.some((p) => p.type === "light")));
  root.add(new THREE.HemisphereLight(0xffffff, 0x404858, lit ? 0.8 : 1.5));
  const camera = flat ? new THREE.OrthographicCamera(-8, 8, 6, -6, -100, 100) : new THREE.PerspectiveCamera(60, 1, 0.05, 2000);

  // Animation: models' clips (picked from what each actor is doing) and sprites' frames.
  const animated: Animated[] = [];
  assets.onModel = (model) => {
    if (!model.animations.length) return;
    let o: THREE.Object3D | null = model;
    while (o && !o.userData.actorId) o = o.parent;
    animated.push({ mixer: new THREE.AnimationMixer(model), clips: model.animations, actor: o?.userData.actorId });
  };
  const flipbooks: Flipbook[] = [];
  const findFlipbooks = (obj: THREE.Object3D, actor?: string) =>
    obj.traverse((o) => {
      const f = o.userData.frames as { assets: string[]; fps: number } | undefined;
      if (!f) return;
      const book: Flipbook = { material: (o as THREE.Mesh).material as THREE.MeshBasicMaterial, frames: f.assets.map(() => null), fps: f.fps, actor };
      f.assets.forEach((asset, i) =>
        assets
          .texture(asset)
          .then((tex) => (book.frames[i] = tex))
          .catch(() => {}),
      );
      flipbooks.push(book);
    });

  // The level stays put; each actor is drawn on its own so it can move.
  const index = new Map<string, THREE.Object3D>();
  for (const e of statics) {
    const g = build(e, things, flat, false, index, false, assets);
    findFlipbooks(g);
    root.add(g);
  }
  const groups = new Map<string, THREE.Object3D>();
  const addActor = (a: Actor) => {
    const g = build(a.entity, things, flat, false, index, false, assets);
    g.userData.base = { quaternion: g.quaternion.clone(), scaleX: g.scale.x };
    g.userData.actorId = a.id;
    findFlipbooks(g, a.id);
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
    // The workspace's play keys aren't the game's.
    if (/^f\d+$/.test(e.key.toLowerCase())) return;
    const k = e.key.toLowerCase();
    if (GAME_KEYS.has(k) || ctl.current.free) e.preventDefault();
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
  // The free camera (F8): where it is and which way it looks.
  const free = { on: false, yaw: 0, pitch: 0, last: 0, pan: new THREE.Vector2() };
  const onPointerDown = (e: PointerEvent) => {
    drag = { x: e.clientX, y: e.clientY };
    renderer.domElement.setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: PointerEvent) => {
    if (!drag) return;
    const dx = e.clientX - drag.x;
    const dy = e.clientY - drag.y;
    drag = { x: e.clientX, y: e.clientY };
    if (free.on) {
      if (flat) {
        const scale = (camera as THREE.OrthographicCamera).top * 2 / Math.max(1, el.clientHeight);
        free.pan.x -= dx * scale;
        free.pan.y += dy * scale;
      } else {
        free.yaw -= dx * 0.004;
        free.pitch = Math.max(-1.55, Math.min(1.55, free.pitch - dy * 0.004));
      }
      return;
    }
    if (orbit || flat) return;
    yaw -= dx * 0.006;
    pitch = Math.min(1.3, Math.max(-0.25, pitch + dy * 0.004));
  };
  const onPointerUp = () => (drag = null);
  const onWheel = (e: WheelEvent) => {
    if (orbit && !free.on) return;
    e.preventDefault();
    if (free.on && flat) {
      const cam = camera as THREE.OrthographicCamera;
      cam.zoom = Math.min(20, Math.max(0.05, cam.zoom * (e.deltaY > 0 ? 0.9 : 1.1)));
      cam.updateProjectionMatrix();
    } else if (!free.on && !flat) distance = Math.min(60, Math.max(1.5, distance * (e.deltaY > 0 ? 1.1 : 0.9)));
  };
  renderer.domElement.addEventListener("pointerdown", onPointerDown);
  renderer.domElement.addEventListener("pointermove", onPointerMove);
  renderer.domElement.addEventListener("pointerup", onPointerUp);
  renderer.domElement.addEventListener("wheel", onWheel, { passive: false });
  if (!flat && !w.player) {
    orbit = new OrbitControls(camera, renderer.domElement);
    orbit.enableDamping = true;
    const box = new THREE.Box3().setFromObject(root);
    const center = box.isEmpty() ? new THREE.Vector3() : box.getCenter(new THREE.Vector3());
    const radius = box.isEmpty() ? 5 : Math.max(box.getSize(new THREE.Vector3()).length() / 2, 1);
    camera.position.copy(center).add(new THREE.Vector3(0.55, 0.6, 1).normalize().multiplyScalar(radius * 2));
    orbit.target.copy(center);
  }

  const input = (): Input => {
    // While the camera is free, the game hears nothing.
    if (free.on) return { move: [0, 0, 0], jump: false, jumpPressed: false, pressed: [] };
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

  // Sounds. Paused with the game, and while the window is hidden.
  const playing = new Set<HTMLAudioElement>();
  const urls = new Map<string, Promise<string>>();
  let quiet = false;
  const hush = (on: boolean) => {
    if (on === quiet) return;
    quiet = on;
    for (const a of playing) (on ? a.pause() : a.play().catch(() => {}));
  };
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
        a.playbackRate = ctl.current.speed;
        playing.add(a);
        a.onended = () => playing.delete(a);
        if (!quiet) a.play().catch(() => playing.delete(a));
      })
      .catch(() => {});
  };

  // The animation loop stops while the window is hidden, so the sounds are hushed from here.
  const onVisibility = () => hush(document.hidden || ctl.current.paused);
  document.addEventListener("visibilitychange", onVisibility);

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
  /** Lets go of the player (or takes it back), starting from where the camera is. */
  const setFree = (on: boolean) => {
    if (on === free.on) return;
    free.on = on;
    held.clear();
    if (on) {
      const euler = new THREE.Euler().setFromQuaternion(camera.quaternion, "YXZ");
      free.yaw = euler.y;
      free.pitch = euler.x;
      free.pan.set(0, 0);
      free.last = 0;
    } else if (camera instanceof THREE.OrthographicCamera) {
      camera.zoom = 1;
    }
    if (orbit) orbit.enabled = !on;
  };
  const flyFree = (now: number) => {
    const dt = Math.min(0.1, free.last ? (now - free.last) / 1000 : 0);
    free.last = now;
    const k = (...names: string[]) => (names.some((n) => held.has(n)) ? 1 : 0);
    const fast = held.has("shift") ? 4 : 1;
    if (flat) {
      const speed = (camera as THREE.OrthographicCamera).top * 1.5 * fast * dt;
      free.pan.x += (k("d", "arrowright") - k("a", "arrowleft")) * speed;
      free.pan.y += (k("w", "arrowup") - k("s", "arrowdown")) * speed;
      return;
    }
    camera.quaternion.setFromEuler(new THREE.Euler(free.pitch, free.yaw, 0, "YXZ"));
    const forward = k("w", "arrowup") - k("s", "arrowdown");
    const side = k("d", "arrowright") - k("a", "arrowleft");
    const lift = k("e", "pageup") - k("q", "pagedown");
    if (!forward && !side && !lift) return;
    const dir = camera.getWorldDirection(new THREE.Vector3());
    const right = new THREE.Vector3().crossVectors(dir, up).normalize();
    const move = new THREE.Vector3().addScaledVector(dir, forward).addScaledVector(right, side).addScaledVector(up, lift);
    camera.position.add(move.normalize().multiplyScalar(5 * fast * dt));
  };

  const placeCamera = (now: number) => {
    const aspect = el.clientWidth / Math.max(1, el.clientHeight);
    if (camera instanceof THREE.OrthographicCamera) {
      if (free.on) flyFree(now);
      const v = w.view;
      const half = v.halfHeight ?? (v.size ? Math.max(v.size[1] / 2, v.size[0] / 2 / aspect, 1) * 1.05 : 5);
      camera.top = half;
      camera.bottom = -half;
      camera.left = -half * aspect;
      camera.right = half * aspect;
      camera.position.set(v.center[0] + free.pan.x, v.center[1] + free.pan.y, 50);
      camera.updateProjectionMatrix();
      return;
    }
    if (free.on) return flyFree(now);
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

  const actorById = new Map<string, Actor>();
  const sync = () => {
    actorById.clear();
    for (const a of w.actors) {
      actorById.set(a.id, a);
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

  let clock = 0;
  const animate = (dt: number) => {
    clock += dt;
    for (const m of animated) {
      const clip = clipFor(m.clips, m.actor ? actorById.get(m.actor) : undefined, flat);
      if (m.current?.getClip() !== clip) {
        const next = m.mixer.clipAction(clip);
        next.reset().fadeIn(m.current ? 0.2 : 0).play();
        m.current?.fadeOut(0.2);
        m.current = next;
      }
      m.mixer.update(dt);
    }
    for (const book of flipbooks) {
      const tex = book.frames[Math.floor(clock * book.fps) % book.frames.length];
      if (tex && book.material.map !== tex) {
        book.material.map = tex;
        book.material.needsUpdate = true;
      }
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
          for (let i = animated.length - 1; i >= 0; i--) if (animated[i].actor === e.id) animated.splice(i, 1);
          for (let i = flipbooks.length - 1; i >= 0; i--) if (flipbooks[i].actor === e.id) flipbooks.splice(i, 1);
        }
      } else if (e.type === "scene") hooks.onScene(e.scene);
    }
    w.events.length = 0;
  };

  let then = performance.now();
  let pending = 0;
  let stepsSeen = ctl.current.steps;
  let speed = ctl.current.speed;
  renderer.setAnimationLoop((now) => {
    const c = ctl.current;
    const elapsed = Math.min(0.1, (now - then) / 1000);
    then = now;
    // Minimised or behind another window: nothing runs and nothing is heard.
    hush(document.hidden || c.paused);
    if (document.hidden) return;
    setFree(c.free);
    if (c.speed !== speed) {
      speed = c.speed;
      for (const a of playing) a.playbackRate = speed;
    }
    let ran = 0;
    if (!c.paused) {
      pending += elapsed * c.speed;
      const most = Math.max(5, Math.ceil(5 * c.speed));
      while (pending >= STEP && ran < most) {
        step(w, input(), STEP);
        pressed = [];
        pending -= STEP;
        ran++;
      }
      if (ran === most) pending = 0;
    } else {
      pending = 0;
      // One step for each press of F10.
      while (stepsSeen < c.steps) {
        step(w, input(), STEP);
        pressed = [];
        stepsSeen++;
        ran++;
      }
    }
    stepsSeen = c.steps;
    events();
    sync();
    animate(ran * STEP);
    placeCamera(now);
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
    document.removeEventListener("visibilitychange", onVisibility);
    renderer.domElement.removeEventListener("pointerdown", onPointerDown);
    renderer.domElement.removeEventListener("pointermove", onPointerMove);
    renderer.domElement.removeEventListener("pointerup", onPointerUp);
    renderer.domElement.removeEventListener("wheel", onWheel);
    orbit?.dispose();
    for (const m of animated) m.mixer.stopAllAction();
    for (const a of playing) a.pause();
    urls.forEach((u) => u.then((x) => URL.revokeObjectURL(x)).catch(() => {}));
    dispose(root);
    assets.dispose();
    renderer.dispose();
    el.removeChild(renderer.domElement);
  };
}
