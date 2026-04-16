import { useCallback, useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import type { Account } from "../../types/profile";

interface AccountItemProps {
  account: Account;
  active: boolean;
  onFlip: () => void;
  onDismiss: () => void;
}

export default function AccountItem({
  account,
  active,
  onFlip,
  onDismiss,
}: AccountItemProps) {
  const [confirming, setConfirming] = useState(false);

  const handleDismissClick = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      if (confirming) {
        onDismiss();
        setConfirming(false);
      } else {
        setConfirming(true);
        // 3 秒后自动取消确认态
        setTimeout(() => setConfirming(false), 3000);
      }
    },
    [confirming, onDismiss],
  );

  return (
    <motion.button
      layout
      onClick={onFlip}
      className={`
        w-full flex items-center gap-2 px-3 py-2 rounded-lg text-left
        transition-colors duration-200 group
        ${active ? "bg-white/15 ring-1 ring-white/20" : "bg-white/5 hover:bg-white/10"}
      `}
    >
      {/* 激活指示器 */}
      <span
        className={`w-2 h-2 rounded-full shrink-0 transition-colors ${
          active ? "bg-green-400 shadow-[0_0_6px_rgba(74,222,128,0.5)]" : "bg-gray-600"
        }`}
      />

      {/* 类型标签 */}
      <span
        className={`text-[10px] uppercase tracking-wider px-1.5 py-0.5 rounded font-medium shrink-0 ${
          account.type === "plan"
            ? "bg-violet-500/20 text-violet-300"
            : "bg-sky-500/20 text-sky-300"
        }`}
      >
        {account.type}
      </span>

      {/* 账号名 */}
      <span className="flex-1 text-sm truncate text-gray-100">
        {account.label}
      </span>

      {/* 删除按钮：二次确认 */}
      <AnimatePresence mode="wait">
        {confirming ? (
          <motion.span
            key="confirm"
            initial={{ opacity: 0, scale: 0.8 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.8 }}
            onClick={handleDismissClick}
            className="shrink-0 text-[10px] text-red-400 hover:text-red-300 px-1.5 py-0.5 rounded bg-red-500/15 cursor-pointer"
          >
            确认删除
          </motion.span>
        ) : (
          <motion.span
            key="delete"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            onClick={handleDismissClick}
            className="shrink-0 opacity-0 group-hover:opacity-100 text-gray-500 hover:text-red-400 transition-opacity text-xs cursor-pointer px-1"
          >
            &times;
          </motion.span>
        )}
      </AnimatePresence>
    </motion.button>
  );
}
