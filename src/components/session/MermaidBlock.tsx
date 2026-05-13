import { useEffect, useMemo, useState } from "react";

type MermaidApi = typeof import("mermaid").default;

let mermaidPromise: Promise<MermaidApi> | null = null;

interface MermaidBlockProps {
  chart: string;
}

export default function MermaidBlock({ chart }: MermaidBlockProps) {
  const renderId = useMemo(
    () => `mermaid-${Math.random().toString(36).slice(2)}`,
    [],
  );
  const [svg, setSvg] = useState<string | null>(null);
  const [renderError, setRenderError] = useState<Error | null>(null);

  useEffect(() => {
    let cancelled = false;

    async function renderDiagram() {
      try {
        setRenderError(null);
        const mermaid = await loadMermaid();
        const result = await mermaid.render(renderId, chart);
        if (!cancelled) setSvg(result.svg);
      } catch (error) {
        const normalizedError =
          error instanceof Error ? error : new Error(String(error));
        console.warn(
          `[markdown] mermaid render failed firstLine="${chart.split(/\r?\n/, 1)[0] ?? ""}". Falling back to code block.`,
          normalizedError,
        );
        if (!cancelled) {
          setSvg(null);
          setRenderError(normalizedError);
        }
      }
    }

    void renderDiagram();

    return () => {
      cancelled = true;
    };
  }, [chart, renderId]);

  if (renderError) {
    return (
      <pre className="overflow-x-auto rounded-md bg-black/30 p-3 text-xs text-gray-300">
        <code>{chart}</code>
      </pre>
    );
  }

  if (!svg) {
    return (
      <div className="my-3 rounded-md border border-white/10 bg-black/20 p-3 text-xs text-gray-500">
        Rendering diagram...
      </div>
    );
  }

  return (
    <div
      className="my-3 overflow-x-auto rounded-md border border-white/10 bg-black/20 p-3 [&_svg]:mx-auto [&_svg]:h-auto [&_svg]:max-w-full"
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}

async function loadMermaid(): Promise<MermaidApi> {
  mermaidPromise ??= import("mermaid").then(({ default: mermaid }) => {
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      theme: "dark",
      themeVariables: {
        background: "transparent",
        mainBkg: "#1f2937",
        primaryColor: "#1f2937",
        primaryTextColor: "#e5e7eb",
        primaryBorderColor: "#6b7280",
        lineColor: "#9ca3af",
        secondaryColor: "#111827",
        tertiaryColor: "#374151",
        fontFamily: "Inter, system-ui, sans-serif",
      },
    });
    return mermaid;
  });

  return mermaidPromise;
}
