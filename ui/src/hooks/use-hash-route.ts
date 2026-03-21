/** React hook that syncs HashRoute state ↔ window.location.hash bidirectionally. */

import { useState, useEffect, useCallback, useRef } from "react";
import { parseHash, buildHash, type HashRoute } from "@/lib/hash-route";

export function useHashRoute(): [HashRoute, (route: HashRoute) => void] {
  const [route, setRoute] = useState<HashRoute>(() => parseHash(window.location.hash));
  // Prevent re-parsing when we ourselves set the hash
  const selfUpdate = useRef(false);

  // Listen for external hash changes (back/forward, manual URL edit)
  useEffect(() => {
    const onHashChange = () => {
      if (selfUpdate.current) {
        selfUpdate.current = false;
        return;
      }
      setRoute(parseHash(window.location.hash));
    };
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  const navigate = useCallback((next: HashRoute) => {
    selfUpdate.current = true;
    setRoute(next);
    const hash = buildHash(next);
    if (window.location.hash !== hash) {
      window.location.hash = hash;
    } else {
      // Hash didn't change, so hashchange won't fire — reset flag
      selfUpdate.current = false;
    }
  }, []);

  return [route, navigate];
}
