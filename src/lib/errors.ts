/**
 * Errors in words: what happened and what to try, for the cases people meet
 * most (a tool or Java missing, a file in use, not a game, no connection).
 * The original text is kept as the details, for "Copy details".
 */

export interface Friendly {
  /** What happened and what to try. */
  message: string;
  /** The error as it came, when the message says it differently. */
  details: string | null;
}

const CASES: { test: RegExp; say: (raw: string) => string }[] = [
  {
    test: /\bjava\b.*(not found|not installed|isn't installed|missing|no such file)|JAVA_HOME|'java' is not recognized|needs java/i,
    say: () => "Java isn't installed, and this step needs it. Install it in Settings → Tools (one click), then try again.",
  },
  {
    test: /\b(apktool|jadx|uber-apk-signer|signer|gdre|adb)\b.*(not found|missing|isn't installed|not installed|is not recognized)/i,
    say: (raw) => {
      const tool = raw.match(/\b(apktool|jadx|uber-apk-signer|signer|gdre|adb)\b/i)?.[1] ?? "a tool";
      return `A tool GameX needs (${tool}) isn't installed. Install it in Settings → Tools, then try again.`;
    },
  },
  {
    test: /os error 32|being used by another process|os error 33|locked/i,
    say: () => "A file is open in another program. Close programs that use this game's files (an editor, an emulator, Explorer's preview) and try again.",
  },
  {
    test: /os error 5\b|access is denied|permission denied/i,
    say: () => "Windows didn't let GameX change a file there. Pick another folder (not Program Files), or close programs using it, and try again.",
  },
  {
    test: /os error 112|no space left|not enough space|disk is full/i,
    say: () => "The disk is full. Free some space and try again.",
  },
  {
    test: /end of central directory|invalid zip|not a zip|invalid apk|isn't a game|not a game|unsupported file|invalid archive/i,
    say: () => "This doesn't look like a game file GameX can open. Pick the game's .apk, a .zip of its files, or its folder.",
  },
  {
    test: /no devices|device offline|device not found|adb.*(refused|failed)|unauthorized/i,
    say: () =>
      "GameX can't reach the emulator or phone. Make sure the emulator is running (or the phone is plugged in with USB debugging on and allowed), then try again.",
  },
  {
    test: /dns|could not resolve|connection (refused|reset)|timed out|network|offline|error sending request/i,
    say: () => "GameX couldn't reach the internet. Check your connection and try again.",
  },
  {
    test: /os error 2\b|os error 3\b|cannot find the (file|path)|no such file or directory/i,
    say: () => "A file GameX needed isn't there any more (it may have been moved or deleted). If it's the game you imported, import it again.",
  },
];

/** What went wrong, in words, with the original kept as details. */
export function friendlyError(error: unknown): Friendly {
  const raw = (error instanceof Error ? error.message : String(error ?? "Something went wrong.")).replace(/^Error:\s*/, "").trim();
  const known = CASES.find((c) => c.test.test(raw));
  return known ? { message: known.say(raw), details: raw } : { message: raw, details: null };
}

/** The same as one line of text, for the places that show plain text (details after a blank line). */
export function errorText(error: unknown): string {
  const f = friendlyError(error);
  return f.details ? `${f.message}${DETAILS}${f.details}` : f.message;
}

/** Splits text made by errorText back into the message and its details. */
export const DETAILS = "\n\nDetails: ";
