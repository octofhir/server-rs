import {
    Button as MantineButton,
    type ButtonProps as MantineButtonProps,
    createPolymorphicComponent,
} from "@mantine/core";
import { forwardRef } from "react";

export type OctoVariant = "primary" | "secondary" | "fire" | "ghost";

export interface OctoButtonProps {
    octoVariant?: OctoVariant;
}

declare module "@mantine/core" {
    export interface ButtonProps extends OctoButtonProps { }
}

const _Button = forwardRef<HTMLButtonElement, MantineButtonProps>(
    ({ octoVariant, ...props }, ref) => (
        <MantineButton
            ref={ref}
            {...props}
            {...(octoVariant ? { "data-octo-variant": octoVariant } : {})}
        />
    )
);

export const Button = createPolymorphicComponent<"button", MantineButtonProps>(_Button);

export type { MantineButtonProps as ButtonProps };
