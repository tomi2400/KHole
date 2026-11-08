import { useCallback } from "react";
import { DropOverlay } from "@/components/DropOverlay";
import { SetupScreen } from "@/screens/SetupScreen";
import { StartScreen } from "@/screens/StartScreen";
import { TriageScreen } from "@/screens/TriageScreen";
import { openFolder, useApp } from "@/state/app";
import { useSession } from "@/state/session";

export default function App() {
  const screen = useApp((s) => s.screen);
  const sessionId = useSession((s) => s.session?.id);
  const onDrop = useCallback((path: string) => void openFolder(path), []);

  return (
    <>
      {screen === "start" || sessionId == null ? (
        <StartScreen />
      ) : screen === "setup" ? (
        <SetupScreen key={sessionId} />
      ) : (
        <TriageScreen key={sessionId} />
      )}
      <DropOverlay onDrop={onDrop} />
    </>
  );
}
