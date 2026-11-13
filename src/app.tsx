import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { PhoneCamera } from "./phone.js";
import { RoomViewer } from "./room-viewer.js";
import "./phone.css";

type Page = "publish" | "room";

function App() {
  const [page, setPage] = useState<Page>(() =>
    new URLSearchParams(location.hash.slice(1)).get("view") === "room"
      ? "room"
      : "publish",
  );
  useEffect(() => {
    const theme = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () =>
      document.documentElement.setAttribute(
        "data-theme",
        theme.matches ? "dark" : "light",
      );
    update();
    theme.addEventListener("change", update);
    return () => theme.removeEventListener("change", update);
  }, []);
  return (
    <>
      <nav className="app-nav" aria-label="Camera tools">
        {(
          [
            ["publish", "Stream"],
            ["room", "Room viewer"],
          ] as const
        ).map(([value, label]) => (
          <button
            key={value}
            type="button"
            aria-current={page === value ? "page" : undefined}
            onClick={() => setPage(value)}
          >
            {label}
          </button>
        ))}
      </nav>
      <div hidden={page !== "publish"}>
        <PhoneCamera />
      </div>
      <div hidden={page !== "room"}>
        <RoomViewer />
      </div>
    </>
  );
}

const root = document.getElementById("root");
if (!root) throw new Error("Camera tools root is missing");
createRoot(root).render(<App />);
