# Project conventions

## Workflow (token-efficient by default)

- Explore before editing: use `find`, `grep`, and `ls` to locate code, then `read` only the
  relevant ranges (pass `offset`/`limit`). Never dump whole files or the whole repo into context.
- Prefer the `edit` tool with small, targeted `oldText` matches over rewriting files with `write`.
- Run cheap, fast checks (typecheck, one failing test) before running the full suite.
- One logical change per turn. Commit working states frequently.

## Engineering principles (Hamilton + Akin)

Full set + sources: skill `engineering-principles` (`.agents/skills/engineering-principles/`).

- **Detect errors at the earliest possible point** (Hamilton): cheap checks before expensive
  ones; validate inputs at every boundary — never silently default or coerce on invalid input.
- **Design to operate when some things are wrong** (Akin 2): every delegated call needs an
  error path; failures, truncation, and aborts must be reported loudly, never passed off as success.
- **Numbers, not opinions** (Akin 1): decisions are backed by measurements (Zed usage panel,
  test results, timings). Estimates are allowed only if marked as such and revisited (Akin 10).
- **Errors migrate to interfaces** (Hamilton, Shea 15): define the contract before the
  implementation; validate at the boundary.
- **Don't build better than requirements dictate** (Akin 13/14): YAGNI; "better" is the enemy
  of "good" — ship when checks pass.
- **Verify extraordinary claims** (Akin 19): sanity-check sub-agent/model answers before
  passing them through; previous analysis is not truth (Akin 16).
- **When in doubt, document** (Akin 22): record decisions, assumptions, and numbers.

## Fact-check gate (every reply, 100%)

Every reply passes this gate before being sent. Classify each factual claim (paths, numbers,
dates, prices, quotes, "X is present/absent in Y", "I verified/checked") into exactly one of:

- **Proven** — backed by a command/output shown in the reply itself (command + result, or
  file:line reference).
- **Unverified** — no direct evidence; must be labeled inline `[не проверено]` / `[assumption]`.

Hard rules:

1. Never present an unverified claim as proven. Run the cheap check (`grep`, `ls`, `read`)
   before asserting (H1). "I verified it" without showing the check is a violation.
2. No invented numbers, quotes, prices, paths, or dates. A number without a source is an
   estimate — say so explicitly.
3. A check that found nothing is reported literally ("`grep X` → 0 matches"), never
   paraphrased as "checked, fine".
4. Extraordinary claims get a sanity check before leaving the reply (A19); previous analysis
   is not truth — re-verify before passing through (A16).
5. Verification-heavy turns (cross-file consistency audits, fact-checking) — escalate to
   `deepseek-v4-pro`; flash is for routine edits.
6. Every reply ends with a one-line footer: `Fact-check: <n> proven / <m> unverified / <k>
checks run this turn`. No footer = gate skipped = failure.

## Zed-native harness

- Rules: this `AGENTS.md` is the project rule file for the Zed Agent.
- Skills and slash commands: `.agents/skills/` (e.g. `/plan`, `/review`, `/ship`, `/team`, `/harness`).
- ZCode roles: `.agents/roles/` — Zed has one native agent, so a role is a self-contained brief passed to `spawn_agent`, not a separately registered agent.
- MCP: `context_servers` in `.zed/settings.json`; LSP: native Zed LSP plus Serena's `.serena/project.yml`.
- Memory: `.agents/memory/memory.json` via the `memory` MCP server.
- Legacy pi.dev config lives in `.pi/` and is **not** read by the Zed Agent. `tools/pi-cost/` is pi-specific; use Zed's usage panel for session cost.

## Model usage (DeepSeek)

- `deepseek-flash` is the default workhorse: cheapest input/output and automatic context caching.
- Escalate to `deepseek-v4-pro` (model picker + high thinking) only for hard
  architecture, subtle debugging, or security-sensitive reasoning.
- Keep the thinking level low for routine edits: reasoning tokens are billed as output and stay
  in the conversation context.

## MCP usage (token-efficient)

Full rationale + sources: skill `mcp-usage` (`.agents/skills/mcp-usage/`).

- **New capability → cheapest option first** (A13): skill / CLI script / native tool;
  a context server only for an external ecosystem (browser, DB, GitHub/Jira).
- **Every enabled context server is an every-turn tax**: Zed has no deferred exposure, so
  `context_servers` must stay minimal; a rarely-used server is cheaper enabled on demand.
- **Skills are the on-demand layer**: a markdown file read only when the task matches.
- **Bulk = one script**: N > 1 uniform calls in a single shell/Node run, reduced result only.
- **Every MCP call is a delegated call** (A2): check errors/timeouts; a failed call is never success.
- **Numbers, not opinions** (A1): "MCP is expensive/underused" → measure first.

## Hygiene

- Keep the system prompt and tool set stable during a session so DeepSeek's automatic prefix
  cache stays warm (cache hits are ~30-50x cheaper than misses).
- Review tool output before trusting destructive commands.
- Do not commit secrets or `auth.json`.

## Tools

- `node tools/harness_check.mjs` — structural self-check of skills, roles, MCP/LSP config, memory, and tool availability.
- `node tools/token-thrift-check.mjs` — gate that the token-saving rules are present in AGENTS.md and every referenced skill/tool resolves.
- `node tools/deepseek-balance.mjs` — real DeepSeek account balance; requires `DEEPSEEK_API_KEY`, never prints the key.
- Session cost: use the Zed agent usage panel. `tools/pi-cost/` is pi-specific and is not used by this project.
