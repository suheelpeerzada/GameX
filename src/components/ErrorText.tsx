import { Copy } from "lucide-react";
import { useState } from "react";
import { DETAILS } from "../lib/errors";

/**
 * An error as people read it: the message, and the original error behind a
 * Copy details button (for a bug report), never in the way.
 */
export default function ErrorText({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const at = text.indexOf(DETAILS);
  const message = at < 0 ? text : text.slice(0, at);
  const details = at < 0 ? null : text.slice(at + DETAILS.length);
  const copy = () =>
    navigator.clipboard
      .writeText(details ?? message)
      .then(() => setCopied(true))
      .catch(() => {});
  return (
    <span className="friendly-error">
      {message}
      <button className="link-button small" onClick={copy} title={details ?? "Copy this message"}>
        <Copy size={12} /> {copied ? "Copied" : "Copy details"}
      </button>
    </span>
  );
}
