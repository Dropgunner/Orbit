import { diffLines } from "diff";

export interface DiffLine {
  type: "add" | "del" | "ctx";
  text: string;
}

function toLines(before: string | null, after: string): DiffLine[] {
  const parts = diffLines(before ?? "", after);
  const lines: DiffLine[] = [];
  for (const part of parts) {
    const type: DiffLine["type"] = part.added ? "add" : part.removed ? "del" : "ctx";
    const segments = part.value.split("\n");
    const body = segments[segments.length - 1] === "" ? segments.slice(0, -1) : segments;
    for (const segment of body) {
      lines.push({ type, text: segment });
    }
  }
  return lines;
}

export default function DiffView({ before, after }: { before: string | null; after: string }) {
  const lines = toLines(before, after);
  return (
    <div className="diff-view">
      {lines.map((line, i) => (
        <div key={i} className={"diff-line diff-" + line.type}>
          <span className="diff-marker">
            {line.type === "add" ? "+" : line.type === "del" ? "-" : " "}
          </span>
          <span className="diff-text">{line.text}</span>
        </div>
      ))}
    </div>
  );
}
