/**
 * Gravity-blessed form layer for OctoFHIR admin UI.
 *
 * Built on top of `@gravity-ui/dialog-fields` (powered by `react-final-form`).
 * For declarative dialog/CRUD forms use `<DFDialog>` with a list of
 * `ControlField` configs and built-in field types (text, textarea, select,
 * checkbox, radio, tumbler, multiText, editableList, editableManyLists, tabs,
 * plainText, customBlock).
 *
 * For free-form pages, use the `react-final-form` primitives (`Form`, `Field`,
 * `useForm`, `useField`) directly with bare `@gravity-ui/uikit` controls and
 * `FormRow` from `@gravity-ui/components` for label/description layout.
 */

export {
    DFDialog,
    FORM_ERROR,
    registerDialogControl,
    registerDialogTabControl,
} from "@gravity-ui/dialog-fields";
export type {
    ControlField,
    DFDialogField,
    DFDialogProps,
    DFDialogTabField,
    FormApi,
    TabbedField,
} from "@gravity-ui/dialog-fields";

// Re-export react-final-form primitives so consumers get a single import surface.
export { Field, Form, FormSpy, useField, useForm, useFormState } from "react-final-form";
export type {
    FieldInputProps,
    FieldProps,
    FieldRenderProps,
    FormProps,
    FormRenderProps,
} from "react-final-form";
