// Preserve edits made since `base`, while adopting unrelated external changes
// and backend normalization. Preference lists are edited as complete values.
export function rebasePreferences<T>(draft: T, base: T, current: T): T {
  if (JSON.stringify(draft) === JSON.stringify(base)) return current;
  if (
    draft && base && current &&
    typeof draft === 'object' && typeof base === 'object' && typeof current === 'object' &&
    !Array.isArray(draft) && !Array.isArray(base) && !Array.isArray(current)
  ) {
    const result = { ...current } as Record<string, unknown>;
    const edited = draft as Record<string, unknown>;
    const before = base as Record<string, unknown>;
    for (const key of new Set([...Object.keys(edited), ...Object.keys(before)])) {
      if (key in edited) result[key] = rebasePreferences(edited[key], before[key], result[key]);
      else delete result[key];
    }
    return result as T;
  }
  return draft;
}
