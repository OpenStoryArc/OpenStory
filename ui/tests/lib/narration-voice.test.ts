/** narration-voice — pick the best installed voice for reel narration.
 *
 *  The reel player used to hand the utterance to the browser with no voice
 *  set, so Chrome read every reel in the system default (on the author's
 *  Mac: the basic, non-enhanced Daniel). The same machine had four premium
 *  and enhanced voices installed, and the tour scripts already prefer them
 *  ("Serena (Premium)"). This module makes the web player agree. */

import { describe, expect, it } from "vitest";
import { pickNarrationVoice, NARRATION_VOICE_PREFERENCES } from "@/lib/narration-voice";

type V = { name: string; lang: string; localService: boolean; default: boolean };
const v = (name: string, lang = "en-US", localService = true, def = false): V => ({
  name,
  lang,
  localService,
  default: def,
});

describe("when a premium voice from the preference list is installed", () => {
  it("should pick the highest-ranked preference, not the browser default", () => {
    const voices = [v("Daniel", "en-GB", true, true), v("Samantha (Enhanced)"), v("Serena (Premium)", "en-GB")];
    expect(pickNarrationVoice(voices)?.name).toBe("Serena (Premium)");
  });

  it("should match by name prefix, since Chrome may list a premium voice by its plain name", () => {
    const voices = [v("Daniel", "en-GB", true, true), v("Serena", "en-GB")];
    expect(pickNarrationVoice(voices)?.name).toBe("Serena");
  });
});

describe("when no preferred voice is installed", () => {
  it("should fall back to a local English voice before a remote one", () => {
    const voices = [v("Google US English", "en-US", false), v("Fred", "en-US", true), v("Thomas", "fr-FR", true)];
    expect(pickNarrationVoice(voices)?.name).toBe("Fred");
  });

  it("should fall back to the browser default when nothing is English", () => {
    const voices = [v("Thomas", "fr-FR", true, true), v("Anna", "de-DE", true)];
    expect(pickNarrationVoice(voices)?.name).toBe("Thomas");
  });
});

describe("when the browser has not loaded its voices yet", () => {
  it("should return null so the utterance keeps the browser default", () => {
    expect(pickNarrationVoice([])).toBeNull();
  });
});

describe("the preference list", () => {
  it("should lead with the voice the tour scripts already use", () => {
    expect(NARRATION_VOICE_PREFERENCES[0]).toBe("Serena (Premium)");
  });
});
