import { useState, useEffect, useRef } from "react";
import type { Observable } from "rxjs";

/**
 * Core bridge between RxJS and React.
 * Subscribes to an Observable and returns its latest value.
 */
export function useObservable<T>(obs$: Observable<T>, initial: T): T {
  const [value, setValue] = useState<T>(initial);
  const obsRef = useRef(obs$);
  obsRef.current = obs$;

  useEffect(() => {
    const sub = obsRef.current.subscribe(setValue);
    return () => sub.unsubscribe();
  }, [obs$]);

  return value;
}
