import { describe, expect, it, vi } from "vitest";
import { preserveSessionResourceHref } from "./sessionResource";

describe("session resource href", () => {
  it("keeps web URLs and local paths", () => {
    expect(preserveSessionResourceHref("https://example.com/docs")).toBe(
      "https://example.com/docs",
    );
    expect(preserveSessionResourceHref("file:///tmp/result.html")).toBe(
      "file:///tmp/result.html",
    );
    expect(preserveSessionResourceHref("docs/result.html")).toBe(
      "docs/result.html",
    );
    expect(preserveSessionResourceHref("C:\\Users\\demo\\result.html")).toBe(
      "C:\\Users\\demo\\result.html",
    );
  });

  it("blocks unsupported protocols with an observable warning", () => {
    const warning = vi.spyOn(console, "warn").mockImplementation(() => undefined);

    expect(preserveSessionResourceHref("javascript:alert(1)")).toBe("");

    expect(warning).toHaveBeenCalledOnce();
    warning.mockRestore();
  });
});
