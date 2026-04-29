// === Form controls ===
export * from "./Button";
export * from "./TextInput";
export * from "./Badge";
export * from "./SegmentedRadioGroup";
export * from "./ActionIcon";
export * from "./Select";
export * from "./Checkbox";
export * from "./Switch";
export * from "./NumberInput";
export * from "./TextArea";
export * from "./PasswordInput";
export * from "./DatePicker";

// === Layout primitives ===
export * from "./Box";
export * from "./Flex";
export * from "./Grid";
export * from "./Container";
export * from "./Burger";

// === Data display ===
export * from "./Card";
export * from "./Table";
export * from "./Tabs";
export * from "./Accordion";
export * from "./Text";
export * from "./Code";
export * from "./Hotkey";
export * from "./Breadcrumbs";
export * from "./Skeleton";
export * from "./AppShell";

// === Feedback ===
export * from "./Alert";
export * from "./Spin";
export * from "./Progress";

// === Overlays ===
export * from "./Modal";
export * from "./Tooltip";
export * from "./Menu";
export * from "./Popover";
export * from "./Portal";
export * from "./Drawer";

// === Navigation ===
export * from "./Link";

// === Forms (Gravity components helpers) ===
export * from "./FormRow";

// === Forms (Gravity dialog-fields + react-final-form) ===
export * from "./Form";

// === Empty / NotFound / Error states ===
export * from "./EmptyState";

// === Data table (TanStack-driven, for grids) ===
export * from "./DataTable";

// === Tracker layout (AsideHeader-based admin shell) ===
export * from "./TrackerLayout";

// === Misc ===
export * from "./Divider";
export * from "./ThemeIcon";
export * from "./ClipboardButton";
export * from "./Collapse";
export * from "./Chip";

// =====================================================================
// === DEPRECATED legacy aliases — to be removed in a follow-up sweep ===
// =====================================================================
// These exist purely so that ~50 consumer files still compile during the
// multi-step migration to a Gravity-only API surface. Do not introduce new
// uses. The follow-up commit should migrate consumers to the canonical names
// (right-hand side) and delete this whole block.

/** @deprecated use {@link Link} */
export { Link as Anchor, type LinkProps as AnchorProps } from "./Link";
/** @deprecated use {@link Link} */
export { Link as NavLink, type LinkProps as NavLinkProps } from "./Link";
/** @deprecated use {@link TextArea} */
export { TextArea as Textarea, type TextAreaProps as TextareaProps } from "./TextArea";
/** @deprecated use {@link TextArea} */
export { TextArea as JsonInput, type TextAreaProps as JsonInputProps } from "./TextArea";
/** @deprecated use {@link Select} */
export { Select as MultiSelect, type SelectProps as MultiSelectProps } from "./Select";
/** @deprecated use {@link Select} */
export { Select as Combobox, type SelectProps as ComboboxProps } from "./Select";
/** @deprecated use {@link Select} */
export type ComboboxData = unknown;
/** @deprecated use {@link DatePicker} */
export { DatePicker as DateInput, type DatePickerProps as DateInputProps } from "./DatePicker";
/** @deprecated use {@link DatePicker} */
export { DatePicker as DateTimePicker, type DatePickerProps as DateTimePickerProps } from "./DatePicker";
/** @deprecated use {@link SegmentedRadioGroup} */
export { SegmentedRadioGroup as SegmentedControl, type SegmentedRadioGroupProps as SegmentedControlProps } from "./SegmentedRadioGroup";
/** @deprecated use {@link Grid} */
export { Grid as SimpleGrid, type GridProps as SimpleGridProps } from "./Grid";
/** @deprecated use {@link Card} */
export { Card as Paper, type CardProps as PaperProps } from "./Card";
/** @deprecated use {@link Box} */
export { Box as ScrollArea, type BoxProps as ScrollAreaProps } from "./Box";
/** @deprecated use {@link Text} with `variant="header-N"` */
export { Text as Title, type TextProps as TitleProps } from "./Text";
/** @deprecated use {@link Spin} */
export { Spin as Loader, type SpinProps as LoaderProps } from "./Spin";
/** @deprecated use {@link Spin} */
export { Spin as RingProgress, type SpinProps as RingProgressProps } from "./Spin";
/** @deprecated use {@link Spin} */
export { Spin as LoadingOverlay, type SpinProps as LoadingOverlayProps } from "./Spin";
/** @deprecated use {@link ClipboardButton} */
export { ClipboardButton as CopyButton, type ClipboardButtonProps as CopyButtonProps } from "./ClipboardButton";
/** @deprecated use {@link Button} */
export { Button as UnstyledButton, type ButtonProps as UnstyledButtonProps } from "./Button";
/** @deprecated use {@link Hotkey} with `value` prop */
export { Hotkey as Kbd, type HotkeyProps as KbdProps } from "./Hotkey";
/** @deprecated use {@link Flex} with `direction="column"` */
export { Flex as Stack, type FlexProps as StackProps } from "./Flex";
/** @deprecated use {@link Flex} */
export { Flex as Group, type FlexProps as GroupProps } from "./Flex";
/** @deprecated use {@link Flex} with center alignment */
export { Flex as Center, type FlexProps as CenterProps } from "./Flex";
