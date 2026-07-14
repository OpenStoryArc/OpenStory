/** Persist a boolean UI preference to localStorage (quota/SSR-safe). */

import { useState } from "react";

export function usePersistedFlag(key: string, initial: boolean): [boolean, (v: boolean) => void] {
  const [v, setV] = useState<boolean>(() => {
    try {
      const s = localStorage.getItem(key);
      return s == null ? initial : s === "1";
    } catch {
      return initial;
    }
  });
  const set = (nv: boolean) => {
    setV(nv);
    try {
      localStorage.setItem(key, nv ? "1" : "0");
    } catch {
      /* ignore quota/SSR */
    }
  };
  return [v, set];
}
