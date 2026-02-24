import { TextInput as MantineTextInput, type TextInputProps } from "@octofhir/ui-kit";
import classes from "./TextInput.module.css";

export function TextInput(props: TextInputProps) {
    return <MantineTextInput {...props} classNames={classes} />;
}
