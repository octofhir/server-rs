import type { AnchorHTMLAttributes } from "react";
import classes from "./ReferenceLink.module.css";

export interface ParsedFhirReference {
    resourceType?: string;
    id?: string;
}

export interface ReferenceLinkProps extends AnchorHTMLAttributes<HTMLAnchorElement> {
    reference: string;
    display?: string;
}

export function parseFhirReference(reference: string): ParsedFhirReference {
    const [resourceType, id] = reference.split("/");
    return {
        resourceType: resourceType || undefined,
        id: id || undefined,
    };
}

function PathLabel({ reference }: { reference: string }) {
    const { resourceType, id } = parseFhirReference(reference);
    if (!resourceType || !id) return <>{reference}</>;
    return (
        <>
            <span className={classes.type}>{resourceType}/</span>
            {id}
        </>
    );
}

export function ReferenceLink({ reference, display, className, title, ...props }: ReferenceLinkProps) {
    return (
        <a
            className={[classes.root, className].filter(Boolean).join(" ")}
            title={title ?? reference}
            {...props}
        >
            <span className={classes.label}>{display ?? <PathLabel reference={reference} />}</span>
        </a>
    );
}
