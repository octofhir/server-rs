import { ActionIcon, Box, Button, Group, Select, Text, TextInput, Tooltip } from "@mantine/core";
import { useForm } from "@mantine/form";
import { useDebouncedValue } from "@mantine/hooks";
import { IconFilter, IconSearch, IconX } from "@tabler/icons-react";
import { useUnit } from "effector-react";
import type React from "react";
import { useCallback, useEffect, useState } from "react";
import {
  $searchParams,
  $selectedResourceType,
  setSearchParams,
} from "@/entities/fhir";
import styles from "./ResourceSearchForm.module.css";

interface ResourceSearchFormProps {
  className?: string;
}

interface SearchFormValues {
  _text: string;
  _content: string;
  status: string;
  active: string;
  customParam: string;
  customValue: string;
}

export const ResourceSearchForm: React.FC<ResourceSearchFormProps> = ({ className }) => {
  const searchParams = useUnit($searchParams);
  const selectedResourceType = useUnit($selectedResourceType);
  const [showAdvanced, setShowAdvanced] = useState(false);

  const form = useForm<SearchFormValues>({
    initialValues: {
      _text: searchParams._text || "",
      _content: searchParams._content || "",
      status: searchParams.status || "",
      active: searchParams.active || "",
      customParam: "",
      customValue: "",
    },
  });

  const [debouncedText] = useDebouncedValue(form.values._text, 500);
  const [debouncedContent] = useDebouncedValue(form.values._content, 500);

  // Update search params when debounced values change
  useEffect(() => {
    if (debouncedText) {
      setSearchParams({ _text: debouncedText, _count: "20" });
    }
  }, [debouncedText]);

  useEffect(() => {
    if (debouncedContent) {
      setSearchParams({ _content: debouncedContent, _count: "20" });
    }
  }, [debouncedContent]);

  const handleFilterChange = useCallback(
    (field: string, value: string | null) => {
      if (value && value !== "") {
        setSearchParams({ [field]: value, _count: "20" });
      } else {
        setSearchParams({ _count: "20" });
      }
      form.setFieldValue(field as keyof SearchFormValues, value || "");
    },
    [form]
  );

  const handleCustomParameterAdd = useCallback(() => {
    const { customParam, customValue } = form.values;

    if (customParam && customValue) {
      setSearchParams({ [customParam]: customValue, _count: "20" });

      // Reset custom parameter fields
      form.setFieldValue("customParam", "");
      form.setFieldValue("customValue", "");
    }
  }, [form]);

  const handleClearAll = useCallback(() => {
    setSearchParams({ _count: "20" });

    form.setValues({
      _text: "",
      _content: "",
      status: "",
      active: "",
      customParam: "",
      customValue: "",
    });
  }, [form]);

  const hasActiveFilters = Object.keys(searchParams).some(
    (key) => key !== "_count" && searchParams[key]
  );

  // Common status options (can be customized per resource type)
  const getStatusOptions = () => {
    const commonOptions = [
      { value: "", label: "Any status" },
      { value: "active", label: "Active" },
      { value: "inactive", label: "Inactive" },
    ];

    // Add resource-specific status options
    switch (selectedResourceType) {
      case "Patient":
        return [
          { value: "", label: "Any status" },
          { value: "active", label: "Active" },
          { value: "inactive", label: "Inactive" },
          { value: "deceased", label: "Deceased" },
        ];
      case "Observation":
        return [
          { value: "", label: "Any status" },
          { value: "final", label: "Final" },
          { value: "preliminary", label: "Preliminary" },
          { value: "cancelled", label: "Cancelled" },
          { value: "entered-in-error", label: "Error" },
        ];
      default:
        return commonOptions;
    }
  };

  return (
    <Box className={`${styles.container} ${className || ""}`}>
      <Group justify="space-between" mb="sm">
        <Text size="sm" fw={500}>
          Search {selectedResourceType || "Resources"}
        </Text>
        <Group gap="xs">
          <Tooltip label="Advanced filters">
            <ActionIcon
              variant={showAdvanced ? "filled" : "subtle"}
              size="sm"
              onClick={() => setShowAdvanced(!showAdvanced)}
            >
              <IconFilter size={14} />
            </ActionIcon>
          </Tooltip>
          {hasActiveFilters && (
            <Tooltip label="Clear all filters">
              <ActionIcon size="sm" variant="subtle" color="red" onClick={handleClearAll}>
                <IconX size={14} />
              </ActionIcon>
            </Tooltip>
          )}
        </Group>
      </Group>

      <Box className={styles.searchForm}>
        <TextInput
          placeholder="Search in text fields..."
          leftSection={<IconSearch size={16} />}
          value={form.values._text}
          onChange={(event) => form.setFieldValue("_text", event.currentTarget.value)}
          mb="sm"
          size="sm"
        />

        {showAdvanced && (
          <Box className={styles.advancedFilters}>
            <TextInput
              placeholder="Search in all content..."
              label="Content Search"
              value={form.values._content}
              onChange={(event) => form.setFieldValue("_content", event.currentTarget.value)}
              size="sm"
              mb="sm"
            />

            <Group grow mb="sm">
              <Select
                label="Status"
                data={getStatusOptions()}
                value={form.values.status}
                onChange={(value) => handleFilterChange("status", value)}
                size="sm"
              />

              <Select
                label="Active"
                data={[
                  { value: "", label: "Any" },
                  { value: "true", label: "Yes" },
                  { value: "false", label: "No" },
                ]}
                value={form.values.active}
                onChange={(value) => handleFilterChange("active", value)}
                size="sm"
              />
            </Group>

            <Box className={styles.customParameter}>
              <Text size="xs" c="dimmed" mb="xs">
                Custom Parameter
              </Text>
              <Group>
                <TextInput
                  placeholder="Parameter name"
                  value={form.values.customParam}
                  onChange={(event) => form.setFieldValue("customParam", event.currentTarget.value)}
                  size="sm"
                  style={{ flex: 1 }}
                />
                <TextInput
                  placeholder="Value"
                  value={form.values.customValue}
                  onChange={(event) => form.setFieldValue("customValue", event.currentTarget.value)}
                  size="sm"
                  style={{ flex: 1 }}
                />
                <Button
                  size="sm"
                  variant="light"
                  onClick={handleCustomParameterAdd}
                  disabled={!form.values.customParam || !form.values.customValue}
                >
                  Add
                </Button>
              </Group>
            </Box>
          </Box>
        )}

        {hasActiveFilters && (
          <Box className={styles.activeFilters}>
            <Text size="xs" c="dimmed" mb="xs">
              Active Filters:
            </Text>
            <Group gap="xs">
              {Object.entries(searchParams)
                .filter(([key, value]) => key !== "_count" && value)
                .map(([key, value]) => (
                  <Box key={key} className={styles.filterTag}>
                    <Text size="xs">
                      <strong>{key}:</strong> {value}
                    </Text>
                    <ActionIcon
                      size="xs"
                      variant="subtle"
                      color="red"
                      onClick={() => handleFilterChange(key, null)}
                    >
                      <IconX size={10} />
                    </ActionIcon>
                  </Box>
                ))}
            </Group>
          </Box>
        )}
      </Box>
    </Box>
  );
};
