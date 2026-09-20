import { useEffect, useRef, useState } from "react";
import { useStore, type ChatMsg } from "../state/store";
import { renderMarkdown } from "../lib/markdown";

function ToolMessage({ m }: { m: ChatMsg }) {
  return (
    <details className="chat-tool">
      <summary>
        <span className="tool-icon">🔧</span>
        <span className="tool-name">{m.toolName ?? "tool"}</span>
      </summary>
      {m.toolArgs && (
        <pre className="tool-block">{m.toolArgs.slice(0, 2000)}</pre>
      )}
      {m.toolResult && (
        <pre className="tool-block">{m.toolResult.slice(0, 4000)}</pre>
      )}
    </details>
  );
}

export default function ChatPanel() {
  const chat = useStore((s) => s.chat);
  const streaming = useStore((s) => s.streaming);
  const sendChat = useStore((s) => s.sendChat);
  const stopChat = useStore((s) => s.stopChat);
  const projectRoot = useStore((s) => s.projectRoot);
  const [input, setInput] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [chat]);

  function send() {
    const text = input.trim();
    if (!text) return;
    setInput("");
    void sendChat(text);
  }

  return (
    <div className="chat-panel">
      <div className="chat-header">Agent</div>
      <div className="chat-messages" ref={scrollRef}>
        {chat.length === 0 && (
          <div className="chat-empty">
            Ask the agent to explain or change your code. Proposed edits are
            reviewed before they are written to disk.
          </div>
        )}
        {chat.map((m) => {
          if (m.role === "tool") return <ToolMessage key={m.id} m={m} />;
          const html = m.role === "assistant" ? renderMarkdown(m.content) : null;
          return (
            <div key={m.id} className={"chat-msg " + m.role}>
              <div className="chat-role">{m.role === "user" ? "You" : "Orbit"}</div>
              {m.role === "assistant" ? (
                <div className="chat-md" dangerouslySetInnerHTML={{ __html: html ?? "" }} />
              ) : (
                <div className="chat-text">{m.content}</div>
              )}
              {m.streaming && <span className="chat-cursor" />}
            </div>
          );
        })}
      </div>
      <div className="chat-input-row">
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
          placeholder={projectRoot ? "Ask about your code… (Enter to send)" : "Open a folder first"}
          disabled={!projectRoot}
          rows={2}
        />
        {streaming ? (
          <button className="chat-stop" onClick={() => void stopChat()}>
            Stop
          </button>
        ) : (
          <button className="chat-send" onClick={send} disabled={!projectRoot}>
            Send
          </button>
        )}
      </div>
    </div>
  );
}
