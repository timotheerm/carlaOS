// Appends one metadata-only line per agent event to .claude/logs/agent-usage.jsonl (gitignored).
// Wired from .claude/settings.json: SubagentStart (an agent really started) and
// PreToolUse on the Agent tool (the coordinator's call, which carries the requested model).
// Logs a fixed whitelist of fields only: never prompts, tool results or message text.
import fs from 'node:fs';
import path from 'node:path';

let raw = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (c) => (raw += c));
process.stdin.on('end', () => {
  try {
    const e = JSON.parse(raw || '{}');
    const input = e.tool_input && typeof e.tool_input === 'object' ? e.tool_input : {};
    const clip = (v, n) => (typeof v === 'string' ? v.replace(/\s+/g, ' ').slice(0, n) : null);
    const rec = {
      ts: new Date().toISOString(),
      event: e.hook_event_name === 'SubagentStart' ? 'start' : e.hook_event_name === 'PreToolUse' ? 'call' : String(e.hook_event_name || 'unknown'),
      session: e.session_id || null,
      agent: e.agent_type || input.subagent_type || null,
      agent_id: e.agent_id || null,
      // Present only on the coordinator's call, and only if it overrode the agent's default.
      model: clip(input.model, 40),
      // The short task label the coordinator gave (3-5 words), clipped. Never the prompt.
      label: clip(input.description, 60),
    };
    const root = process.env.CLAUDE_PROJECT_DIR || e.cwd || process.cwd();
    const dir = path.join(root, '.claude', 'logs');
    fs.mkdirSync(dir, { recursive: true });
    fs.appendFileSync(path.join(dir, 'agent-usage.jsonl'), JSON.stringify(rec) + '\n');
  } catch {
    // Logging must never disturb a session.
  }
  process.exit(0);
});
