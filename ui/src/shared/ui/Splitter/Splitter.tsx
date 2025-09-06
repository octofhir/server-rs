import { Box } from "@mantine/core";
import type React from "react";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import styles from "./Splitter.module.css";

export interface SplitterProps {
  direction: "horizontal" | "vertical";
  defaultSize?: number;
  minSize?: number;
  maxSize?: number;
  onResize?: (size: number) => void;
  disabled?: boolean;
  children: [ReactNode, ReactNode];
  className?: string;
  resizerClassName?: string;
  snapPositions?: number[];
  snapThreshold?: number;
}

export const Splitter: React.FC<SplitterProps> = ({
  direction,
  defaultSize = 50,
  minSize = 10,
  maxSize = 90,
  onResize,
  disabled = false,
  children,
  className,
  resizerClassName,
  snapPositions = [],
  snapThreshold = 5,
}) => {
  const [size, setSize] = useState(defaultSize);
  const [isDragging, setIsDragging] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);
  const resizerRef = useRef<HTMLDivElement>(null);
  const startPositionRef = useRef(0);
  const startSizeRef = useRef(0);

  const isHorizontal = direction === "horizontal";

  // Get container dimensions
  const getContainerSize = useCallback(() => {
    if (!containerRef.current) return 0;
    return isHorizontal ? containerRef.current.offsetWidth : containerRef.current.offsetHeight;
  }, [isHorizontal]);

  // Convert percentage to pixels
  const percentageToPixels = useCallback(
    (percentage: number) => {
      return (percentage / 100) * getContainerSize();
    },
    [getContainerSize]
  );

  // Convert pixels to percentage
  const pixelsToPercentage = useCallback(
    (pixels: number) => {
      const containerSize = getContainerSize();
      return containerSize > 0 ? (pixels / containerSize) * 100 : 0;
    },
    [getContainerSize]
  );

  // Apply snap positions
  const applySnap = useCallback(
    (newSize: number) => {
      for (const snapPos of snapPositions) {
        if (Math.abs(newSize - snapPos) <= snapThreshold) {
          return snapPos;
        }
      }
      return newSize;
    },
    [snapPositions, snapThreshold]
  );

  // Update size with constraints
  const updateSize = useCallback(
    (newSize: number) => {
      const constrainedSize = Math.min(Math.max(newSize, minSize), maxSize);
      const snappedSize = applySnap(constrainedSize);

      setSize(snappedSize);
      onResize?.(snappedSize);
    },
    [minSize, maxSize, onResize, applySnap]
  );

  // Handle mouse events
  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (disabled) return;

      e.preventDefault();
      setIsDragging(true);
      startPositionRef.current = isHorizontal ? e.clientX : e.clientY;
      startSizeRef.current = percentageToPixels(size);

      // Prevent text selection
      document.body.style.userSelect = "none";
      document.body.style.cursor = isHorizontal ? "col-resize" : "row-resize";
    },
    [disabled, isHorizontal, size, percentageToPixels]
  );

  // Handle touch events
  const handleTouchStart = useCallback(
    (e: React.TouchEvent) => {
      if (disabled || e.touches.length !== 1) return;

      const touch = e.touches[0];
      setIsDragging(true);
      startPositionRef.current = isHorizontal ? touch.clientX : touch.clientY;
      startSizeRef.current = percentageToPixels(size);
    },
    [disabled, isHorizontal, size, percentageToPixels]
  );

  // Handle move events
  const handleMove = useCallback(
    (clientX: number, clientY: number) => {
      if (!isDragging) return;

      const currentPosition = isHorizontal ? clientX : clientY;
      const delta = currentPosition - startPositionRef.current;
      const newSizePixels = startSizeRef.current + delta;
      const newSizePercentage = pixelsToPercentage(newSizePixels);

      updateSize(newSizePercentage);
    },
    [isDragging, isHorizontal, pixelsToPercentage, updateSize]
  );

  const handleMouseMove = useCallback(
    (e: MouseEvent) => {
      handleMove(e.clientX, e.clientY);
    },
    [handleMove]
  );

  const handleTouchMove = useCallback(
    (e: TouchEvent) => {
      if (e.touches.length !== 1) return;
      e.preventDefault(); // Prevent scrolling
      const touch = e.touches[0];
      handleMove(touch.clientX, touch.clientY);
    },
    [handleMove]
  );

  // Handle end events
  const handleEnd = useCallback(() => {
    if (isDragging) {
      setIsDragging(false);
      document.body.style.userSelect = "";
      document.body.style.cursor = "";
    }
  }, [isDragging]);

  // Keyboard navigation
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (disabled) return;

      const { key, ctrlKey, metaKey } = e;
      const modifier = ctrlKey || metaKey;
      let delta = 0;
      let shouldPrevent = true;

      switch (key) {
        case "ArrowLeft":
          delta = isHorizontal ? -1 : 0;
          break;
        case "ArrowRight":
          delta = isHorizontal ? 1 : 0;
          break;
        case "ArrowUp":
          delta = isHorizontal ? 0 : -1;
          break;
        case "ArrowDown":
          delta = isHorizontal ? 0 : 1;
          break;
        case "Home":
          updateSize(minSize);
          break;
        case "End":
          updateSize(maxSize);
          break;
        case "Enter":
        case " ":
          // Reset to default size
          updateSize(defaultSize);
          break;
        default:
          shouldPrevent = false;
      }

      if (shouldPrevent) {
        e.preventDefault();
      }

      if (delta !== 0) {
        const step = modifier ? 10 : 1;
        const newSize = size + delta * step;
        updateSize(newSize);
      }
    },
    [disabled, isHorizontal, size, updateSize, minSize, maxSize, defaultSize]
  );

  // Set up event listeners
  useEffect(() => {
    if (isDragging) {
      document.addEventListener("mousemove", handleMouseMove);
      document.addEventListener("mouseup", handleEnd);
      document.addEventListener("touchmove", handleTouchMove, { passive: false });
      document.addEventListener("touchend", handleEnd);

      return () => {
        document.removeEventListener("mousemove", handleMouseMove);
        document.removeEventListener("mouseup", handleEnd);
        document.removeEventListener("touchmove", handleTouchMove);
        document.removeEventListener("touchend", handleEnd);
      };
    }
  }, [isDragging, handleMouseMove, handleTouchMove, handleEnd]);

  // Handle window resize
  useEffect(() => {
    const handleResize = () => {
      // Force a re-render to recalculate percentages
      onResize?.(size);
    };

    window.addEventListener("resize", handleResize);
    return () => window.removeEventListener("resize", handleResize);
  }, [size, onResize]);

  const containerClassName = `${styles.container} ${styles[direction]} ${className || ""}`;
  const resizerClasses = `${styles.resizer} ${isDragging ? styles.dragging : ""} ${disabled ? styles.disabled : ""} ${resizerClassName || ""}`;

  return (
    <Box ref={containerRef} className={containerClassName} data-direction={direction}>
      <div
        className={styles.panel}
        style={{
          [isHorizontal ? "width" : "height"]: `${size}%`,
        }}
      >
        {children[0]}
      </div>

      <div
        ref={resizerRef}
        className={resizerClasses}
        onMouseDown={handleMouseDown}
        onTouchStart={handleTouchStart}
        onKeyDown={handleKeyDown}
        tabIndex={disabled ? -1 : 0}
        role="separator"
        aria-orientation={isHorizontal ? "vertical" : "horizontal"}
        aria-valuenow={Math.round(size)}
        aria-valuemin={minSize}
        aria-valuemax={maxSize}
        aria-label={`Resize ${direction} splitter`}
      >
        <div className={styles.resizerHandle} />
      </div>

      <div
        className={styles.panel}
        style={{
          [isHorizontal ? "width" : "height"]: `${100 - size}%`,
        }}
      >
        {children[1]}
      </div>
    </Box>
  );
};
