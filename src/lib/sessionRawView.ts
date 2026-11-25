const LARGE_RAW_STRING_CHAR_LIMIT = 200_000;
const DATA_IMAGE_PREFIX = "data:image/";

export function formatSessionRawValue(
  value: unknown,
  revealLargeStrings: boolean,
): string {
  try {
    return (
      JSON.stringify(
        value,
        (_key, fieldValue: unknown) =>
          representRawField(fieldValue, revealLargeStrings),
        2,
      ) ?? String(value)
    );
  } catch (error) {
    console.warn("[sessions] failed to format entry raw value", error);
    return String(value);
  }
}

export function sessionRawValueHasLargeStrings(value: unknown): boolean {
  if (typeof value === "string") {
    return (
      value.startsWith(DATA_IMAGE_PREFIX) ||
      value.length > LARGE_RAW_STRING_CHAR_LIMIT
    );
  }
  if (Array.isArray(value)) {
    return value.some(sessionRawValueHasLargeStrings);
  }
  if (value && typeof value === "object") {
    return Object.values(value).some(sessionRawValueHasLargeStrings);
  }
  return false;
}

function representRawField(
  fieldValue: unknown,
  revealLargeStrings: boolean,
): unknown {
  if (revealLargeStrings || typeof fieldValue !== "string") {
    return fieldValue;
  }

  if (fieldValue.startsWith(DATA_IMAGE_PREFIX)) {
    const mimeType = fieldValue
      .slice(DATA_IMAGE_PREFIX.length)
      .split(/[;,]/, 1)[0];
    return `[内嵌图片 image/${mimeType || "unknown"}：已隐藏 ${fieldValue.length} 个字符，可点击“显示完整大字段”查看]`;
  }

  if (fieldValue.length > LARGE_RAW_STRING_CHAR_LIMIT) {
    return `[大字段：已隐藏 ${fieldValue.length} 个字符，可点击“显示完整大字段”查看]`;
  }

  return fieldValue;
}
