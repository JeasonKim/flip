import { getCurrentWindow } from "@tauri-apps/api/window";
import PopupPage from "./pages/PopupPage";
import SessionPage from "./pages/SessionPage";

function App() {
  const windowLabel = getCurrentWindow().label;

  if (windowLabel === "sessions") {
    return <SessionPage />;
  }

  return <PopupPage />;
}

export default App;
