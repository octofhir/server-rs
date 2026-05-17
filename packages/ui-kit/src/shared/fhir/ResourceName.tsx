import type { HTMLAttributes } from "react";
import { Text } from "../ui";
import { ResourceTypeBadge } from "./ResourceTypeBadge";
import classes from "./ResourceName.module.css";

export interface FhirDisplayResource {
    resourceType?: string;
    id?: string;
    name?: unknown;
    title?: unknown;
    display?: unknown;
    code?: unknown;
}

export interface ResourceNameProps extends Omit<HTMLAttributes<HTMLSpanElement>, "resource"> {
    resource?: FhirDisplayResource | null;
    fallback?: string;
    showType?: boolean;
}

export function getResourceDisplayName(
    resource?: FhirDisplayResource | null,
    fallback = "Unknown resource",
): string {
    if (!resource) return fallback;

    return (
        stringValue(resource.display) ??
        stringValue(resource.title) ??
        nameValue(resource.name) ??
        stringValue(resource.code) ??
        resource.id ??
        resource.resourceType ??
        fallback
    );
}

export function ResourceName({
    resource,
    fallback,
    showType = false,
    className,
    ...props
}: ResourceNameProps) {
    const label = getResourceDisplayName(resource, fallback);

    return (
        <span className={[classes.root, className].filter(Boolean).join(" ")} {...props}>
            {showType && resource?.resourceType ? (
                <ResourceTypeBadge resourceType={resource.resourceType} />
            ) : null}
            <Text as="span" variant="body-2" className={classes.label} title={label}>
                {label}
            </Text>
        </span>
    );
}

function stringValue(value: unknown): string | undefined {
    return typeof value === "string" && value.trim() ? value : undefined;
}

function nameValue(value: unknown): string | undefined {
    const direct = stringValue(value);
    if (direct) return direct;

    if (!Array.isArray(value)) return undefined;
    return value.map(humanNameValue).find(Boolean);
}

function humanNameValue(value: unknown): string | undefined {
    if (!isRecord(value)) return undefined;

    const text = stringValue(value.text);
    if (text) return text;

    const given = Array.isArray(value.given) ? value.given.filter(isNonEmptyString).join(" ") : "";
    const family = stringValue(value.family) ?? "";
    const fullName = [given, family].filter(Boolean).join(" ").trim();

    return fullName || undefined;
}

function isRecord(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null;
}

function isNonEmptyString(value: unknown): value is string {
    return typeof value === "string" && value.trim().length > 0;
}
