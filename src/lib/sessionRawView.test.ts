import { describe, expect, it } from "vitest";
import { formatSessionRawValue } from "./sessionRawView";

describe("session raw view", () => {
  it("keeps ordinary raw values exact", () => {
    expect(
      formatSessionRawValue({ type: "message", payload: { answer: 42 } }, false),
    ).toContain('"answer": 42');
  });

  it("summarizes huge strings until the user explicitly reveals them", () => {
    const hugeText = `head-${"x".repeat(210_000)}-tail`;

    const preview = formatSessionRawValue({ output: hugeText }, false);
    const revealed = formatSessionRawValue({ output: hugeText }, true);

    expect(preview).toContain("已隐藏 210010 个字符");
    expect(preview).not.toContain(hugeText);
    expect(revealed).toContain(hugeText);
  });

  it("describes embedded images without placing base64 payloads in the DOM", () => {
    const image = `data:image/png;base64,${"a".repeat(500_000)}`;

    const preview = formatSessionRawValue({ image }, false);

    expect(preview).toContain("image/png");
    expect(preview).toContain("500022 个字符");
    expect(preview).not.toContain(image);
  });
});
