import { useEffect, useRef } from "react";
import * as THREE from "three";
import { mergeVertices } from "three/examples/jsm/utils/BufferGeometryUtils.js";
import type { OrbMode } from "../lib/types";

interface Props {
  source: { mode(): OrbMode; level(): number };
  size?: number;
}

const COLORS: Record<OrbMode, [number[], number[]]> = {
  idle: [[0.45, 0.55, 0.7], [0.3, 0.3, 0.55]],
  listen: [[0.5, 0.86, 1.0], [0.45, 0.35, 1.0]],
  speak: [[0.55, 0.92, 1.0], [0.62, 0.4, 1.0]],
  think: [[0.89, 0.73, 0.39], [1.0, 0.52, 0.3]],
};

const VERT = `
  uniform float uTime; uniform float uLevel; uniform float uPx;
  varying float vD; varying vec3 vN; varying vec3 vView;
  float wave(vec3 p) {
    return sin(p.x * 3.1 + uTime * 1.7) * sin(p.y * 3.7 + uTime * 1.3) * sin(p.z * 2.9 + uTime * 2.1)
         + 0.5 * sin(p.x * 7.0 - uTime * 3.0 + p.y * 5.0) * sin(p.z * 6.0 + uTime * 2.4);
  }
  void main() {
    vec3 p = normalize(position);
    float d = wave(p * (1.0 + uLevel)) * (0.05 + uLevel * 0.34);
    vD = d;
    vec4 mv = modelViewMatrix * vec4(p * (1.0 + d), 1.0);
    vN = normalize(normalMatrix * p);
    vView = normalize(-mv.xyz);
    gl_Position = projectionMatrix * mv;
    gl_PointSize = (1.6 + d * 8.0) * (3.6 / -mv.z) * uPx;
  }`;

// One point per vertex (the geometry is de-duplicated), so uDup stands in for the copies a
// non-indexed sphere used to draw on top of each other: same brightness, a sixth of the work.
const FRAG_POINTS = `
  uniform vec3 uA; uniform vec3 uB; uniform float uLevel; uniform float uDup;
  varying float vD; varying vec3 vN; varying vec3 vView;
  void main() {
    float r = length(gl_PointCoord - 0.5);
    if (r > 0.5) discard;
    float rim = pow(1.0 - abs(dot(vN, vView)), 1.6);
    vec3 col = mix(uA, uB, clamp(vD * 3.0 + 0.5, 0.0, 1.0));
    float a = smoothstep(0.5, 0.0, r) * (0.12 + rim * 0.55 + uLevel * 0.25);
    float a2 = min(1.0, a * uDup);
    gl_FragColor = vec4(min(col * (0.7 + rim * 0.6) * a * uDup / max(a2, 1e-4), vec3(1.0)), a2);
  }`;

const FRAG_SHELL = `
  uniform vec3 uA; uniform vec3 uB; uniform float uLevel;
  varying float vD; varying vec3 vN; varying vec3 vView;
  void main() {
    float rim = pow(1.0 - abs(dot(vN, vView)), 2.4);
    gl_FragColor = vec4(mix(uB, uA, rim), rim * (0.4 + uLevel * 0.5));
  }`;

/** Below this input level a listening orb barely moves, so it's drawn at a lower rate. */
const QUIET = 0.05;
/** Frame gap while listening quietly: about 24 fps. */
const QUIET_GAP_MS = 1000 / 24;
/** How often a resting orb checks whether it should wake. */
const POLL_MS = 200;

/**
 * A sphere's vertices with each shared corner kept once (IcosahedronGeometry repeats it per
 * face). `points` drops the index too: Points draws once per index entry, which would bring
 * the repeats back.
 */
function sphere(detail: number, points = false) {
  const g = new THREE.IcosahedronGeometry(1, detail);
  const copies = g.attributes.position.count;
  g.deleteAttribute("normal");
  g.deleteAttribute("uv");
  const merged = mergeVertices(g, 1e-5);
  g.dispose();
  if (points) merged.setIndex(null);
  return { geometry: merged, dup: copies / merged.attributes.position.count };
}

/**
 * 3D voice sphere: particle skin + glass shell + glowing core, driven by live audio level.
 *
 * Draws only while something is happening: full rate when speaking or thinking, about 24 fps
 * while listening quietly, and not at all once it has settled at rest (it then checks five
 * times a second whether to wake). Stops while the window is hidden; one still frame with
 * reduced motion.
 */
