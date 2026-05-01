import { useUnit } from "effector-react";
import { Badge, Flex, Tabs, Text, Box } from "@/shared/ui";
import { $body, $customHeaders, $method } from "../state/consoleStore";
import { BodyEditor } from "./BodyEditor";
import { HeaderEditor } from "./HeaderEditor";

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
    <Box style={{ backgroundColor: "var(--g-color-base-generic-subtle)" }}>
      <Tabs defaultValue="headers">
        <Box style={{ padding: "0 20px" }}>
          <Tabs.List>
            <Tabs.Tab id="headers">
              <Flex gap="2" alignItems="center">
                <Text variant="body-2">Headers</Text>
                {customHeaderCount > 0 && (
                  <Badge size="s" theme="info" style={{ borderRadius: "50%", width: 18, height: 18, padding: 0, justifyContent: "center" }}>
                    {customHeaderCount}
                  </Badge>
                )}
              </Flex>
            </Tabs.Tab>
            <Tabs.Tab id="body">
              <Flex gap="2" alignItems="center">
                <Text variant="body-2">Body</Text>
                {bodySize > 0 && (
                  <Badge size="s" theme="warning" style={{ borderRadius: "50%", width: 18, height: 18, padding: 0, justifyContent: "center" }}>
                    1
                  </Badge>
                )}
              </Flex>
            </Tabs.Tab>
          </Tabs.List>
        </Box>

        <Box style={{ padding: "16px 20px", backgroundColor: "var(--g-color-base-background)" }}>
          <Tabs.Panel value="headers">
            <HeaderEditor />
          </Tabs.Panel>

          <Tabs.Panel value="body">
            <BodyEditor resourceType={resourceType} method={method} />
          </Tabs.Panel>
        </Box>
      </Tabs>
    </Box>
  );
}
