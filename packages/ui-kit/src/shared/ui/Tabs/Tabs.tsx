import React, { useState } from "react";
import { 
    TabProvider, 
    TabList, 
    Tab, 
    TabPanel, 
    type TabProviderProps, 
    type TabListProps, 
    type TabProps, 
    type TabPanelProps 
} from "@gravity-ui/uikit";
import { cleanLayoutProps, getSpacingStyles } from "../layout-utils";

export interface TabsProps extends Omit<TabProviderProps, "onUpdate" | "value"> {
    value?: string | null;
    defaultValue?: string | null;
    onChange?: (value: string) => void;
    onUpdate?: (value: string) => void;
    variant?: string;
    radius?: string | number;
    h?: number | string;
    p?: number | string;
}

const TabsComponent = ({ value, defaultValue, onChange, onUpdate, children }: TabsProps) => {
    const [innerValue, setInnerValue] = useState(defaultValue ?? undefined);
    const currentValue = value ?? innerValue;

    const handleUpdate = (nextValue: string) => {
        if (value === undefined) setInnerValue(nextValue);
        onChange?.(nextValue);
        onUpdate?.(nextValue);
    };

    return (
        <TabProvider value={currentValue} onUpdate={handleUpdate}>
            {children}
        </TabProvider>
    );
};

type LegacyTabProps = TabProps & {
    children?: React.ReactNode;
    leftSection?: React.ReactNode;
    rightSection?: React.ReactNode;
};

function TabsTab({ leftSection, rightSection: _rightSection, icon, ...props }: LegacyTabProps) {
    const TabAny = Tab as unknown as React.ComponentType<Record<string, unknown>>;
    return <TabAny icon={icon ?? leftSection} {...props} />;
}

interface LegacyPanelProps extends TabPanelProps {
    h?: number | string;
    p?: number | string;
}

function TabsPanel({ h, p, style, ...props }: LegacyPanelProps) {
    const layoutProps = { h, p };
    return <TabPanel style={{ ...getSpacingStyles(layoutProps), ...style }} {...cleanLayoutProps(props)} />;
}

export const Tabs = Object.assign(TabsComponent, {
    List: TabList,
    Tab: TabsTab,
    Panel: TabsPanel,
});

export type { TabListProps, TabProps, TabPanelProps };
