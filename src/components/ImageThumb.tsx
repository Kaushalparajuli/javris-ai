import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

// Data URLs are cached so thumbnails don't re-read files on every render.
const cache = new Map<string, Promise<string>>();

export function loadImage(path: string): Promise<string> {
  if (!cache.has(path)) {
    const p = invoke<string>("read_image", { path });
    p.catch(() => cache.delete(path));
    cache.set(path, p);
  }
  return cache.get(path)!;
}

export default function ImageThumb({ path, className, alt, onClick }: { path: string; className?: string; alt: string; onClick?: () => void }) {
  const [src, setSrc] = useState("");
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    loadImage(path).then((s) => live && setSrc(s)).catch(() => live && setFailed(true));
    return () => {
      live = false;
    };
  }, [path]);
  if (failed) return <div className={`${className ?? ""} img-missing`}>Image missing</div>;
  if (!src) return <div className={`${className ?? ""} img-loading`} />;
  return <img className={className} src={src} alt={alt} onClick={onClick} draggable={false} />;
}
