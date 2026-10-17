// Codex · 2026-10-21 · Re-target from the live presentation on every reversal.
import { useLayoutEffect, useRef, useState } from "react";

export function captureOrigin(element: HTMLElement | null) {
  if (!element) return;
  const r = element.getBoundingClientRect();
  document.documentElement.style.setProperty(
    "--sheet-origin-x",
    `${r.x + r.width / 2}px`,
  );
  document.documentElement.style.setProperty(
    "--sheet-origin-y",
    `${r.y + r.height / 2}px`,
  );
}

export function useSpatialPresence(open: boolean) {
  const [retained, setRetained] = useState(open);
  const [node, panel] = useState<HTMLDivElement | null>(null);
  const [shadeNode, overlay] = useState<HTMLDivElement | null>(null);
  const animations = useRef<Animation[]>([]);
  const present = open || retained;
  useLayoutEffect(() => {
    if (open) setRetained(true);
    if (!node || !present) return;
    const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
    const running = animations.current.some((a) => a.playState === "running");
    // Read before cancelling: an interrupted close resumes from its visible value.
    const current = getComputedStyle(node);
    const start = {
      opacity: running ? current.opacity : open ? "0" : "1",
      scale: reduced ? "1" : running ? current.scale : open ? "0.96" : "1",
    };
    const shadeOpacity = shadeNode ? getComputedStyle(shadeNode).opacity : "0";
    animations.current.forEach((a) => a.cancel());
    const motion = node.animate(
      [
        start,
        {
          opacity: open ? "1" : "0",
          scale: reduced ? "1" : open ? "1" : "0.96",
        },
      ],
      {
        duration: reduced ? 150 : 350,
        easing: "cubic-bezier(0.16, 1, 0.3, 1)",
        fill: "both",
      },
    );
    const shade = shadeNode?.animate(
      [
        { opacity: running ? shadeOpacity : open ? "0" : "1" },
        { opacity: open ? "1" : "0" },
      ],
      { duration: reduced ? 150 : 350, fill: "both" },
    );
    animations.current = [motion, ...(shade ? [shade] : [])];
    motion.onfinish = () => {
      if (!open) setRetained(false);
    };
    return () => {
      motion.onfinish = null;
    };
  }, [open, present, node, shadeNode]);
  useLayoutEffect(
    () => () => animations.current.forEach((a) => a.cancel()),
    [],
  );
  return { present, panel, overlay };
}
