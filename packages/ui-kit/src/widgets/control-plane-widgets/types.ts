import type { ReactNode } from "react";

export type ControlPlaneHttpMethod = "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS";

export interface ControlPlaneAppReference {
    id: string;
    name: string;
}

export interface OperationCatalogItem {
    id: string;
    name: string;
    description?: ReactNode;
    category: string;
    methods: ControlPlaneHttpMethod[];
    pathPattern: string;
    public: boolean;
    module?: string;
    app?: ControlPlaneAppReference;
}

export interface AuthSessionSummary {
    id: string;
    deviceName: string;
    browserName?: string;
    ipAddress?: string;
    lastActivityLabel?: string;
    expiresLabel?: string;
    status?: "active" | "expired" | "revoked";
    current?: boolean;
}
