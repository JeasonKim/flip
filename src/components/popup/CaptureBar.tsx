import { useCallback, useEffect, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import type { AgentId } from "../../types/profile";
import * as api from "../../lib/invoke";

interface CaptureBarProps {
  agent: AgentId;
  onCapture: (agent: AgentId) => Promise<void>;
}

export default function CaptureBar({ agent, onCapture }: CaptureBarProps) {
  const [unsaved, setUnsaved] = useState(false);
  const [capturing, setCapturing] = useState(false);

  // 打开弹窗时检测一次
  useEffect(() => {
    api.detectUnsaved(agent).then(setUnsaved).catch(() => {});
  }, [agent]);

  const handleCapture = useCallback(async () => {
    setCapturing(true);
    try {
      await onCapture(agent);
      setUnsaved(false);
    } finally {
      setCapturing(false);
    }
  }, [agent, onCapture]);

  return (
    <div className="flex items-center gap-2">
      <AnimatePresence mode="wait">
        {unsaved ? (
          <motion.button
            key="unsaved"
            initial={{ opacity: 0, x: -4 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -4 }}
            onClick={handleCapture}
            disabled={capturing}
            className="text-xs text-amber-400 hover:text-amber-300 transition-colors disabled:opacity-40 flex items-center gap-1.5"
          >
            <span className="w-1.5 h-1.5 rounded-full bg-amber-400 animate-pulse" />
            {capturing ? "正在捕获..." : "检测到新配置，点击保存"}
          </motion.button>
        ) : (
          <motion.button
            key="normal"
            initial={{ opacity: 0, x: -4 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: -4 }}
            onClick={handleCapture}
            disabled={capturing}
            className="text-xs text-gray-400 hover:text-white transition-colors disabled:opacity-40"
          >
            {capturing ? "正在捕获..." : "+ 捕获当前配置"}
          </motion.button>
        )}
      </AnimatePresence>
    </div>
  );
}
