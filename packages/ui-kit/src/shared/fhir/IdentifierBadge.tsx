import { StatusBadge } from "../ui";

export interface IdentifierBadgeProps {
    system?: string;
    value: string;
}

export function IdentifierBadge({ system, value }: IdentifierBadgeProps) {
    return (
        <StatusBadge tone="neutral" title={system ? `${system}|${value}` : value}>
            {value}
        </StatusBadge>
    );
}
