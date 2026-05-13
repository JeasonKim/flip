const mermaidStartPatterns = [
  /^(flowchart|graph)\s+(TB|TD|BT|RL|LR)\b/,
  /^(sequenceDiagram|classDiagram|stateDiagram(?:-v2)?|erDiagram)\b/,
  /^(journey|gantt|pie|mindmap|timeline|gitGraph)\b/,
  /^(quadrantChart|requirementDiagram|sankey-beta|xychart-beta)\b/,
  /^(block-beta|packet-beta|architecture-beta|kanban)\b/,
  /^C4(Context|Container|Component|Dynamic|Deployment)\b/,
];

const mermaidContinuationPatterns = [
  /^(subgraph|end|direction|style|classDef|class|linkStyle|click)\b/,
  /^(accTitle|accDescr)\b/,
  /^(title|section|dateFormat|axisFormat|tickInterval|excludes|todayMarker)\b/,
  /^(participant|actor|autonumber|activate|deactivate|note|loop|alt|else|opt|par|and|rect|critical|break)\b/i,
  /(?:-->|---|-.->|==>|--\|.*\||--.*-->|--.*---)/,
];

function trimmedLine(line: string): string {
  return line.trim();
}

export function isMermaidDiagramSource(source: string): boolean {
  const firstContentLine = source.split(/\r?\n/).find((line) => trimmedLine(line) !== "");
  if (!firstContentLine) return false;
  return isMermaidDiagramStartLine(firstContentLine);
}

export function normalizeMarkdownDiagrams(markdown: string): string {
  const lines = markdown.split(/\r?\n/);
  const normalizedLines: string[] = [];
  let inFence = false;
  let fenceMarker: string | null = null;

  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const marker = parseFenceMarker(line);

    if (marker) {
      if (!inFence) {
        inFence = true;
        fenceMarker = marker;
      } else if (fenceMarker && marker.startsWith(fenceMarker[0])) {
        inFence = false;
        fenceMarker = null;
      }
      normalizedLines.push(line);
      continue;
    }

    if (!inFence && isMermaidDiagramStartLine(line)) {
      normalizedLines.push("```mermaid");
      normalizedLines.push(line);

      while (index + 1 < lines.length && isMermaidDiagramContinuationLine(lines[index + 1])) {
        index += 1;
        normalizedLines.push(lines[index]);
      }

      normalizedLines.push("```");
      continue;
    }

    normalizedLines.push(line);
  }

  return normalizedLines.join("\n");
}

function parseFenceMarker(line: string): string | null {
  const match = line.trimStart().match(/^(```+|~~~+)/);
  return match?.[1] ?? null;
}

function isMermaidDiagramStartLine(line: string): boolean {
  const normalized = trimmedLine(line);
  return mermaidStartPatterns.some((pattern) => pattern.test(normalized));
}

function isMermaidDiagramContinuationLine(line: string): boolean {
  const normalized = trimmedLine(line);
  if (!normalized) return false;
  if (/^\s+/.test(line)) return true;
  if (isMermaidDiagramStartLine(line)) return true;
  return mermaidContinuationPatterns.some((pattern) => pattern.test(normalized));
}
