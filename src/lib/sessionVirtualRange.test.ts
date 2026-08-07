import { describe, expect, it } from "vitest";
import {
  resolveMeasuredHeightScrollOffset,
  resolveSessionVirtualRange,
  retainMeasuredHeightsForStableItems,
} from "./sessionVirtualRange";

describe("session virtual range", () => {
  it("returns no items for an empty transcript", () => {
    expect(
      resolveSessionVirtualRange({
        itemCount: 0,
        scrollTop: 0,
        viewportHeight: 400,
        estimatedItemHeight: 80,
        overscan: 2,
        measuredHeights: {},
      }),
    ).toEqual({ items: [], totalHeight: 0 });
  });

  it("selects visible messages with overscan", () => {
    const range = resolveSessionVirtualRange({
      itemCount: 100,
      scrollTop: 800,
      viewportHeight: 240,
      estimatedItemHeight: 80,
      overscan: 2,
      measuredHeights: {},
    });

    expect(range.totalHeight).toBe(8000);
    expect(range.items[0].index).toBe(7);
    expect(range.items[range.items.length - 1]?.index).toBe(16);
  });

  it("uses measured heights when calculating offsets", () => {
    const range = resolveSessionVirtualRange({
      itemCount: 4,
      scrollTop: 100,
      viewportHeight: 100,
      estimatedItemHeight: 50,
      overscan: 0,
      measuredHeights: {
        0: 120,
        1: 30,
      },
    });

    expect(range.totalHeight).toBe(250);
    expect(range.items.map((item) => item.index)).toEqual([0, 1, 2, 3]);
    expect(range.items.map((item) => item.start)).toEqual([0, 120, 150, 200]);
  });

  it("retains measured heights for stable transcript items", () => {
    const previousItems = [
      { role: "user", timestamp: 1, content: "draw a chart" },
      { role: "assistant", timestamp: 2, content: "done" },
    ];
    const nextItems = [
      previousItems[0],
      previousItems[1],
      { role: "assistant", timestamp: 3, content: "next" },
    ];

    expect(
      retainMeasuredHeightsForStableItems({
        currentHeights: { 0: 64, 1: 320, 2: 120 },
        previousItems,
        nextItems,
      }),
    ).toEqual({ 0: 64, 1: 320 });
  });

  it("drops measured heights when a transcript item changes in place", () => {
    const retained = retainMeasuredHeightsForStableItems({
      currentHeights: { 0: 64, 1: 320 },
      previousItems: [
        { role: "user", timestamp: 1, content: "draw a chart" },
        { role: "assistant", timestamp: 2, content: "old" },
      ],
      nextItems: [
        { role: "user", timestamp: 1, content: "draw a chart" },
        { role: "assistant", timestamp: 2, content: "new" },
      ],
    });

    expect(retained).toEqual({ 0: 64 });
  });

  it("compensates scroll position when a measured row above the viewport changes height", () => {
    expect(
      resolveMeasuredHeightScrollOffset({
        followTail: false,
        itemStart: 80,
        previousHeight: 120,
        nextHeight: 260,
        scrollTop: 300,
      }),
    ).toBe(140);
  });

  it("does not compensate rows inside the viewport or while following tail", () => {
    expect(
      resolveMeasuredHeightScrollOffset({
        followTail: false,
        itemStart: 260,
        previousHeight: 120,
        nextHeight: 260,
        scrollTop: 300,
      }),
    ).toBe(0);

    expect(
      resolveMeasuredHeightScrollOffset({
        followTail: true,
        itemStart: 80,
        previousHeight: 120,
        nextHeight: 260,
        scrollTop: 300,
      }),
    ).toBe(0);
  });
});
