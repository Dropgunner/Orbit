// DeepSeek prompts and the report summary they are grounded in. Shared by the
// Ask tab, the automatic narrative that follows an analysis, and probe
// generation, so every DeepSeek call sees the same facts.

import type { BotReport, GuardReport, ProfileReport } from "../ipc/contract";

/** Any combination of the three Jev reports. */
export interface AnalysisReports {
  profile: ProfileReport | null;
  bot: BotReport | null;
  guard: GuardReport | null;
}

function fmt(value: number, digits = 2): string {
  return value.toFixed(digits);
}

/** A compact, factual summary of the current reports, sent as DeepSeek context. */
export function buildDiscussionContext(analysis: AnalysisReports | null): string {
  if (!analysis) return "";
  const parts: string[] = [];

  if (analysis.profile) {
    const { profile } = analysis;
    const ranked = [...profile.dimensions].sort((a, b) => b.score - a.score);
    const top = ranked
      .slice(0, 8)
      .map((d) => `${d.label} ${fmt(d.score)}`)
      .join(", ");
    const risky = ranked
      .filter((d) => d.polarity === "inverted-risk" && d.score >= 0.6)
      .map((d) => `${d.label} ${fmt(d.score)}`);
    parts.push(
      `Psychological profile of ${profile.author ?? profile.source}: ` +
        `${profile.scored_count} of ${profile.message_count} messages scored across ${profile.dimensions.length} dimensions.`,
      `Highest dimensions: ${top}.`,
      risky.length > 0
        ? `Elevated inverted-risk dimensions: ${risky.join(", ")}.`
        : "No inverted-risk dimension is elevated."
    );
  }

  if (analysis.bot) {
    const { bot } = analysis;
    const strongest = [...bot.dimensions]
      .sort((a, b) => b.score * b.weight - a.score * a.weight)
      .slice(0, 5)
      .map((d) => `${d.label} ${fmt(d.score)} (weight ${fmt(d.weight)})`)
      .join(", ");
    parts.push(
      `Bot detection: ${bot.verdict}, composite machine likelihood ${fmt(bot.likelihood)} ` +
        `over ${bot.scored_count} of ${bot.turn_count} turns.`,
      `Strongest weighted signals: ${strongest}.`
    );
  }

  if (analysis.guard) {
    const { guard } = analysis;
    parts.push(
      `Guardrails under the ${guard.policy} policy: ${guard.action}, severity ${fmt(guard.severity)}. ` +
        `Flagged hazards: ${guard.flagged.join(", ") || "none"}.`
    );
  }

  return parts.join(" ");
}

/** Asked automatically after an analysis when auto-narrative is on. */
export const NARRATIVE_PROMPT =
  "Write a short briefing for a human reviewer of these results. Cover what the numbers " +
  "actually support, which signals are strong versus weak, what the evidence does not show, " +
  "and the next thing to check. Be concrete and name the dimensions that moved. " +
  "Do not restate the tables, and do not make a decision about the person.";

/** Asked by the Generate probes action, alongside a fresh bot report. */
export const PROBE_PROMPT =
  "Write five follow-up probes that would separate a bot from a human in this conversation. " +
  "For each probe give the question to ask, what a bot-like answer looks like, and what a " +
  "human-like answer looks like. Ground each probe in a signal named in the report.";
