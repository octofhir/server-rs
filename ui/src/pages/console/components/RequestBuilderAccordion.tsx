import { useUnit } from "effector-react";
import { Badge, Tabs, Text } from "@/shared/ui";
import { $body, $customHeaders, $method } from "../state/consoleStore";
import { BodyEditor } from "./BodyEditor";
import { HeaderEditor } from "./HeaderEditor";
import styles from "./RequestBuilderAccordion.module.css";

interface RequestOptionTabsProps {
  resourceType?: string;
}

export function RequestOptionTabs({ resourceType }: RequestOptionTabsProps) {
  const { method, customHeaders, body } = useUnit({
    method: $method,
    customHeaders: $customHeaders,
    body: $body,
  });

  const customHeaderCount = Object.keys(customHeaders).length;
  const bodySize = body.length;

  return (
    <div className={styles.root}>
      <Tabs defaultValue="headers">
        <div className={styles.tabList}>
          <Tabs.List>
            <Tabs.Tab id="headers">
              <span className={styles.tabLabel}>
                <Text variant="body-2">Headers</Text>
                {customHeaderCount > 0 && (
                  <Badge size="s" theme="info" className={styles.counter}>
                    {customHeaderCount}
                  </Badge>
                )}
              </span>
            </Tabs.Tab>
            <Tabs.Tab id="body">
              <span className={styles.tabLabel}>
                <Text variant="body-2">Body</Text>
                {bodySize > 0 && (
                  <Badge size="s" theme="warning" className={styles.counter}>
                    1
                  </Badge>
                )}
              </span>
            </Tabs.Tab>
          </Tabs.List>
        </div>

        <div className={styles.panel}>
          <Tabs.Panel value="headers">
            <HeaderEditor />
          </Tabs.Panel>

          <Tabs.Panel value="body">
            <BodyEditor resourceType={resourceType} method={method} />
          </Tabs.Panel>
        </div>
      </Tabs>
    </div>
  );
}
