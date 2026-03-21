/**
 * Pure functional BDD helpers.
 *
 * Data flows explicitly: given() returns context, when() transforms it,
 * then() asserts on the output. No shared mutable state, no hidden closures.
 *
 * Usage:
 *   scenario(
 *     () => ({ events: makeEvents(10), filter: { category: 'tools' } }),
 *     ({ events, filter }) => events.filter(e => matchesFilter(e, filter)),
 *     (filtered) => expect(filtered).toHaveLength(3),
 *   );
 */

/** Synchronous scenario: given → when → then as a pure pipeline. */
export function scenario<G, W>(
  given: () => G,
  when: (context: G) => W,
  then: (result: W) => void,
): void {
  const context = given();
  const result = when(context);
  then(result);
}

/** Async scenario: for specs involving promises, observables, or timers. */
export async function scenarioAsync<G, W>(
  given: () => G | Promise<G>,
  when: (context: G) => W | Promise<W>,
  then: (result: W) => void | Promise<void>,
): Promise<void> {
  const context = await given();
  const result = await when(context);
  await then(result);
}
