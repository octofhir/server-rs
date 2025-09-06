import { AppShell as MantineAppShell } from "@mantine/core";
import { useDisclosure, useLocalStorage } from "@mantine/hooks";
import { Header } from "./Header";
import { Sidebar } from "./Sidebar";

interface AppShellProps {
  children: React.ReactNode;
}

const HEADER_HEIGHT = 60;
const DEFAULT_SIDEBAR_WIDTH = 280;

export function AppShell({ children }: AppShellProps) {
  const [sidebarOpened] = useDisclosure(true);
  const [sidebarWidth] = useLocalStorage({
    key: "octofhir-sidebar-width",
    defaultValue: DEFAULT_SIDEBAR_WIDTH,
  });

  return (
    <MantineAppShell
      header={{ height: HEADER_HEIGHT }}
      navbar={{
        width: sidebarWidth,
        breakpoint: "sm",
        collapsed: { mobile: !sidebarOpened },
      }}
      padding="md"
    >
      <MantineAppShell.Header>
        <Header height={HEADER_HEIGHT} />
      </MantineAppShell.Header>

      <MantineAppShell.Navbar p="md">
        <Sidebar width={sidebarWidth} />
      </MantineAppShell.Navbar>

      <MantineAppShell.Main>{children}</MantineAppShell.Main>
    </MantineAppShell>
  );
}
