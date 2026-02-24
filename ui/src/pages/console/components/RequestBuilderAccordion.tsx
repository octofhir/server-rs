import { useUnit } from "effector-react";
import { Badge, Group, Tabs, Text } from "@/shared/ui";
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
    <Tabs defaultValue={null} variant="outline">
      <Tabs.List>
        <Tabs.Tab value="headers">
          <Group gap={6}>
            <Text size="sm">Headers</Text>
            {customHeaderCount > 0 && (
              <Badge size="xs" variant="light" circle>
                {customHeaderCount}
              </Badge>
            )}
          </Group>
        </Tabs.Tab>
        <Tabs.Tab value="body">
          <Group gap={6}>
            <Text size="sm">Body</Text>
            {bodySize > 0 && (
              <Badge size="xs" variant="light" circle>
                1
              </Badge>
            )}
          </Group>
        </Tabs.Tab>
      </Tabs.List>

      <Tabs.Panel value="headers" pt="md">
        <HeaderEditor />
      </Tabs.Panel>

      <Tabs.Panel value="body" pt="md">
        <BodyEditor resourceType={resourceType} method={method} />
      </Tabs.Panel>
    </Tabs>
  );
}
