import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import {
    Boxes,
    Database,
    FileText,
    FolderTree,
    Gauge,
    KeyRound,
    Settings,
    ShieldCheck,
    Terminal,
    Users,
} from "lucide-react";
import { Badge } from "../../shared/ui/Badge";
import { Sidebar } from "./Sidebar";

const meta: Meta<typeof Sidebar> = {
    title: "Widgets/Sidebar",
    component: Sidebar,
    parameters: { layout: "fullscreen" },
};

export default meta;
type Story = StoryObj<typeof Sidebar>;

const groups = [
    {
        id: "main",
        label: "Main",
        items: [
            { id: "dashboard", label: "Dashboard", icon: Gauge, active: true },
            { id: "resources", label: "Resources", icon: FolderTree },
            { id: "console", label: "REST Console", icon: Terminal },
            { id: "packages", label: "Packages", icon: Boxes, badge: <Badge size="xs" color="primary">3</Badge> },
        ],
    },
    {
        id: "auth",
        label: "Auth & Security",
        items: [
            { id: "users", label: "Users", icon: Users },
            { id: "clients", label: "Clients", icon: KeyRound },
            { id: "roles", label: "Roles", icon: ShieldCheck },
        ],
    },
    {
        id: "tools",
        label: "Tools",
        items: [
            { id: "db", label: "DB Console", icon: Database },
            { id: "logs", label: "System Logs", icon: FileText },
            { id: "settings", label: "Settings", icon: Settings },
        ],
    },
];

export const Default: Story = {
    render: () => {
        const [scheme, setScheme] = useState<"light" | "dark">("light");
        return (
            <div style={{ height: "100vh", display: "flex" }}>
                <Sidebar
                    brand={{ title: "OctoFHIR" }}
                    groups={groups}
                    status={{ label: "Healthy", theme: "success" }}
                    account={{ name: "Alex Streltsov", secondary: "admin", onSignOut: () => {} }}
                    colorScheme={scheme}
                    onToggleColorScheme={() => setScheme((s) => (s === "dark" ? "light" : "dark"))}
                    persistKey="sb-story"
                />
            </div>
        );
    },
};

export const Collapsed: Story = {
    render: () => (
        <div style={{ height: "100vh", display: "flex" }}>
            <Sidebar
                brand={{ title: "OctoFHIR" }}
                groups={groups}
                status={{ label: "Healthy", theme: "success" }}
                account={{ name: "Alex Streltsov", secondary: "admin", onSignOut: () => {} }}
                defaultCollapsed
            />
        </div>
    ),
};
