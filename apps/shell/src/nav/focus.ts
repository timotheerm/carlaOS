// Spatial focus: every focusable thing carries `data-focus`, and a direction
// moves real DOM focus to the nearest one that way. Controller, keyboard and
// touch therefore all end up driving the same focused element.

export type Dir = "up" | "down" | "left" | "right";

const SELECTOR = "[data-focus]";

function candidates(): HTMLElement[] {
  return [...document.querySelectorAll<HTMLElement>(SELECTOR)].filter(
    (el) => el.offsetParent !== null && !el.hasAttribute("disabled"),
  );
}

export function focusFirst(root: ParentNode = document): void {
  const el =
    root.querySelector<HTMLElement>(`${SELECTOR}[data-autofocus]`) ??
    root.querySelector<HTMLElement>(SELECTOR);
  el?.focus();
}

export function focusById(id: string): boolean {
  const el = document.querySelector<HTMLElement>(`${SELECTOR}[data-fid="${CSS.escape(id)}"]`);
  el?.focus();
  return !!el;
}

export function currentFocusId(): string | null {
  return (document.activeElement as HTMLElement | null)?.dataset?.fid ?? null;
}

// Gap between two 1-D ranges, 0 when they overlap.
function gap(a1: number, a2: number, b1: number, b2: number): number {
  return Math.max(0, Math.max(a1, b1) - Math.min(a2, b2));
}

export function move(dir: Dir): void {
  const cur = document.activeElement as HTMLElement | null;
  if (!cur || !cur.matches(SELECTOR)) {
    focusFirst();
    return;
  }
  const c = cur.getBoundingClientRect();
  const horizontal = dir === "left" || dir === "right";
  const scored: { el: HTMLElement; primary: number; ortho: number }[] = [];

  for (const el of candidates()) {
    if (el === cur) continue;
    const r = el.getBoundingClientRect();
    let primary: number;
    if (dir === "right") primary = r.left - c.right;
    else if (dir === "left") primary = c.left - r.right;
    else if (dir === "down") primary = r.top - c.bottom;
    else primary = c.top - r.bottom;
    // Must lie fully on that side (a little overlap is forgiven).
    if (primary < -8) continue;
    const ortho = horizontal
      ? gap(c.top, c.bottom, r.top, r.bottom)
      : // Left edges break ties, so leaving a full-width item lands on the
        // first card of the next row instead of one in the middle.
        gap(c.left, c.right, r.left, r.right) + Math.abs(r.left - c.left) / 1000;
    // Sideways moves stay inside the current row.
    if (horizontal && ortho > 0) continue;
    scored.push({ el, primary: Math.max(0, primary), ortho });
  }
  if (scored.length === 0) return;

  // Take the nearest band in the travel direction, then the best aligned in it.
  const nearest = Math.min(...scored.map((s) => s.primary));
  const band = scored.filter((s) => s.primary <= nearest + 48);
  band.sort((a, b) => a.ortho - b.ortho || a.primary - b.primary);
  band[0].el.focus();
}

// Keep the focused element comfortably on screen.
export function installScrollIntoView(): () => void {
  const onFocus = (e: FocusEvent) => {
    const el = e.target as HTMLElement | null;
    if (el?.matches?.(SELECTOR)) {
      el.scrollIntoView({ block: "center", inline: "center", behavior: "smooth" });
    }
  };
  document.addEventListener("focusin", onFocus);
  return () => document.removeEventListener("focusin", onFocus);
}
