import type { ContainerKind, GameProject } from "./types";

/**
 * The parts of GameX a project can use, decided from its analysis. Parts a
 * game can't use are never shown or loaded, so their code isn't downloaded
 * into the window and nothing of theirs runs in the background.
 */
export interface Features {
  /** Run on an emulator or phone (Android games). */
  run: boolean;
  /** Build a changed APK. */
  build: boolean;
  /** Live editing: the game's Java objects while it runs (Java/Kotlin games only). */
  live: boolean;
}

export function projectFeatures(container: ContainerKind, analysis: GameProject | null): Features {
  const apk = container === "apk";
  return {
    run: apk,
    build: apk,
    // Live editing reads Java objects, so the game's own code must be Java/Kotlin.
    live: apk && analysis?.engine.id === "android-native",
  };
}
