import { forwardRef } from "react";
import { Button as KitButton, type ButtonProps } from "@octofhir/ui-kit";
import classes from "./Button.module.css";

const ButtonRoot = forwardRef<HTMLButtonElement, ButtonProps>(
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
ButtonRoot.displayName = "Button";

export const Button = Object.assign(ButtonRoot, {
    Icon: KitButton.Icon,
});
