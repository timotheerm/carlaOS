import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { cloud } from "./data/sample";
import { library, message, useLibrary } from "./library/store";
import { appInfo, boot, quit } from "./native";
import { currentFocusId, focusById, focusFirst, installScrollIntoView } from "./nav/focus";
import { installInput, type Action } from "./nav/input";
import { cleanup } from "./cleanup/store";
import { Cleanup, type CleanupView } from "./screens/cleanup";
import { LinkDesktops } from "./screens/link";
import { Manage } from "./screens/manage";
import { Playing } from "./screens/playing";
import { Search } from "./screens/search";
import { Startup } from "./screens/startup";
import { GameDetail } from "./screens/pregame";
import { Cloud, Desktops, Games, Home, HostView, Settings, type Ctx } from "./screens/screens";
import { Updates } from "./screens/updates";
import { Ambient } from "./ui/ambient";
import { Hints, SECTIONS, TopBar, type Section } from "./ui/chrome";
import { installSounds, play as cue, setSounds, soundsOn } from "./ui/sound";
import { Splash } from "./ui/splash";

type Page = { kind: "game"; id: string } | { kind: "host"; id: string } | { kind: "cleanup"; id: CleanupView }
  | { kind: "manage"; id: "home" } | { kind: "desktops"; id: "home" } | { kind: "search"; id: "home" }
  | { kind: "updates"; id: "home" } | { kind: "startup"; id: "home" };
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

// The desktop a stream comes from, when the game is on only one.
function streamFrom(places: string[], hosts: { id: string; name: string }[]): string | undefined {
  const away = places.filter((p) => p !== "local" && p !== "cloud");
  return away.length === 1 ? hosts.find((h) => h.id === away[0])?.name : undefined;
}

export function App() {
  const [section, setSection] = useState<Section>("home");
  const [stack, setStack] = useState<Page[]>([]);
  const [theme, setTheme] = useState<Theme>(() => load<Theme>(THEME_KEY, "light"));
  const { games, hosts, session, scanning } = useLibrary();
  const favourites = useMemo(() => new Set(games.filter((g) => g.favourite).map((g) => g.id)), [games]);
  const [toast, setToast] = useState<string | null>(null);
  // The startup animation plays once each time CarlaOS starts.
  const [splash, setSplash] = useState(true);
  const endSplash = useCallback(() => {
    setSplash(false);
    // The library is on the screen: this start worked.
    void boot.ready().then(
      (s) => s.stood_down && setToast("CarlaOS failed to start twice, so it left the desktop alone once."),
      () => undefined,
    );
    if (main.current && !main.current.contains(document.activeElement)) focusFirst(main.current);
  }, []);
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
    return installSounds();
  }, []);
  const [sounds, setSoundsOn] = useState(soundsOn);

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

  const [query, setQuery] = useState("");
  const playing = session !== null;
  const searching = stack.some((p) => p.kind === "search");
  const toggleFavourite = useCallback(
    (id: string) => void library.setFavourite(id, !favourites.has(id)).catch((e) => setToast(message(e))),
    [favourites],
  );

  // The input listener is installed once and always calls the newest handler,
  // so changing section does not start it afresh while a button is held.
  const handler = useRef<(action: Action) => void>(() => undefined);
  useEffect(() => installInput((action) => handler.current(action)), []);
  useEffect(() => {
    handler.current = (action: Action) => {
      if (playing || splash) return; // only the one button on the playing screen works
      if (action === "back") {
        cue("back");
        back();
      } else if (action === "prev" || action === "next") {
        cue("section");
        const i = SECTIONS.findIndex((s) => s.id === section);
        const n = (i + (action === "next" ? 1 : -1) + SECTIONS.length) % SECTIONS.length;
        go(SECTIONS[n].id);
      } else if (action === "favourite") {
        const id = (document.activeElement as HTMLElement | null)?.closest<HTMLElement>("[data-game]")?.dataset.game;
        if (id) {
          if (!favourites.has(id)) cue("favourite");
          toggleFavourite(id);
        }
      } else if (action === "search") {
        // Once is enough: Y on the search page itself does nothing.
        if (!searching) {
          setQuery("");
          push({ kind: "search", id: "home" });
        }
      }
    };
  }, [back, go, playing, push, searching, section, splash, toggleFavourite]);

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
    play: (gameId, place) => {
      cue("launch");
      void library.launch(gameId, place).catch((e) => setToast(message(e)));
    },
    wakeHost: (id) => {
      setToast("Waking it. This can take up to a minute.");
      void library.wake(id).catch((e) => setToast(message(e)));
    },
    openCloud: (service, name) => {
      cue("launch");
      void library.openCloud(service, name).catch((e) => setToast(message(e)));
    },
    refreshHosts: () => {
      setToast("Checking…");
      void library.refreshHosts().catch((e) => setToast(message(e)));
    },
  };

  let page;
  if (top?.kind === "game") page = <GameDetail {...ctx} gameId={top.id} onToggleFavourite={toggleFavourite} />;
  else if (top?.kind === "host") page = <HostView {...ctx} hostId={top.id} />;
  else if (top?.kind === "search") page = <Search {...ctx} query={query} setQuery={setQuery} />;
  else if (top?.kind === "manage") page = <Manage games={games} notify={setToast} />;
  else if (top?.kind === "desktops") page = <LinkDesktops hosts={hosts} notify={setToast} />;
  else if (top?.kind === "updates") page = <Updates hosts={hosts} notify={setToast} refreshHosts={() => void library.refreshHosts().catch(() => undefined)} />;
  else if (top?.kind === "startup") page = <Startup notify={setToast} />;
  else if (top?.kind === "cleanup") page = <Cleanup view={top.id} open={(view) => push({ kind: "cleanup", id: view })} />;
  else if (section === "home") page = <Home {...ctx} />;
  else if (section === "games") page = <Games {...ctx} />;
  else if (section === "desktops") page = <Desktops {...ctx} />;
  else if (section === "cloud") page = <Cloud {...ctx} services={cloud} />;
  else
    page = (
      <Settings
        theme={theme}
        onTheme={() => setTheme(theme === "light" ? "dark" : "light")}
        onCleanup={() => push({ kind: "cleanup", id: "home" })}
        onManage={() => push({ kind: "manage", id: "home" })}
        onDesktops={() => push({ kind: "desktops", id: "home" })}
        onUpdates={() => push({ kind: "updates", id: "home" })}
        onStartup={() => push({ kind: "startup", id: "home" })}
        sounds={sounds}
        onSounds={() => {
          setSounds(!sounds);
          setSoundsOn(!sounds);
        }}
        hostCount={hosts.length}
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
        <Ambient games={games} fixed={games.find((g) => g.id === session.gameId) ?? null} />
        <main className="main" ref={main} key={pageKey}>
          <Playing
            session={session}
            game={games.find((g) => g.id === session.gameId)}
            from={streamFrom(games.find((g) => g.id === session.gameId)?.places ?? [], hosts)}
            onEnd={() => void library.endSession()}
          />
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
      <Ambient games={games} />
      <TopBar section={section} onSelect={go} />
      <main className="main" ref={main} key={pageKey}>
        {page}
      </main>
      <Hints
        items={[
          ["A", "Select"],
          ["B", "Back"],
          ["X", "Favourite"],
          ["Y", "Search"],
          ["LB RB", "Sections"],
        ]}
      />
      {splash && <Splash onDone={endSplash} />}
      {toast && (
        <div className="toast" role="status">
          {toast}
        </div>
      )}
    </div>
  );
}
