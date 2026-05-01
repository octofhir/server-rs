import { StatusBadge } from "../ui";

export interface CodingBadgeProps {
    system?: string;
    code: string;
    display?: string;
}

export function CodingBadge({ system, code, display }: CodingBadgeProps) {
    return (
        <StatusBadge tone="info" title={system ? `${system}|${code}` : code}>
            {display ?? code}
        </StatusBadge>
    );
}
