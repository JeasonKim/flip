import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { RefObject } from "react";
import type { SessionMessage } from "../../types/profile";
import {
  resolveMeasuredHeightScrollOffset,
  resolveSessionVirtualRange,
  retainMeasuredHeightsForStableItems,
  type VirtualRangeItem,
} from "../../lib/sessionVirtualRange";
import MessageBubble from "./MessageBubble";

const ESTIMATED_MESSAGE_HEIGHT = 120;
const MESSAGE_OVERSCAN = 8;

interface VirtualMessageListProps {
  messages: SessionMessage[];
  scrollRef: RefObject<HTMLDivElement | null>;
  followTail: boolean;
  onProgrammaticScroll: () => void;
}

export default function VirtualMessageList({
  messages,
  scrollRef,
  followTail,
  onProgrammaticScroll,
}: VirtualMessageListProps) {
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);
  const [measuredHeights, setMeasuredHeights] = useState<Record<number, number>>(
    {},
  );
  const measuredHeightsRef = useRef<Record<number, number>>({});
  const previousMessagesRef = useRef<SessionMessage[]>([]);

  const synchronizeViewport = useCallback(() => {
    const scrollElement = scrollRef.current;
    if (!scrollElement) {
      return;
    }

    setScrollTop(scrollElement.scrollTop);
    setViewportHeight(scrollElement.clientHeight);
  }, [scrollRef]);

  const scrollToMeasuredBottom = useCallback(() => {
    const scrollElement = scrollRef.current;
    if (!scrollElement) {
      return;
    }

    onProgrammaticScroll();
    scrollElement.scrollTop = scrollElement.scrollHeight;
    setScrollTop(scrollElement.scrollTop);
    setViewportHeight(scrollElement.clientHeight);
  }, [onProgrammaticScroll, scrollRef]);

  useEffect(() => {
    const retainedHeights = retainMeasuredHeightsForStableItems({
      currentHeights: measuredHeightsRef.current,
      previousItems: previousMessagesRef.current,
      nextItems: messages,
    });

    measuredHeightsRef.current = retainedHeights;
    previousMessagesRef.current = messages;
    setMeasuredHeights(retainedHeights);
    requestAnimationFrame(synchronizeViewport);
  }, [messages, synchronizeViewport]);

  useEffect(() => {
    const scrollElement = scrollRef.current;
    if (!scrollElement) {
      return;
    }

    synchronizeViewport();
    scrollElement.addEventListener("scroll", synchronizeViewport, {
      passive: true,
    });

    const resizeObserver = new ResizeObserver(synchronizeViewport);
    resizeObserver.observe(scrollElement);

    return () => {
      scrollElement.removeEventListener("scroll", synchronizeViewport);
      resizeObserver.disconnect();
    };
  }, [scrollRef, synchronizeViewport]);

  const virtualRange = useMemo(
    () =>
      resolveSessionVirtualRange({
        itemCount: messages.length,
        scrollTop,
        viewportHeight,
        estimatedItemHeight: ESTIMATED_MESSAGE_HEIGHT,
        overscan: MESSAGE_OVERSCAN,
        measuredHeights,
      }),
    [measuredHeights, messages.length, scrollTop, viewportHeight],
  );

  useLayoutEffect(() => {
    if (!followTail) {
      return;
    }

    scrollToMeasuredBottom();
    const frame = requestAnimationFrame(scrollToMeasuredBottom);
    return () => cancelAnimationFrame(frame);
  }, [
    followTail,
    messages.length,
    scrollToMeasuredBottom,
    virtualRange.totalHeight,
  ]);

  const recordMeasuredHeight = useCallback(
    (item: VirtualRangeItem, measuredHeight: number) => {
      const normalizedHeight = Math.max(1, measuredHeight);
      const currentHeights = measuredHeightsRef.current;
      const previousHeight = currentHeights[item.index] ?? item.height;
      if (Math.abs(previousHeight - normalizedHeight) < 1) {
        return;
      }

      const scrollElement = scrollRef.current;
      const scrollOffset = resolveMeasuredHeightScrollOffset({
        followTail,
        itemStart: item.start,
        previousHeight,
        nextHeight: normalizedHeight,
        scrollTop: scrollElement?.scrollTop ?? scrollTop,
      });

      const nextHeights = {
        ...currentHeights,
        [item.index]: normalizedHeight,
      };
      measuredHeightsRef.current = nextHeights;
      setMeasuredHeights(nextHeights);

      if (scrollElement && scrollOffset !== 0) {
        onProgrammaticScroll();
        scrollElement.scrollTop += scrollOffset;
        setScrollTop(scrollElement.scrollTop);
        setViewportHeight(scrollElement.clientHeight);
      }
    },
    [followTail, onProgrammaticScroll, scrollRef, scrollTop],
  );

  return (
    <div
      className="relative w-full"
      style={{ height: `${virtualRange.totalHeight}px` }}
    >
      {virtualRange.items.map((item) => {
        const message = messages[item.index];
        return (
          <VirtualMessageRow
            key={`${item.index}-${message.role}-${message.timestamp ?? "na"}`}
            item={item}
            message={message}
            onMeasure={recordMeasuredHeight}
          />
        );
      })}
    </div>
  );
}

interface VirtualMessageRowProps {
  item: VirtualRangeItem;
  message: SessionMessage;
  onMeasure: (item: VirtualRangeItem, height: number) => void;
}

function VirtualMessageRow({
  item,
  message,
  onMeasure,
}: VirtualMessageRowProps) {
  const rowRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const rowElement = rowRef.current;
    if (!rowElement) {
      return;
    }

    const measureRow = () => {
      onMeasure(item, rowElement.getBoundingClientRect().height);
    };

    measureRow();
    const resizeObserver = new ResizeObserver(measureRow);
    resizeObserver.observe(rowElement);

    return () => resizeObserver.disconnect();
  }, [item, onMeasure]);

  return (
    <div
      ref={rowRef}
      className="absolute left-0 right-1 py-1"
      style={{ transform: `translateY(${item.start}px)` }}
    >
      <MessageBubble message={message} />
    </div>
  );
}
