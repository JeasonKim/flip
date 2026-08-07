export interface VirtualRangeInput {
  itemCount: number;
  scrollTop: number;
  viewportHeight: number;
  estimatedItemHeight: number;
  overscan: number;
  measuredHeights: Record<number, number>;
}

export interface VirtualRangeItem {
  index: number;
  start: number;
  height: number;
}

export interface VirtualRangeResult {
  items: VirtualRangeItem[];
  totalHeight: number;
}

export interface StableVirtualItem {
  role: string;
  timestamp: number | null;
  content: string;
}

export interface RetainMeasuredHeightsInput {
  currentHeights: Record<number, number>;
  previousItems: StableVirtualItem[];
  nextItems: StableVirtualItem[];
}

export interface MeasuredHeightScrollOffsetInput {
  followTail: boolean;
  itemStart: number;
  previousHeight: number;
  nextHeight: number;
  scrollTop: number;
}

export function resolveSessionVirtualRange(
  input: VirtualRangeInput,
): VirtualRangeResult {
  const itemCount = Math.max(0, input.itemCount);
  const estimatedItemHeight = Math.max(1, input.estimatedItemHeight);
  const overscan = Math.max(0, input.overscan);
  const viewportTop = Math.max(0, input.scrollTop);
  const viewportBottom = viewportTop + Math.max(0, input.viewportHeight);

  const starts: number[] = [];
  const heights: number[] = [];
  let totalHeight = 0;

  for (let index = 0; index < itemCount; index += 1) {
    const measuredHeight = input.measuredHeights[index];
    const height =
      typeof measuredHeight === "number" && measuredHeight > 0
        ? measuredHeight
        : estimatedItemHeight;

    starts.push(totalHeight);
    heights.push(height);
    totalHeight += height;
  }

  if (itemCount === 0) {
    return { items: [], totalHeight };
  }

  let firstVisibleIndex = 0;
  while (
    firstVisibleIndex < itemCount - 1 &&
    starts[firstVisibleIndex] + heights[firstVisibleIndex] < viewportTop
  ) {
    firstVisibleIndex += 1;
  }

  let lastVisibleIndex = firstVisibleIndex;
  while (
    lastVisibleIndex < itemCount - 1 &&
    starts[lastVisibleIndex] <= viewportBottom
  ) {
    lastVisibleIndex += 1;
  }

  const startIndex = Math.max(0, firstVisibleIndex - overscan);
  const endIndex = Math.min(itemCount - 1, lastVisibleIndex + overscan);
  const items: VirtualRangeItem[] = [];

  for (let index = startIndex; index <= endIndex; index += 1) {
    items.push({
      index,
      start: starts[index],
      height: heights[index],
    });
  }

  return { items, totalHeight };
}

export function retainMeasuredHeightsForStableItems(
  input: RetainMeasuredHeightsInput,
): Record<number, number> {
  const retainedHeights: Record<number, number> = {};
  let retainedCount = 0;
  const currentEntries = Object.entries(input.currentHeights);

  for (const [indexText, height] of currentEntries) {
    const index = Number(indexText);
    if (
      Number.isInteger(index) &&
      index >= 0 &&
      virtualItemsShareMeasuredHeight(
        input.previousItems[index],
        input.nextItems[index],
      )
    ) {
      retainedHeights[index] = height;
      retainedCount += 1;
    }
  }

  return retainedCount === currentEntries.length
    ? input.currentHeights
    : retainedHeights;
}

export function resolveMeasuredHeightScrollOffset(
  input: MeasuredHeightScrollOffsetInput,
): number {
  if (input.followTail) {
    return 0;
  }

  const previousHeight = Math.max(1, input.previousHeight);
  const nextHeight = Math.max(1, input.nextHeight);
  const heightDelta = nextHeight - previousHeight;
  if (Math.abs(heightDelta) < 1) {
    return 0;
  }

  const viewportTop = Math.max(0, input.scrollTop);
  const itemBottomBeforeMeasurement = Math.max(0, input.itemStart) + previousHeight;
  return itemBottomBeforeMeasurement <= viewportTop ? heightDelta : 0;
}

function virtualItemsShareMeasuredHeight(
  previousItem: StableVirtualItem | undefined,
  nextItem: StableVirtualItem | undefined,
): boolean {
  return (
    previousItem !== undefined &&
    nextItem !== undefined &&
    previousItem.role === nextItem.role &&
    previousItem.timestamp === nextItem.timestamp &&
    previousItem.content === nextItem.content
  );
}
