import type { ReactNode } from "react";
import { toaster } from "./toaster";

export const rem = (value: string | number) =>
    typeof value === "number" ? `${value / 16}rem` : value;
export const em = (value: string | number) =>
    typeof value === "number" ? `${value / 16}em` : value;
export const px = (value: string | number) =>
    typeof value === "number" ? value : parseFloat(value as string) * 16;

type NotificationColor = "red" | "green" | "yellow" | "blue" | "default";

const themeForColor = (
    color?: NotificationColor,
): "normal" | "danger" | "success" | "warning" | "info" => {
    switch (color) {
        case "red":
            return "danger";
        case "green":
            return "success";
        case "yellow":
            return "warning";
        case "blue":
            return "info";
        default:
            return "normal";
    }
};

const reactNodeToString = (n?: ReactNode): string | undefined => {
    if (n === null || n === undefined) return undefined;
    if (typeof n === "string") return n;
    if (typeof n === "number" || typeof n === "boolean") return String(n);
    return undefined;
};

export const notifications = {
    show: (props: {
        title?: ReactNode;
        message: ReactNode;
        color?: NotificationColor;
        autoClose?: boolean | number;
    }) => {
        toaster.add({
            name: Math.random().toString(36).substring(2, 9),
            title: reactNodeToString(props.title),
            content: props.message,
            theme: themeForColor(props.color),
            autoHiding:
                props.autoClose === false
                    ? false
                    : typeof props.autoClose === "number"
                      ? props.autoClose
                      : 5000,
        });
    },
    clean: () => toaster.removeAll(),
};

export { toaster };
export { ConfirmModalHost, modals, type OpenConfirmModalOptions } from "./confirm-modal";
