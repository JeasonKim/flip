const ALLOWED_SESSION_RESOURCE_PROTOCOLS = new Set([
  "http:",
  "https:",
  "file:",
]);

export function preserveSessionResourceHref(href: string): string {
  const trimmed = href.trim();
  if (/^[a-z]:[\\/]/i.test(trimmed)) {
    return trimmed;
  }
  const protocol = trimmed.match(/^([a-z][a-z\d+.-]*:)/i)?.[1]?.toLowerCase();
  if (!protocol || ALLOWED_SESSION_RESOURCE_PROTOCOLS.has(protocol)) {
    return trimmed;
  }
  console.warn(
    `[sessions] blocked unsupported resource protocol href=${JSON.stringify(trimmed)} protocol=${protocol}`,
  );
  return "";
}
