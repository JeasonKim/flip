import { describe, expect, it } from "vitest";
import {
  isMermaidDiagramSource,
  normalizeMarkdownDiagrams,
} from "./markdownDiagrams";

describe("markdown diagram normalization", () => {
  it("wraps bare flowcharts as mermaid code fences", () => {
    const markdown = [
      "flowchart TD",
      "  A[用户购买商品档位] --> B[支付订单]",
      "  B --> C[发放基础积分与赠送积分]",
    ].join("\n");

    expect(normalizeMarkdownDiagrams(markdown)).toBe(
      [
        "```mermaid",
        "flowchart TD",
        "  A[用户购买商品档位] --> B[支付订单]",
        "  B --> C[发放基础积分与赠送积分]",
        "```",
      ].join("\n"),
    );
  });

  it("keeps existing fenced diagrams unchanged", () => {
    const markdown = [
      "```mermaid",
      "flowchart LR",
      "  A --> B",
      "```",
    ].join("\n");

    expect(normalizeMarkdownDiagrams(markdown)).toBe(markdown);
  });

  it("does not treat mermaid-like text inside code fences as a diagram", () => {
    const markdown = [
      "```text",
      "flowchart TD",
      "  A --> B",
      "```",
    ].join("\n");

    expect(normalizeMarkdownDiagrams(markdown)).toBe(markdown);
  });

  it("detects common mermaid diagram sources", () => {
    expect(isMermaidDiagramSource("sequenceDiagram\nAlice->>Bob: Hi")).toBe(true);
    expect(isMermaidDiagramSource("graph LR\nA --> B")).toBe(true);
    expect(isMermaidDiagramSource("普通段落")).toBe(false);
  });
});
