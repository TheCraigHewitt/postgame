import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

export function App() {
  const [version, setVersion] = useState("…");

  useEffect(() => {
    invoke<string>("audio_shim_version").then(setVersion, () => setVersion("unavailable"));
  }, []);

  return (
    <main>
      <h1>postgame</h1>
      <p>Audio shim {version}</p>
    </main>
  );
}
