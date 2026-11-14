// Small line icons, drawn here so nothing is fetched and nothing is a brand's
// logo. They take the colour of the text around them.
import type { ReactNode } from "react";
import type { Kind } from "../data/types";

function Icon({ children, filled = false }: { children: ReactNode; filled?: boolean }) {
  return (
    <svg
      className="icon"
      viewBox="0 0 24 24"
      fill={filled ? "currentColor" : "none"}
      stroke="currentColor"
      strokeWidth={filled ? 0 : 2.4}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {children}
    </svg>
  );
}

/** Installed on the Ally: plays without a desktop. */
export const OnDevice = () => (
  <Icon>
    <path d="M12 4v10m0 0l-4-4m4 4l4-4M5 19h14" />
  </Icon>
);

/** Streamed from a desktop. */
export const Streamed = () => (
  <Icon>
    <path d="M4.5 10a11 11 0 0115 0M7.5 13.5a6.5 6.5 0 019 0" />
    <circle cx="12" cy="17.5" r="1.3" fill="currentColor" />
  </Icon>
);

export const Heart = () => (
  <Icon filled>
    <path d="M12 20.5s-7.5-4.6-7.5-10.1A4.2 4.2 0 0112 7.9a4.2 4.2 0 017.5 2.5c0 5.5-7.5 10.1-7.5 10.1z" />
  </Icon>
);

export const Trophy = () => (
  <Icon>
    <path d="M8 4h8v5a4 4 0 01-8 0V4zM8 6H5a3 3 0 003 3.5M16 6h3a3 3 0 01-3 3.5M12 13v4M8.5 20h7M10 17h4" />
  </Icon>
);

export const Lock = () => (
  <Icon>
    <rect x="6" y="11" width="12" height="8.5" rx="2.2" />
    <path d="M8.5 11V8.5a3.5 3.5 0 017 0V11" />
  </Icon>
);

export const Sparkle = () => (
  <Icon filled>
    <path d="M12 2.5l1.9 5.6 5.6 1.9-5.6 1.9L12 17.5l-1.9-5.6-5.6-1.9 5.6-1.9z" />
  </Icon>
);

export const Clock = () => (
  <Icon>
    <circle cx="12" cy="12" r="8" />
    <path d="M12 7.5V12l3 2" />
  </Icon>
);

export const SaveIcon = () => (
  <Icon>
    <path d="M5 5h11l3 3v11H5zM8 5v5h7V5M8 19v-5h8v5" />
  </Icon>
);

/** Played on a cloud gaming service. */
export const CloudIcon = () => (
  <Icon>
    <path d="M7.5 18.5h9.5a4 4 0 00.6-7.95A5.5 5.5 0 006.9 9.6 4.5 4.5 0 007.5 18.5z" />
  </Icon>
);

const SOURCE: Record<Kind, ReactNode> = {
  // A shopping bag: came from a store.
  game: <path d="M6 8h12l-1 11.5H7zM9 8V6.5a3 3 0 016 0V8" />,
  // A cartridge: plays through an emulator.
  emulated: <path d="M7 4h10v16H7zM10 4v4h4V4M10 16h4" />,
  // A wrench: rebuilt to run natively.
  port: <path d="M14.5 5a4 4 0 00-3.8 5.3L5 16v3h3l5.7-5.7A4 4 0 0019 9.5l-2.5 2.5-2-2L17 7.5A4 4 0 0014.5 5z" />,
  // A little house: home-made.
  homebrew: <path d="M4.5 11.5L12 5l7.5 6.5M7 10v9h10v-9M10.5 19v-4.5h3V19" />,
};

/** Where a game came from, by its kind. The name is written next to it. */
export const Source = ({ kind }: { kind: Kind }) => <Icon>{SOURCE[kind]}</Icon>;

/** Fetch a game from its store. */
export const Download = () => (
  <Icon>
    <path d="M12 3v11m0 0l-4.5-4.5M12 14l4.5-4.5M4 17v2a2 2 0 002 2h12a2 2 0 002-2v-2" />
  </Icon>
);
