import { TextInput as MantineTextInput, type TextInputProps } from "@mantine/core";

export function TextInput(props: TextInputProps) {
    return <MantineTextInput {...props} />;
}

export type { TextInputProps };
