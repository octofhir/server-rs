import { forwardRef } from "react";
import {
    PlaceholderContainer,
    type PlaceholderContainerProps,
} from "@gravity-ui/uikit";

/**
 * Generic empty / not-found / error state.
 *
 * Built on top of Gravity UI `PlaceholderContainer`. Provide an `image`
 * (decorative ReactNode or `{src, alt}`), a `title`, optional `description`
 * and `actions` (CTAs). Use `size="promo"` for full-page states.
 */
export type EmptyStateProps = PlaceholderContainerProps;

export const EmptyState = forwardRef<HTMLDivElement, EmptyStateProps>(
    function EmptyState(props, _ref) {
        return <PlaceholderContainer {...props} />;
    },
);
