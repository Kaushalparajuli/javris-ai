import { useEffect, useRef } from "react";
import * as THREE from "three";
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

const FRAG_POINTS = `
  uniform vec3 uA; uniform vec3 uB; uniform float uLevel;
  varying float vD; varying vec3 vN; varying vec3 vView;
  void main() {
    float r = length(gl_PointCoord - 0.5);
    if (r > 0.5) discard;
    float rim = pow(1.0 - abs(dot(vN, vView)), 1.6);
    vec3 col = mix(uA, uB, clamp(vD * 3.0 + 0.5, 0.0, 1.0));
    float a = smoothstep(0.5, 0.0, r) * (0.12 + rim * 0.55 + uLevel * 0.25);
    gl_FragColor = vec4(col * (0.7 + rim * 0.6), a);
  }`;

const FRAG_SHELL = `
  uniform vec3 uA; uniform vec3 uB; uniform float uLevel;
  varying float vD; varying vec3 vN; varying vec3 vView;
  void main() {
    float rim = pow(1.0 - abs(dot(vN, vView)), 2.4);
    gl_FragColor = vec4(mix(uB, uA, rim), rim * (0.4 + uLevel * 0.5));
  }`;

/** 3D voice sphere: particle skin + glass shell + glowing core, driven by live audio level. */
export default function Orb({ source, size = 280 }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current!;
    const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
    const px = Math.min(devicePixelRatio, 2);
    const renderer = new THREE.WebGLRenderer({ canvas, alpha: true, antialias: true });
    renderer.setPixelRatio(px);
    renderer.setSize(size, size, false);
    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(40, 1, 0.1, 100);
    camera.position.z = 4.2;

    const uniforms = {
      uTime: { value: 0 },
      uLevel: { value: 0.1 },
      uPx: { value: px },
      uA: { value: new THREE.Color(...(COLORS.idle[0] as [number, number, number])) },
      uB: { value: new THREE.Color(...(COLORS.idle[1] as [number, number, number])) },
    };
    const additive = { uniforms, vertexShader: VERT, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending };
    const pts = new THREE.Points(new THREE.IcosahedronGeometry(1, 22), new THREE.ShaderMaterial({ ...additive, fragmentShader: FRAG_POINTS }));
    const shell = new THREE.Mesh(new THREE.IcosahedronGeometry(1, 20), new THREE.ShaderMaterial({ ...additive, fragmentShader: FRAG_SHELL }));
    shell.scale.setScalar(0.96);

    const core = new THREE.Mesh(new THREE.SphereGeometry(0.34, 48, 48), new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.9 }));
    const glowCanvas = document.createElement("canvas");
    glowCanvas.width = glowCanvas.height = 128;
    const g = glowCanvas.getContext("2d")!;
    const grad = g.createRadialGradient(64, 64, 0, 64, 64, 64);
    grad.addColorStop(0, "rgba(255,255,255,1)");
    grad.addColorStop(0.25, "rgba(160,230,255,.6)");
    grad.addColorStop(1, "rgba(120,200,255,0)");
    g.fillStyle = grad;
    g.fillRect(0, 0, 128, 128);
    const glow = new THREE.Sprite(
      new THREE.SpriteMaterial({ map: new THREE.CanvasTexture(glowCanvas), transparent: true, depthWrite: false, blending: THREE.AdditiveBlending }),
    );

    scene.add(shell, pts, glow, core);

    const white = new THREE.Color(1, 1, 1);
    const tmpA = new THREE.Color();
    const tmpB = new THREE.Color();
    let level = 0.1;
    let t = 0;
    let raf = 0;

    const frame = () => {
      const mode = source.mode();
      const input = source.level();
      const target =
        mode === "speak" ? 0.25 + input * 0.9 : mode === "think" ? 0.28 : mode === "listen" ? 0.1 + input * 0.8 : 0.06;
      level += (target - level) * 0.18;
      if (!reduce) t += 0.016;
      uniforms.uTime.value = t;
      uniforms.uLevel.value = level;
      const [a, b] = COLORS[mode];
      uniforms.uA.value.lerp(tmpA.setRGB(a[0], a[1], a[2]), 0.06);
      uniforms.uB.value.lerp(tmpB.setRGB(b[0], b[1], b[2]), 0.06);
      core.material.color.copy(uniforms.uA.value).lerp(white, 0.5);
      glow.material.color.copy(uniforms.uA.value);
      core.scale.setScalar(0.85 + level * 0.6);
      glow.scale.setScalar(1.3 + level * 1.4);
      pts.rotation.y = shell.rotation.y = t * 0.25;
      pts.rotation.x = shell.rotation.x = Math.sin(t * 0.3) * 0.3;
      renderer.render(scene, camera);
      raf = requestAnimationFrame(frame);
    };
    frame();

    return () => {
      cancelAnimationFrame(raf);
      renderer.dispose();
      scene.traverse((o) => {
        const m = o as THREE.Mesh;
        m.geometry?.dispose();
        const mat = m.material as THREE.Material | undefined;
        mat?.dispose();
      });
    };
  }, [source, size]);

  return <canvas ref={canvasRef} className="orb" style={{ width: size, height: size }} aria-hidden="true" />;
}
