import { TextInput as KitTextInput, type TextInputProps } from "@octofhir/ui-kit";
import classes from "./TextInput.module.css";

export function TextInput({ className, ...props }: TextInputProps) {
    return (
        <KitTextInput
            {...props}
            className={[classes.root, className].filter(Boolean).join(" ")}
        />
    );
}
