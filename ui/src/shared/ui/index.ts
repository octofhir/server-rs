// From ui-kit (simple re-exports)
export {
    Box, type BoxProps,
    Flex, type FlexProps,
    Grid, type GridProps,
    Text, type TextProps,
    Link, type LinkProps,
    Spin, type SpinProps,
    Divider, type DividerProps,
    Tooltip, type TooltipProps,
    Skeleton, type SkeletonProps,
    Checkbox, type CheckboxProps,
    Switch, type SwitchProps,
    NumberInput, type NumberInputProps,
    Select, type SelectProps,
    Table, type TableProps,
    Tabs, type TabsProps,
    Collapse, type CollapseProps,
    Accordion, type AccordionProps,
    Alert, type AlertProps,
    Menu, type MenuProps,
    Popover, type PopoverProps,
    Portal, type PortalProps,
    Drawer, type DrawerProps,
    AppShell, type AppShellProps,
    Breadcrumbs, type BreadcrumbsProps,
    Progress, type ProgressProps,
    ClipboardButton, type ClipboardButtonProps,
    Chip, type ChipProps,
    Container, type ContainerProps,
    Burger, type BurgerProps,
    Code, type CodeProps,
    DataPreview, type DataPreviewProps, type DataPreviewColumn, type DataPreviewRow,
    KeyValueList, type KeyValueListProps, type KeyValueListItem,
    SegmentedRadioGroup, type SegmentedRadioGroupProps,
    SegmentedRadioGroup as SegmentedControl, type SegmentedRadioGroupProps as SegmentedControlProps,
    ThemeIcon, type ThemeIconProps,
    PasswordInput, type PasswordInputProps,
    RecordList, type RecordListProps, type RecordListItem,
    SectionPanel, type SectionPanelProps,
    StatGrid, type StatGridProps,
    StatusBadge, type StatusBadgeProps, type StatusTone,

    // Legacy aliases (deprecated, to be removed)
    Stack, type StackProps,
    Group, type GroupProps,
    Center, type CenterProps,
    Spin as Loader, type SpinProps as LoaderProps,
    Link as Anchor, type LinkProps as AnchorProps,
    Link as NavLink, type LinkProps as NavLinkProps,
    Card as Paper, type CardProps as PaperProps,
    Box as ScrollArea, type BoxProps as ScrollAreaProps,
    Text as Title, type TextProps as TitleProps,
    MultiSelect, type SelectProps as MultiSelectProps,
    TextArea as Textarea, type TextAreaProps as TextareaProps,
    TextArea as JsonInput, type TextAreaProps as JsonInputProps,
    Button as UnstyledButton, type ButtonProps as UnstyledButtonProps,
    Spin as RingProgress, type SpinProps as RingProgressProps,
    Spin as LoadingOverlay, type SpinProps as LoadingOverlayProps,
    Grid as SimpleGrid, type GridProps as SimpleGridProps,
    Hotkey as Kbd, type HotkeyProps as KbdProps,
    ClipboardButton as CopyButton, type ClipboardButtonProps as CopyButtonProps,
    DatePicker as DateInput, type DatePickerProps as DateInputProps,
    DatePicker as DateTimePicker, type DatePickerProps as DateTimePickerProps,
} from "@octofhir/ui-kit";

// Custom wrappers (app-specific enrichment)
export * from "./Button";
export * from "./Badge";
export * from "./TextInput";
export * from "./ActionIcon";
export * from "./Card";
export * from "./Modal";
export * from "./ErrorBoundary";
export * from "./JsonViewer";
export * from "./utils";
