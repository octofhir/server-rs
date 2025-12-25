import { TextInput as MantineTextInput, type TextInputProps } from "@mantine/core";
import classes from "./TextInput.module.css";

export function TextInput(props: TextInputProps) {
    return <MantineTextInput {...props} classNames={classes} />;
}
