import {
    Button as MantineButton,
    type ButtonProps as MantineButtonProps,
    createPolymorphicComponent,
} from "@mantine/core";
import { forwardRef } from "react";

export interface OctoButtonProps {
    /**
     * Custom variant for OctoFHIR
     */
    octoVariant?: "primary" | "secondary" | "fire" | "ghost";
}

declare module "@mantine/core" {
    export interface ButtonProps extends OctoButtonProps { }
}

const _Button = forwardRef<HTMLButtonElement, MantineButtonProps>(
    ({ octoVariant = "primary", ...props }, ref) => (
        <MantineButton
            ref={ref}
            {...props}
            data-octo-variant={octoVariant}
        />
    )
);

export const Button = createPolymorphicComponent<"button", MantineButtonProps>(_Button);

export type { MantineButtonProps as ButtonProps };
