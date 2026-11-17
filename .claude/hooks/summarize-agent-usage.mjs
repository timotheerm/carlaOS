// Compact summary of .claude/logs/agent-usage.jsonl for /review-efficiency.
// Usage: node summarize-agent-usage.mjs [maxLines=300]   (reads only the last N log lines)
import fs from 'node:fs';
import path from 'node:path';

const root = process.env.CLAUDE_PROJECT_DIR || process.cwd();
const logFile = path.join(root, '.claude', 'logs', 'agent-usage.jsonl');
const max = Math.max(10, parseInt(process.argv[2] || '300', 10) || 300);
if (!fs.existsSync(logFile)) {
  console.log('No agent usage log yet (.claude/logs/agent-usage.jsonl). Hooks only record agents spawned after they were loaded; start a new session and delegate something.');
  process.exit(0);
}
const rows = fs.readFileSync(logFile, 'utf8').split('\n').filter(Boolean).slice(-max).flatMap((l) => {
  try { return [JSON.parse(l)]; } catch { return []; }
});
const starts = rows.filter((r) => r.event === 'start');
const calls = rows.filter((r) => r.event === 'call');

// An unset model on a call means the agent's frontmatter model was used (inferred, not logged).
const fallback = (agent) => {
  try {
    const t = fs.readFileSync(path.join(root, '.claude', 'agents', `${agent}.md`), 'utf8');
    return (t.match(/^model:\s*(\S+)/m) || [])[1] || null;
  } catch { return null; }
};
const tally = (arr, f) => arr.reduce((m, r) => ((m[f(r)] = (m[f(r)] || 0) + 1), m), {});
const fmt = (o) => Object.entries(o).sort((a, b) => b[1] - a[1]).map(([k, v]) => `${k}=${v}`).join(', ') || 'none';

const sessions = new Set(rows.map((r) => r.session).filter(Boolean));
console.log(`Window: last ${rows.length} log lines, ${rows[0]?.ts?.slice(0, 10) ?? '?'} to ${rows.at(-1)?.ts?.slice(0, 10) ?? '?'}, ${sessions.size} session(s).`);
console.log(`Agents started (SubagentStart, authoritative): ${starts.length}  -> ${fmt(tally(starts, (r) => r.agent || '?'))}`);
console.log(`Agent calls seen (PreToolUse): ${calls.length}`);

const modelOf = (c) => (c.model ? `${c.model} (requested)` : fallback(c.agent) ? `${fallback(c.agent)} (frontmatter fallback, inferred)` : 'unknown');
console.log(`Models on calls: ${fmt(tally(calls, modelOf))}`);

const opus = calls.filter((c) => (c.model || fallback(c.agent)) === 'opus');
console.log(`Opus calls: ${opus.length}${opus.length ? ' -> ' + opus.map((c) => `${c.agent}:"${c.label ?? ''}"`).join('; ') : ''}`);

// Repeated work: same agent + same label more than once in one session.
const key = (c) => `${c.session}|${c.agent}|${(c.label || '').toLowerCase()}`;
const dup = Object.entries(tally(calls.filter((c) => c.label), key)).filter(([, n]) => n > 1);
console.log(`Possible repeated work (same agent+label twice in a session): ${dup.length ? dup.map(([k, n]) => `${k.split('|').slice(1).join(' / ')} x${n}`).join('; ') : 'none'}`);

// Burst: several agents inside one minute in one session = possible parallel/duplicate fan-out.
const bursts = [];
for (const s of sessions) {
  const t = starts.filter((r) => r.session === s).map((r) => Date.parse(r.ts));
  for (let i = 0; i < t.length; i++) {
    const n = t.filter((x) => x >= t[i] && x < t[i] + 60000).length;
    if (n >= 3) { bursts.push(`${s.slice(0, 8)}: ${n} agents within 60s`); break; }
  }
}
console.log(`Fan-out bursts (>=3 agents in 60s): ${bursts.length ? bursts.join('; ') : 'none'}`);

const perSession = Object.values(tally(starts, (r) => r.session)).sort((a, b) => b - a);
console.log(`Agents per session (top): ${perSession.slice(0, 5).join(', ') || 'none'}`);
const gap = Math.abs(starts.length - calls.length);
if (gap) console.log(`Note: ${starts.length} starts vs ${calls.length} calls (differ by ${gap}); resumed agents, hooks loaded mid-session or a denied call can cause this.`);
console.log('Labels of last 8 calls: ' + (calls.slice(-8).map((c) => `${c.agent}[${c.model || 'default'}] ${c.label ?? ''}`).join(' | ') || 'none'));
