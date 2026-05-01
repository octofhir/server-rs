import { StatusBadge } from "../ui";

export interface CapabilityFlagProps {
    label: string;
    enabled?: boolean;
}

export function CapabilityFlag({ label, enabled = false }: CapabilityFlagProps) {
    return (
        <StatusBadge tone={enabled ? "success" : "neutral"}>
            {label}: {enabled ? "On" : "Off"}
        </StatusBadge>
    );
}
