import React from "react";
import { Box, type BoxProps } from "../Box";
import { mapSpaceValue } from "../layout-utils";

export interface GridProps extends BoxProps {
    gutter?: number | string;
    align?: "stretch" | "center" | "flex-start" | "flex-end";
    justify?: "flex-start" | "flex-end" | "center" | "space-between" | "space-around";
}

export interface GridColProps extends BoxProps {
    span?: number | "auto";
}

export const Grid = React.forwardRef<HTMLDivElement, GridProps>(({ gutter = 16, align = "stretch", justify = "flex-start", style, ...props }, ref) => {
    const gutterVal = mapSpaceValue(gutter);
    const gutterNum = typeof gutterVal === 'number' ? gutterVal : 16;
    return (
        <Box 
            ref={ref} 
            style={{ 
                display: "flex", 
                flexWrap: "wrap", 
                margin: `-${gutterNum / 2}px`, 
                alignItems: align, 
                justifyContent: justify,
                ...style 
            }} 
            {...props} 
        />
    );
}) as React.ForwardRefExoticComponent<GridProps & React.RefAttributes<HTMLDivElement>> & { Col: React.FC<GridColProps> };

const Col = React.forwardRef<HTMLDivElement, GridColProps & { gutter?: number | string }>(({ span = 12, gutter = 16, style, ...props }, ref) => {
    const flexBasis = span === "auto" ? "auto" : `${(100 / 12) * span}%`;
    const maxWidth = span === "auto" ? "none" : `${(100 / 12) * span}%`;
    const gutterVal = mapSpaceValue(gutter);
    const gutterNum = typeof gutterVal === 'number' ? gutterVal : 16;
    return (
        <Box 
            ref={ref} 
            style={{ 
                flex: span === "auto" ? "0 0 auto" : `0 0 ${flexBasis}`, 
                maxWidth, 
                padding: `${gutterNum / 2}px`, 
                ...style 
            }} 
            {...props} 
        />
    );
});

Grid.Col = Col;
Grid.displayName = "Grid";
Col.displayName = "Grid.Col";

export interface SimpleGridProps extends Omit<BoxProps, "spacing"> {
    cols?: number;
    spacing?: number | string;
}

export const SimpleGrid = React.forwardRef<HTMLDivElement, SimpleGridProps>(({ cols = 1, spacing = 16, style, ...props }, ref) => {
    const spacingPx = mapSpaceValue(spacing);
    return (
        <Box 
            ref={ref} 
            style={{ 
                display: "grid", 
                gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`, 
                gap: spacingPx, 
                ...style 
            }} 
            {...props} 
        />
    );
});
SimpleGrid.displayName = "SimpleGrid";
