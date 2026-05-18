import { useEffect, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import type {
  AgentConfig,
  AgentId,
  Account,
  ModelInfo,
} from "../../types/profile";
import { useQuota } from "../../hooks/useQuota";
import { readModelInfo } from "../../lib/invoke";
import AccountItem from "./AccountItem";
import QuotaBar from "./QuotaBar";
import CaptureBar from "./CaptureBar";

// Agent 显示名和主题色
const agentMeta: Record<AgentId, { name: string; accent: string }> = {
  claude: { name: "Claude Code", accent: "text-violet-400" },
  codex: { name: "Codex", accent: "text-emerald-400" },
};

interface AgentSectionProps {
  agent: AgentId;
  agentConfig: AgentConfig;
  onFlip: (agent: AgentId, accountId: string) => Promise<void>;
  onCapture: (agent: AgentId) => Promise<void>;
  onDismiss: (agent: AgentId, accountId: string) => Promise<void>;
  onAccountChanged?: () => Promise<void> | void;
}

export default function AgentSection({
  agent,
  agentConfig,
  onFlip,
  onCapture,
  onDismiss,
  onAccountChanged,
}: AgentSectionProps) {
  const [expanded, setExpanded] = useState(true);
  const [modelInfo, setModelInfo] = useState<ModelInfo | null>(null);
  const meta = agentMeta[agent];

  useEffect(() => {
    readModelInfo(agent)
      .then(setModelInfo)
      .catch((error) => {
        console.warn(`[model-info] read failed agent=${agent}`, error);
      });
    const timer = setInterval(() => {
      readModelInfo(agent)
        .then(setModelInfo)
        .catch((error) => {
          console.warn(`[model-info] refresh failed agent=${agent}`, error);
        });
    }, 30_000);
    return () => clearInterval(timer);
  }, [agent, agentConfig.current]);

  // 判断当前激活账号是否为 Plan 类型（决定是否显示限额）
  const activeAccount = agentConfig.accounts.find(
    (a) => a.id === agentConfig.current,
  );
  const showQuota = activeAccount?.type === "plan";
  const {
    tiers,
    loading: quotaLoading,
    error: quotaError,
    refresh: refreshQuota,
  } = useQuota(agent, showQuota);

  return (
    <div className="space-y-2">
      {/* 头部：Agent 名 + 展开/收起 */}
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center gap-2 group"
      >
        <span className={`text-sm font-semibold ${meta.accent}`}>
          {meta.name}
        </span>
        {modelInfo && (modelInfo.model || modelInfo.thinking) && (
          <span className="text-[10px] text-gray-500 truncate">
            {[modelInfo.model, modelInfo.thinking].filter(Boolean).join(" · ")}
          </span>
        )}
        <span className="ml-auto text-gray-500 text-xs group-hover:text-gray-300 transition-colors">
          {expanded ? "−" : "+"}
        </span>
      </button>

      <AnimatePresence initial={false}>
        {expanded && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.2 }}
            className="overflow-hidden space-y-2"
          >
            {/* 账号列表 */}
            {agentConfig.accounts.length === 0 ? (
              <p className="text-xs text-gray-500 italic pl-1">
                No accounts yet
              </p>
            ) : (
              <div className="space-y-1">
                {agentConfig.accounts.map((account: Account) => {
                  const isActive = account.id === agentConfig.current;
                  const showQuotaHere = isActive && showQuota;
                  return (
                    <div key={account.id}>
                      <AccountItem
                        account={account}
                        active={isActive}
                        onFlip={() => onFlip(agent, account.id)}
                        onDismiss={() => onDismiss(agent, account.id)}
                      />
                      {showQuotaHere && (
                        <div className="space-y-1 pl-5 pt-1">
                          {quotaLoading && tiers.length === 0 && (
                            <p className="text-[10px] text-gray-500">
                              Loading quota...
                            </p>
                          )}
                          {quotaError && tiers.length === 0 && (
                            <p className="text-[10px] text-red-400">
                              {quotaError}
                            </p>
                          )}
                          {tiers.map((tier) => (
                            <QuotaBar
                              key={tier.name}
                              tier={tier}
                              onExpired={refreshQuota}
                            />
                          ))}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            )}

            {/* Capture / 手动添加 */}
            <CaptureBar
              agent={agent}
              onCapture={onCapture}
              onEnrolled={onAccountChanged}
            />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
