import { useEffect, useState } from "react";

const KEY = "gamex.showCode";
const EVENT = "gamex:advanced";

/** Code files (smali, Java, XML, scripts): shown only with "Show code" on. */
export const CODE_FILE = /\.(smali|java|kt|xml|gd|cs|js|lua|json|cfg|ini|txt)$/i;

/** Whether code is shown, read once (outside React). */
export function readShowCode(): boolean {
  return read();
}

function read(): boolean {
  try {
    return localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

/** Whether code is shown (the Advanced switch), kept between runs and shared by every screen. */
export function useShowCode(): [boolean, (on: boolean) => void] {
  const [on, setOn] = useState(read);
  useEffect(() => {
    const sync = () => setOn(read());
    window.addEventListener(EVENT, sync);
    return () => window.removeEventListener(EVENT, sync);
  }, []);
  const set = (next: boolean) => {
    try {
      localStorage.setItem(KEY, next ? "1" : "0");
    } catch {
      // Remembering is only a convenience.
    }
    setOn(next);
    window.dispatchEvent(new Event(EVENT));
  };
  return [on, set];
}
