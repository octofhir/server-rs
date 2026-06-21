/**
 * Form layer for the OctoFHIR console.
 *
 * Re-exports the `react-final-form` primitives (`Form`, `Field`, `FormSpy`,
 * `useForm`, `useField`, `useFormState`) so pages get a single import surface.
 * Pair with `FormRow` for label/description layout and the kit input controls.
 */

export { Field, Form, FormSpy, useField, useForm, useFormState } from "react-final-form";
export type {
    FieldInputProps,
    FieldProps,
    FieldRenderProps,
    FormProps,
    FormRenderProps,
} from "react-final-form";
export type { FormApi } from "final-form";
