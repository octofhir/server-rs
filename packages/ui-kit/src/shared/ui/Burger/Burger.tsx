import { forwardRef } from "react";
import { Bars } from "@gravity-ui/icons";
import { Icon } from "@gravity-ui/uikit";
import { ActionIcon, type ActionIconProps } from "../ActionIcon";

export type BurgerProps = Omit<ActionIconProps, "onClick" | "children"> & {
    opened?: boolean;
    onClick?: () => void;
};

export const Burger = forwardRef<HTMLButtonElement, BurgerProps>(
    ({ onClick, ...props }, ref) => {
        return (
            <ActionIcon
                ref={ref}
                view="flat"
                onClick={onClick as ActionIconProps["onClick"]}
                {...(props as ActionIconProps)}
            >
                <Icon data={Bars} size={16} />
            </ActionIcon>
        );
    },
);
Burger.displayName = "Burger";