export default function Orb({ source, size = 280 }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const sizeRef = useRef(size);
  sizeRef.current = size;
  const live = useRef<{ resize(n: number): void } | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current!;
    const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
    const px = Math.min(devicePixelRatio, 2);
    // Tiny round sprites and soft rims gain nothing from MSAA at 2x pixel ratio.
    const renderer = new THREE.WebGLRenderer({ canvas, alpha: true, antialias: false, powerPreference: "low-power" });
    renderer.setPixelRatio(px);
    renderer.setSize(sizeRef.current, sizeRef.current, false);
    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(40, 1, 0.1, 100);
    camera.position.z = 4.2;

    const skin = sphere(18, true);
    const glass = sphere(10);
    const uniforms = {
      uTime: { value: 0 },
      uLevel: { value: 0.1 },
      uPx: { value: px },
      uDup: { value: skin.dup },
      uA: { value: new THREE.Color(...(COLORS.idle[0] as [number, number, number])) },
      uB: { value: new THREE.Color(...(COLORS.idle[1] as [number, number, number])) },
    };
    const additive = { uniforms, vertexShader: VERT, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending };
    const pts = new THREE.Points(skin.geometry, new THREE.ShaderMaterial({ ...additive, fragmentShader: FRAG_POINTS }));
    const shell = new THREE.Mesh(glass.geometry, new THREE.ShaderMaterial({ ...additive, fragmentShader: FRAG_SHELL }));
    shell.scale.setScalar(0.96);

    const core = new THREE.Mesh(new THREE.SphereGeometry(0.34, 32, 32), new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.9 }));
    const glowCanvas = document.createElement("canvas");
    glowCanvas.width = glowCanvas.height = 128;
    const g = glowCanvas.getContext("2d")!;
    const grad = g.createRadialGradient(64, 64, 0, 64, 64, 64);
    grad.addColorStop(0, "rgba(255,255,255,1)");
    grad.addColorStop(0.25, "rgba(160,230,255,.6)");
    grad.addColorStop(1, "rgba(120,200,255,0)");
    g.fillStyle = grad;
    g.fillRect(0, 0, 128, 128);
    const glowTexture = new THREE.CanvasTexture(glowCanvas);
    const glow = new THREE.Sprite(
      new THREE.SpriteMaterial({ map: glowTexture, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending }),
    );

    scene.add(shell, pts, glow, core);

    const white = new THREE.Color(1, 1, 1);
    const tmpA = new THREE.Color();
    const tmpB = new THREE.Color();
    let level = 0.1;
    let t = 0;
    let raf = 0;
    let poll = 0;
    let last = 0;

    const targetOf = (mode: OrbMode, input: number) =>
      mode === "speak" ? 0.25 + input * 0.9 : mode === "think" ? 0.28 : mode === "listen" ? 0.1 + input * 0.8 : 0.06;

    /** Move everything `dt` seconds towards where `mode` wants it (all the way with reduced motion). */
    const step = (dt: number, mode: OrbMode, input: number) => {
      const target = targetOf(mode, input);
      // The original per-frame easing (at 60 fps), made independent of the frame rate.
      const kLevel = reduce ? 1 : 1 - Math.pow(1 - 0.18, dt * 60);
      const kColor = reduce ? 1 : 1 - Math.pow(1 - 0.06, dt * 60);
      level += (target - level) * kLevel;
      if (!reduce) t += dt;
      uniforms.uTime.value = t;
      uniforms.uLevel.value = level;
      const [a, b] = COLORS[mode];
      uniforms.uA.value.lerp(tmpA.setRGB(a[0], a[1], a[2]), kColor);
      uniforms.uB.value.lerp(tmpB.setRGB(b[0], b[1], b[2]), kColor);
      core.material.color.copy(uniforms.uA.value).lerp(white, 0.5);
      glow.material.color.copy(uniforms.uA.value);
      core.scale.setScalar(0.85 + level * 0.6);
      glow.scale.setScalar(1.3 + level * 1.4);
      pts.rotation.y = shell.rotation.y = t * 0.25;
      pts.rotation.x = shell.rotation.x = Math.sin(t * 0.3) * 0.3;
      const near = (c: THREE.Color, v: number[]) => Math.abs(c.r - v[0]) + Math.abs(c.g - v[1]) + Math.abs(c.b - v[2]) < 0.004;
      return Math.abs(target - level) < 0.002 && near(uniforms.uA.value, a) && near(uniforms.uB.value, b);
    };

    const draw = () => renderer.render(scene, camera);

    /** Resting: no frames, just a cheap check for a change of mode. */
    const rest = (mode: OrbMode) => {
      window.clearInterval(poll);
      poll = window.setInterval(() => {
        if (source.mode() !== mode) wake();
      }, POLL_MS);
    };

    const tick = (now: number) => {
      raf = 0;
      const mode = source.mode();
      const input = source.level();
      const quiet = mode === "idle" || (mode === "listen" && input < QUIET);
      if (last && quiet && now - last < QUIET_GAP_MS - 2) {
        raf = requestAnimationFrame(tick);
        return;
      }
      const dt = last ? Math.min((now - last) / 1000, 0.1) : 1 / 60;
      last = now;
      const settled = step(dt, mode, input);
      draw();
      if (document.hidden) return; // `visibilitychange` wakes it
      if (reduce || (mode === "idle" && settled)) return rest(mode);
      raf = requestAnimationFrame(tick);
    };

    function wake() {
      window.clearInterval(poll);
      poll = 0;
      if (!raf && !document.hidden) {
        last = 0;
        raf = requestAnimationFrame(tick);
      }
    }

    const onVisibility = () => {
      if (document.hidden) {
        cancelAnimationFrame(raf);
        raf = 0;
        window.clearInterval(poll);
        poll = 0;
      } else wake();
    };
    document.addEventListener("visibilitychange", onVisibility);

    live.current = {
      resize(n: number) {
        renderer.setSize(n, n, false);
        draw(); // resizing clears the canvas: redraw now so a resting orb doesn't vanish
        wake();
      },
    };

    // First frame straight away (also the only one with reduced motion), then the loop.
    step(1 / 60, source.mode(), source.level());
    draw();
    wake();

    return () => {
      live.current = null;
      document.removeEventListener("visibilitychange", onVisibility);
      cancelAnimationFrame(raf);
      window.clearInterval(poll);
      renderer.dispose();
      glowTexture.dispose();
      scene.traverse((o) => {
        const m = o as THREE.Mesh;
        m.geometry?.dispose();
        const mat = m.material as THREE.Material | undefined;
        mat?.dispose();
      });
    };
  }, [source]);

  // A new size resizes the one renderer; nothing is rebuilt.
  useEffect(() => {
    live.current?.resize(size);
  }, [size]);

  return <canvas ref={canvasRef} className="orb" style={{ width: size, height: size }} aria-hidden="true" />;
}
