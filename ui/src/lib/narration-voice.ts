/** Pick the best installed voice for reel narration.
 *
 *  Pure: takes whatever `speechSynthesis.getVoices()` reports, returns the
 *  voice to set on the utterance, or null to keep the browser default. The
 *  preference list leads with the premium macOS voices the tour scripts
 *  already use (`OS_TOUR_VOICE`), matched by name prefix because Chrome may
 *  list "Serena (Premium)" as plain "Serena". After the list: any local
 *  English voice, then any English voice, then the browser default. */

export const NARRATION_VOICE_PREFERENCES: readonly string[] = [
  "Serena (Premium)",
  "Serena",
  "Samantha (Enhanced)",
  "Samantha",
  "Ava (Premium)",
  "Ava",
  "Jamie (Premium)",
  "Jamie",
  "Kate (Enhanced)",
  "Kate",
  "Zoe (Premium)",
  "Zoe",
];

/** The subset of SpeechSynthesisVoice this module reads. */
export interface NarrationVoice {
  readonly name: string;
  readonly lang: string;
  readonly localService: boolean;
  readonly default: boolean;
}

export function pickNarrationVoice<V extends NarrationVoice>(voices: readonly V[]): V | null {
  if (voices.length === 0) return null;
  for (const pref of NARRATION_VOICE_PREFERENCES) {
    const hit = voices.find((v) => v.name === pref || v.name.startsWith(`${pref} `) || v.name.startsWith(`${pref}(`));
    if (hit) return hit;
  }
  const english = (v: V) => v.lang.toLowerCase().startsWith("en");
  return (
    voices.find((v) => english(v) && v.localService) ??
    voices.find(english) ??
    voices.find((v) => v.default) ??
    voices[0] ??
    null
  );
}
