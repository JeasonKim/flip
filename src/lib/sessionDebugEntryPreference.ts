const DEBUG_ENTRY_VISIBILITY_KEY = "flip.sessions.debugEntriesVisible";

/// 调试条目开关是跨会话的个人阅读偏好，首次使用默认收起
export function recallDebugEntryVisibility(storage: Storage | null): boolean {
  if (!storage) return false;

  let storedValue: string | null;
  try {
    storedValue = storage.getItem(DEBUG_ENTRY_VISIBILITY_KEY);
  } catch (error) {
    console.warn(
      `[sessions] debug entry visibility read failed key=${DEBUG_ENTRY_VISIBILITY_KEY}; fell back to hidden`,
      error,
    );
    return false;
  }

  if (storedValue === null) return false;
  if (storedValue === "true") return true;
  if (storedValue === "false") return false;

  console.warn(
    `[sessions] debug entry visibility discarded key=${DEBUG_ENTRY_VISIBILITY_KEY} stored=${JSON.stringify(storedValue)}; expected "true" or "false", fell back to hidden`,
  );
  return false;
}

export function rememberDebugEntryVisibility(
  storage: Storage | null,
  visible: boolean,
): void {
  if (!storage) return;

  try {
    storage.setItem(DEBUG_ENTRY_VISIBILITY_KEY, String(visible));
  } catch (error) {
    console.warn(
      `[sessions] debug entry visibility persist failed key=${DEBUG_ENTRY_VISIBILITY_KEY} visible=${visible}; preference stays session-only`,
      error,
    );
  }
}
