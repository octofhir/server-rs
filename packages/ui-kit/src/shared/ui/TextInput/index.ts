import React, { type ReactNode } from "react";
import {
    TextInput as GravityTextInput,
    type TextInputProps as GravityTextInputProps,
} from "@gravity-ui/uikit";

export interface TextInputProps extends GravityTextInputProps {
    /** @deprecated Mantine compatibility. Use `startContent`. */
    leftSection?: ReactNode;
    /** @deprecated Mantine compatibility. Use `endContent`. */
    rightSection?: ReactNode;
}

export function TextInput({ leftSection, rightSection, startContent, endContent, ...props }: TextInputProps) {
    return React.createElement(GravityTextInput, {
        startContent: startContent ?? leftSection,
        endContent: endContent ?? rightSection,
        ...props,
    });
}
