import { useEffect, useState } from "react";
import { appInfo, quit, type AppInfo } from "./native";

type Theme = "light" | "dark";

function useClock(): string {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 10_000);
    return () => clearInterval(id);
  }, []);
  return now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

// M0 placeholder screen. It proves the shell installs, opens fullscreen, talks
// to the Rust core and closes. The real home screen replaces it in M1.
export function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [theme, setTheme] = useState<Theme>("light");
  const clock = useClock();

  useEffect(() => {
    appInfo().then(setInfo).catch(() => setInfo(null));
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  return (
    <main className="splash">
      <div className="splash__centre">
        <h1 className="splash__mark">
          Carla<span>OS</span>
        </h1>
        <p className="splash__line">Foundations build. The home screen arrives next.</p>
      </div>
      <footer className="splash__bar">
        <span>
          {clock}
          {info ? ` · v${info.version}` : ""}
        </span>
        <div className="splash__actions">
          <button
            className="pill"
            autoFocus
            onClick={() => setTheme(theme === "light" ? "dark" : "light")}
          >
            {theme === "light" ? "Dark theme" : "Light theme"}
          </button>
          <button className="pill" onClick={() => void quit()}>
            Exit to Windows
          </button>
        </div>
      </footer>
    </main>
  );
}
