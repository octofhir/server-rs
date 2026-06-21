import { forwardRef } from "react";
import { Menu as MenuIcon, X } from "lucide-react";
import { ActionIcon, type ActionIconProps } from "../ActionIcon";

export type BurgerProps = Omit<ActionIconProps, "onClick" | "children"> & {
    opened?: boolean;
    onClick?: () => void;
};

export const Burger = forwardRef<HTMLButtonElement, BurgerProps>(
    ({ opened, onClick, ...props }, ref) => {
        return (
            <ActionIcon
                ref={ref}
                variant="subtle"
                aria-label={opened ? "Close menu" : "Open menu"}
                onClick={onClick as ActionIconProps["onClick"]}
                {...(props as ActionIconProps)}
            >
                {opened ? <X size={16} /> : <MenuIcon size={16} />}
            </ActionIcon>
        );
    },
);
Burger.displayName = "Burger";
