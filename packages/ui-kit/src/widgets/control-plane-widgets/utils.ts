import type { StatusTone } from "#/shared/ui";
import type { ControlPlaneHttpMethod } from "./types";
import classes from "./ControlPlaneWidgets.module.css";

const methodClassName: Record<ControlPlaneHttpMethod, string> = {
    GET: classes.methodGet,
    POST: classes.methodPost,
    PUT: classes.methodPut,
    PATCH: classes.methodPatch,
    DELETE: classes.methodDelete,
    HEAD: classes.methodHead,
    OPTIONS: classes.methodOptions,
};

export function getMethodClassName(method: ControlPlaneHttpMethod) {
    return [classes.method, methodClassName[method]].join(" ");
}

export function getSessionTone(status?: string): StatusTone {
    if (status === "active") return "success";
    if (status === "expired") return "warning";
    if (status === "revoked") return "danger";
    return "neutral";
}

export function getAccessTone(isPublic: boolean): StatusTone {
    return isPublic ? "warning" : "success";
}
