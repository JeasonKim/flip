export interface RawRecordSnapshot {
  key: string;
  text: string;
}

export interface RawSearchMatch {
  recordKey: string;
  matchIndex: number;
}

export type RawSearchDirection = "next" | "previous";

export function collectRawSearchMatches(
  records: RawRecordSnapshot[],
  query: string,
): RawSearchMatch[] {
  const normalizedQuery = query.trim().toLowerCase();
  if (!normalizedQuery) {
    return [];
  }

  return records.flatMap((record) => {
    const normalizedText = record.text.toLowerCase();
    const matches: RawSearchMatch[] = [];
    let fromIndex = 0;

    while (fromIndex < normalizedText.length) {
      const matchIndex = normalizedText.indexOf(normalizedQuery, fromIndex);
      if (matchIndex === -1) {
        break;
      }

      matches.push({
        recordKey: record.key,
        matchIndex,
      });
      fromIndex = matchIndex + normalizedQuery.length;
    }

    return matches;
  });
}

export function resolveRawSearchCursor(
  currentIndex: number,
  totalMatches: number,
  direction: RawSearchDirection,
): number {
  if (totalMatches === 0) {
    return -1;
  }

  if (currentIndex < 0) {
    return direction === "next" ? 0 : totalMatches - 1;
  }

  if (direction === "next") {
    return (currentIndex + 1) % totalMatches;
  }

  return (currentIndex - 1 + totalMatches) % totalMatches;
}
