import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { cloud } from "./data/sample";
import { library, message, useLibrary } from "./library/store";
import { appInfo, quit } from "./native";
import { currentFocusId, focusById, focusFirst, installScrollIntoView } from "./nav/focus";
import { installInput, type Action } from "./nav/input";
import { cleanup } from "./cleanup/store";
import { Cleanup, type CleanupView } from "./screens/cleanup";
import { Manage } from "./screens/manage";
import { Playing } from "./screens/playing";
import { Cloud, Desktops, Emulation, GameDetail, Games, Home, HostView, Ports, Settings, type Ctx } from "./screens/screens";
import { Hints, SECTIONS, TopBar, type Section } from "./ui/chrome";

type Page = { kind: "game"; id: string } | { kind: "host"; id: string } | { kind: "cleanup"; id: CleanupView }
  | { kind: "manage"; id: "home" };
type Theme = "light" | "dark";

const THEME_KEY = "carla.theme";

function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

export function App() {
  const [section, setSection] = useState<Section>("home");
  const [stack, setStack] = useState<Page[]>([]);
  const [theme, setTheme] = useState<Theme>(() => load<Theme>(THEME_KEY, "light"));
  const { games, hosts, session, scanning } = useLibrary();
  const favourites = useMemo(() => new Set(games.filter((g) => g.favourite).map((g) => g.id)), [games]);
  const [toast, setToast] = useState<string | null>(null);
  const [version, setVersion] = useState("");
  // Where focus was before each pushed page, so Back lands on the same card.
  const returnFocus = useRef<(string | null)[]>([]);
  const pendingFocus = useRef<string | null>(null);
  const main = useRef<HTMLElement>(null);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem(THEME_KEY, JSON.stringify(theme));
  }, [theme]);

  useEffect(() => {
    void library.start();
  }, []);

  useEffect(() => {
    appInfo().then((i) => setVersion(i.version)).catch(() => undefined);
    return installScrollIntoView();
  }, []);

  useEffect(() => {
    if (!toast) return;
    const id = setTimeout(() => setToast(null), 2600);
    return () => clearTimeout(id);
  }, [toast]);

  const push = useCallback((page: Page) => {
    returnFocus.current.push(currentFocusId());
    setStack((s) => [...s, page]);
  }, []);

  const back = useCallback(() => {
    setStack((s) => {
      if (s.length === 0) {
        setSection("home");
        return s;
      }
      pendingFocus.current = returnFocus.current.pop() ?? null;
      return s.slice(0, -1);
    });
  }, []);

  const go = useCallback((s: Section) => {
    returnFocus.current = [];
    setStack([]);
    setSection(s);
  }, []);

  const playing = session !== null;
  const toggleFavourite = useCallback(
    (id: string) => void library.setFavourite(id, !favourites.has(id)).catch((e) => setToast(message(e))),
    [favourites],
  );

  useEffect(() => {
    const onAction = (action: Action) => {
      if (playing) return; // only the one button on the playing screen works
      if (action === "back") back();
      else if (action === "prev" || action === "next") {
        const i = SECTIONS.findIndex((s) => s.id === section);
        const n = (i + (action === "next" ? 1 : -1) + SECTIONS.length) % SECTIONS.length;
        go(SECTIONS[n].id);
      } else if (action === "favourite") {
        const id = (document.activeElement as HTMLElement | null)?.closest<HTMLElement>("[data-game]")?.dataset.game;
        if (id) toggleFavourite(id);
      }
    };
    return installInput(onAction);
  }, [back, go, playing, section, toggleFavourite]);

  // The administrator pages lock themselves again as soon as they are left.
  const inCleanup = stack.some((p) => p.kind === "cleanup");
  useEffect(() => {
    if (!inCleanup) cleanup.leave();
  }, [inCleanup]);

  // After every page change put focus somewhere sensible.
  const top = stack[stack.length - 1];
  const pageKey = playing ? "playing" : top ? `${top.kind}:${top.id}` : section;
  useLayoutEffect(() => {
    const wanted = pendingFocus.current;
    pendingFocus.current = null;
    if (wanted && focusById(wanted)) return;
    main.current?.scrollTo({ top: 0 });
    if (main.current) focusFirst(main.current);
  }, [pageKey]);

  const ctx: Ctx = {
    games,
    hosts,
    favourites,
    openGame: (id) => push({ kind: "game", id }),
    openHost: (id) => push({ kind: "host", id }),
    notify: setToast,
    play: (gameId, place) => void library.launch(gameId, place).catch((e) => setToast(message(e))),
  };

  let page;
  if (top?.kind === "game") page = <GameDetail {...ctx} gameId={top.id} onToggleFavourite={toggleFavourite} />;
  else if (top?.kind === "host") page = <HostView {...ctx} hostId={top.id} />;
  else if (top?.kind === "manage") page = <Manage games={games} notify={setToast} />;
  else if (top?.kind === "cleanup") page = <Cleanup view={top.id} open={(view) => push({ kind: "cleanup", id: view })} />;
  else if (section === "home") page = <Home {...ctx} />;
  else if (section === "games") page = <Games {...ctx} />;
  else if (section === "desktops") page = <Desktops {...ctx} />;
  else if (section === "emulation") page = <Emulation {...ctx} />;
  else if (section === "ports") page = <Ports {...ctx} />;
  else if (section === "cloud") page = <Cloud services={cloud} notify={setToast} />;
  else
    page = (
      <Settings
        theme={theme}
        onTheme={() => setTheme(theme === "light" ? "dark" : "light")}
        onCleanup={() => push({ kind: "cleanup", id: "home" })}
        onManage={() => push({ kind: "manage", id: "home" })}
        onRefresh={() => void library.scan().catch((e) => setToast(message(e)))}
        refreshing={scanning}
        gameCount={games.length}
        onExit={() => void quit()}
        version={version}
      />
    );

  if (session) {
    return (
      <div className="shell shell--playing">
        <main className="main" ref={main} key={pageKey}>
          <Playing session={session} onEnd={() => void library.endSession()} />
        </main>
        {toast && (
          <div className="toast" role="status">
            {toast}
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="shell">
      <TopBar section={section} onSelect={go} />
      <main className="main" ref={main} key={pageKey}>
        {page}
      </main>
      <Hints
        items={[
          ["A", "Select"],
          ["B", "Back"],
          ["X", "Favourite"],
          ["LB RB", "Sections"],
        ]}
      />
      {toast && (
        <div className="toast" role="status">
          {toast}
        </div>
      )}
    </div>
  );
}
