import type { ReactNode } from "react";
import { Button, SectionPanel, StatusBadge } from "#/shared/ui";
import classes from "./FhirPackageWidgets.module.css";
import type { FhirPackageRegistryResult } from "./types";

export interface FhirPackageRegistryPanelProps {
    title?: ReactNode;
    description?: ReactNode;
    results: FhirPackageRegistryResult[];
    selectedPackageName?: string;
    selectedVersion?: string;
    emptyText?: string;
    installLabel?: string;
    installingPackageName?: string;
    onSelectPackage?: (pkg: FhirPackageRegistryResult, version: string) => void;
    onInstallPackage?: (pkg: FhirPackageRegistryResult, version: string) => void;
}

export function FhirPackageRegistryPanel({
    title = "Registry results",
    description,
    results,
    selectedPackageName,
    selectedVersion,
    emptyText = "No registry packages found",
    installLabel = "Install",
    installingPackageName,
    onSelectPackage,
    onInstallPackage,
}: FhirPackageRegistryPanelProps) {
    return (
        <SectionPanel
            title={title}
            description={description}
            actions={<StatusBadge tone="info">{results.length.toLocaleString()} results</StatusBadge>}
            view="outlined"
            padding="m"
        >
            {results.length ? (
                <div className={classes.registryList}>
                    {results.map((pkg) => {
                        const activeVersion = selectedPackageName === pkg.name && selectedVersion
                            ? selectedVersion
                            : pkg.latestVersion;
                        const isSelected = selectedPackageName === pkg.name;

                        return (
                            <div
                                key={pkg.name}
                                className={[
                                    classes.registryItem,
                                    isSelected ? classes.registryItemSelected : undefined,
                                ]
                                    .filter(Boolean)
                                    .join(" ")}
                            >
                                <div className={classes.registryTop}>
                                    <div className={classes.titleBlock}>
                                        <div className={classes.titleRow}>
                                            <span className={classes.name}>{pkg.name}</span>
                                            <StatusBadge tone="info">latest {pkg.latestVersion}</StatusBadge>
                                        </div>
                                        {pkg.description ? (
                                            <div className={classes.description}>{pkg.description}</div>
                                        ) : null}
                                        <div className={classes.versionStack}>
                                            {pkg.versions.slice(0, 6).map((version) => {
                                                const installed = pkg.installedVersions?.includes(version);

                                                return (
                                                    <Button
                                                        key={version}
                                                        size="sm"
                                                        variant={version === activeVersion ? "outline" : "subtle"}
                                                        color={version === activeVersion ? "primary" : undefined}
                                                        disabled={installed}
                                                        onClick={() => onSelectPackage?.(pkg, version)}
                                                    >
                                                        {version}{installed ? " installed" : ""}
                                                    </Button>
                                                );
                                            })}
                                        </div>
                                    </div>
                                    {onInstallPackage ? (
                                        <Button
                                            size="md"
                                            variant="filled"
                                            loading={installingPackageName === pkg.name}
                                            disabled={pkg.installedVersions?.includes(activeVersion)}
                                            onClick={() => onInstallPackage(pkg, activeVersion)}
                                        >
                                            {installLabel}
                                        </Button>
                                    ) : null}
                                </div>
                            </div>
                        );
                    })}
                </div>
            ) : (
                <div className={classes.empty}>{emptyText}</div>
            )}
        </SectionPanel>
    );
}
