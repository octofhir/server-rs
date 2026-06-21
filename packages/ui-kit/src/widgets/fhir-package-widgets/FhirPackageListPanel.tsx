import type { ReactNode } from "react";
import { Button, SectionPanel, StatusBadge } from "#/shared/ui";
import classes from "./FhirPackageWidgets.module.css";
import type { FhirPackageSummary } from "./types";

export interface FhirPackageListPanelProps {
    title?: ReactNode;
    description?: ReactNode;
    packages: FhirPackageSummary[];
    selectedPackageId?: string;
    emptyText?: string;
    onSelectPackage?: (pkg: FhirPackageSummary) => void;
    onViewPackage?: (pkg: FhirPackageSummary) => void;
}

function getPackageId(pkg: FhirPackageSummary) {
    return pkg.id ?? `${pkg.name}@${pkg.version}`;
}

export function FhirPackageListPanel({
    title = "Installed packages",
    description,
    packages,
    selectedPackageId,
    emptyText = "No packages installed",
    onSelectPackage,
    onViewPackage,
}: FhirPackageListPanelProps) {
    return (
        <SectionPanel
            title={title}
            description={description}
            actions={<StatusBadge tone="info">{packages.length.toLocaleString()} installed</StatusBadge>}
            view="outlined"
            padding="m"
        >
            {packages.length ? (
                <div className={classes.packageList}>
                    {packages.map((pkg) => {
                        const id = getPackageId(pkg);
                        const Element = onSelectPackage ? "button" : "div";

                        return (
                            <Element
                                key={id}
                                type={onSelectPackage ? "button" : undefined}
                                className={[
                                    classes.packageItem,
                                    onSelectPackage ? classes.packageItemButton : undefined,
                                    id === selectedPackageId ? classes.packageItemSelected : undefined,
                                ]
                                    .filter(Boolean)
                                    .join(" ")}
                                onClick={onSelectPackage ? () => onSelectPackage(pkg) : undefined}
                            >
                                <div className={classes.packageTop}>
                                    <div className={classes.titleBlock}>
                                        <div className={classes.titleRow}>
                                            <span className={classes.name}>{pkg.name}</span>
                                            <StatusBadge tone="neutral">v{pkg.version}</StatusBadge>
                                            <StatusBadge tone={pkg.isCompatible === false ? "warning" : "success"}>
                                                FHIR {pkg.fhirVersion ?? "unknown"}
                                            </StatusBadge>
                                        </div>
                                        {pkg.description ? (
                                            <div className={classes.description}>{pkg.description}</div>
                                        ) : null}
                                        <div className={classes.metaRow}>
                                            {pkg.installedAt ? (
                                                <StatusBadge tone="neutral">Installed {pkg.installedAt}</StatusBadge>
                                            ) : null}
                                            {pkg.isCompatible === false ? (
                                                <StatusBadge tone="warning">Version mismatch</StatusBadge>
                                            ) : null}
                                        </div>
                                    </div>
                                    <div>
                                        <div className={classes.count}>
                                            {(pkg.resourceCount ?? 0).toLocaleString()}
                                        </div>
                                        <div className={classes.caption}>resources</div>
                                    </div>
                                </div>
                                {onViewPackage ? (
                                    <div className={classes.actions}>
                                        <Button
                                            size="sm"
                                            variant="subtle"
                                            onClick={(event) => {
                                                event.stopPropagation();
                                                onViewPackage(pkg);
                                            }}
                                        >
                                            Open package
                                        </Button>
                                    </div>
                                ) : null}
                            </Element>
                        );
                    })}
                </div>
            ) : (
                <div className={classes.empty}>{emptyText}</div>
            )}
        </SectionPanel>
    );
}
