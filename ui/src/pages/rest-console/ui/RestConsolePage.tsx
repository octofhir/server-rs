import { Container, Stack, Text, Title } from "@mantine/core";
import { RequestBuilder } from "@/features/rest-request-builder/ui/RequestBuilder";
import { ResponseViewer } from "@/features/rest-response-viewer/ui/ResponseViewer";

export function RestConsolePage() {
  return (
    <Container size="lg">
      <Stack gap="lg">
        <div>
          <Title order={1} size="h2" mb="xs">
            REST Console
          </Title>
          <Text c="dimmed">Test and interact with FHIR REST API endpoints</Text>
        </div>

        <Stack gap="md">
          <RequestBuilder />
          <ResponseViewer />
        </Stack>
      </Stack>
    </Container>
  );
}
