import type { Tool } from "../../record.ts";

export interface BehaviorRow {
  tool: Capitalize<Tool>;
  input: string;
  cwd: string;
  glob?: string;
  claude: 0 | 2;
  codex?: 0 | 2;
  codex_reason?: string;
  note?: string;
}
