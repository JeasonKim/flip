import type { AgentId } from "../../types/profile";

interface ProjectFilterProps {
  projects: string[];
  agentFilter?: AgentId;
  projectFilter?: string;
  onAgentChange: (agent: AgentId | undefined) => void;
  onProjectChange: (project: string | undefined) => void;
}

export default function ProjectFilter({
  projects,
  agentFilter,
  projectFilter,
  onAgentChange,
  onProjectChange,
}: ProjectFilterProps) {
  return (
    <div className="flex items-center gap-3 flex-wrap">
      {/* Agent 切换 */}
      <div className="flex items-center gap-1 bg-white/5 rounded-lg p-0.5">
        {(["all", "claude", "codex"] as const).map((val) => {
          const isActive =
            val === "all" ? !agentFilter : agentFilter === val;
          return (
            <button
              key={val}
              onClick={() =>
                onAgentChange(val === "all" ? undefined : val)
              }
              className={`px-3 py-1 text-xs rounded-md transition-colors ${
                isActive
                  ? "bg-white/15 text-white"
                  : "text-gray-400 hover:text-gray-200"
              }`}
            >
              {val === "all" ? "All" : val === "claude" ? "Claude" : "Codex"}
            </button>
          );
        })}
      </div>

      {/* 项目筛选 */}
      {projects.length > 0 && (
        <select
          value={projectFilter ?? ""}
          onChange={(e) =>
            onProjectChange(e.target.value || undefined)
          }
          className="bg-white/5 text-gray-300 text-xs rounded-lg px-3 py-1.5 border border-white/10
            focus:outline-none focus:ring-1 focus:ring-white/20"
        >
          <option value="">All projects</option>
          {projects.map((p) => (
            <option key={p} value={p}>
              {p.split("/").pop() ?? p}
            </option>
          ))}
        </select>
      )}
    </div>
  );
}
