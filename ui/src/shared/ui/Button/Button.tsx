import { Button as MantineButton, type ButtonProps, createPolymorphicComponent } from "@mantine/core";
import { forwardRef } from "react";
import classes from "./Button.module.css";

const _Button = forwardRef<HTMLButtonElement, ButtonProps>((props, ref) => {
    return <MantineButton ref={ref} {...props} classNames={classes} />;
});

export const Button = createPolymorphicComponent<"button", ButtonProps>(_Button);
