import { motion } from "framer-motion";
import { useProfiles } from "../hooks/useProfiles";
import AgentSection from "../components/popup/AgentSection";
import Footer from "../components/popup/Footer";
import type { AgentId } from "../types/profile";

const AGENTS: AgentId[] = ["claude", "codex"];

function PopupPage() {
  const { config, loading, error, reload, flip, capture, dismiss } =
    useProfiles();

  if (loading || !config) {
    return (
      <div className="min-h-screen bg-gray-900/80 backdrop-blur-xl flex items-center justify-center">
        <span className="text-gray-500 text-sm animate-pulse">Loading...</span>
      </div>
    );
  }

  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
      transition={{ duration: 0.15 }}
      className="min-h-screen bg-gray-900/80 backdrop-blur-xl p-4 text-white flex flex-col gap-4"
    >
      {error && (
        <p className="text-xs text-red-400 bg-red-500/10 rounded px-2 py-1">
          {error}
        </p>
      )}

      {AGENTS.map((agent) => (
        <AgentSection
          key={agent}
          agent={agent}
          agentConfig={config[agent]}
          onFlip={flip}
          onCapture={async (a: AgentId) => {
            await capture(a);
          }}
          onDismiss={dismiss}
          onAccountChanged={reload}
        />
      ))}

      <div className="mt-auto">
        <Footer onImported={reload} />
      </div>
    </motion.div>
  );
}

export default PopupPage;
