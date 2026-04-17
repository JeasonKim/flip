import { useCallback, useEffect, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import type { AgentId } from "../../types/profile";
import * as api from "../../lib/invoke";

interface CaptureBarProps {
  agent: AgentId;
  onCapture: (agent: AgentId) => Promise<void>;
  onEnrolled?: () => void;
}

export default function CaptureBar({
  agent,
  onCapture,
  onEnrolled,
}: CaptureBarProps) {
  const [unsaved, setUnsaved] = useState(false);
  const [capturing, setCapturing] = useState(false);
  // 手动添加 API Key 表单
  const [showForm, setShowForm] = useState(false);
  const [label, setLabel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [enrolling, setEnrolling] = useState(false);

  // 每次弹窗获焦（用户点击托盘图标）时重新检测
  useEffect(() => {
    const check = () => {
      api.detectUnsaved(agent).then(setUnsaved).catch(() => {});
    };
    check();
    window.addEventListener("focus", check);
    return () => window.removeEventListener("focus", check);
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

  const handleEnroll = useCallback(async () => {
    if (!apiKey.trim()) return;
    setEnrolling(true);
    try {
      const finalLabel = label.trim() || "API";
      await api.enrollApiAccount(
        agent,
        finalLabel,
        apiKey.trim(),
        baseUrl.trim() || undefined,
      );
      // 重置表单
      setShowForm(false);
      setLabel("");
      setApiKey("");
      setBaseUrl("");
      onEnrolled?.();
    } finally {
      setEnrolling(false);
    }
  }, [agent, label, apiKey, baseUrl, onEnrolled]);

  return (
    <div className="space-y-2">
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
              {capturing ? "正在捕获..." : "+ 捕获当前"}
            </motion.button>
          )}
        </AnimatePresence>

        <button
          onClick={() => setShowForm(!showForm)}
          className="text-xs text-gray-400 hover:text-white transition-colors"
        >
          {showForm ? "取消" : "+ 手动添加"}
        </button>
      </div>

      {/* 手动添加 API Key 表单 */}
      <AnimatePresence>
        {showForm && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.15 }}
            className="overflow-hidden"
          >
            <div className="space-y-1.5 bg-white/5 rounded-lg p-2.5">
              <input
                type="text"
                placeholder="名称（可选）"
                value={label}
                onChange={(e) => setLabel(e.target.value)}
                className="w-full bg-white/5 text-xs text-gray-200 rounded px-2 py-1.5 border border-white/10 focus:outline-none focus:border-white/30 placeholder-gray-600"
              />
              <input
                type="password"
                placeholder="API Key *"
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                className="w-full bg-white/5 text-xs text-gray-200 rounded px-2 py-1.5 border border-white/10 focus:outline-none focus:border-white/30 placeholder-gray-600"
              />
              <input
                type="text"
                placeholder="Base URL（可选）"
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                className="w-full bg-white/5 text-xs text-gray-200 rounded px-2 py-1.5 border border-white/10 focus:outline-none focus:border-white/30 placeholder-gray-600"
              />
              <button
                onClick={handleEnroll}
                disabled={!apiKey.trim() || enrolling}
                className="w-full text-xs py-1.5 rounded bg-white/10 hover:bg-white/20 text-gray-200 transition-colors disabled:opacity-30"
              >
                {enrolling ? "添加中..." : "添加"}
              </button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
