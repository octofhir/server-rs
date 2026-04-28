import React from "react";
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

export interface TabsProps extends Omit<TabProviderProps, "onUpdate"> {
    onChange?: (value: string) => void;
    onUpdate?: (value: string) => void;
}

const TabsComponent = ({ onChange, onUpdate, ...props }: TabsProps) => {
    return <TabProvider onUpdate={onChange || onUpdate} {...props} />;
};

export const Tabs = Object.assign(TabsComponent, {
    List: TabList,
    Tab: Tab,
    Panel: TabPanel,
});

export type { TabListProps, TabProps, TabPanelProps };
