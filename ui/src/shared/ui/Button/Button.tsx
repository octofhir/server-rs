import { forwardRef } from "react";
import { Button as KitButton, type ButtonProps } from "@octofhir/ui-kit";
import classes from "./Button.module.css";

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
    ({ className, ...props }, ref) => {
        return (
            <KitButton
                ref={ref}
                {...(props as ButtonProps)}
                className={[classes.button, className].filter(Boolean).join(" ")}
            />
        );
    },
);
Button.displayName = "Button";
