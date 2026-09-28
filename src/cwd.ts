// Keep speculative directories bounded without losing broad home roots.
export function boundedDirectories(current: string, candidates: string[], home: string): string[] {
  const distinct = [...new Set(candidates)].filter((path) => path !== current);
  return distinct.length > 16 ? [home, `${home}/Library`] : distinct;
}
