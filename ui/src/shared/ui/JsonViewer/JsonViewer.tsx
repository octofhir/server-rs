import { useMemo } from "react";
import { Code, CopyButton, ActionIcon, Group, Tooltip, ScrollArea, Box } from "@mantine/core";
import { IconCopy, IconCheck } from "@tabler/icons-react";
import classes from "./JsonViewer.module.css";

interface JsonViewerProps {
    data: unknown;
    maxHeight?: number | string;
    className?: string;
}

/**
 * JSON viewer component with syntax highlighting and copy functionality.
 */
export function JsonViewer({ data, maxHeight = 400, className }: JsonViewerProps) {
    const formattedJson = useMemo(() => {
        try {
            return JSON.stringify(data, null, 2);
        } catch {
            return String(data);
        }
    }, [data]);

    return (
        <Box className={`${classes.root} ${className || ""}`}>
            <Group className={classes.copyButton}>
                <CopyButton value={formattedJson} timeout={2000}>
                    {({ copied, copy }) => (
                        <Tooltip label={copied ? "Copied to clipboard" : "Copy JSON"} withArrow position="left">
                            <ActionIcon
                                variant="light"
                                color={copied ? "teal" : "primary"}
                                onClick={copy}
                                size="md"
                                radius="md"
                                style={{
                                    boxShadow: "var(--mantine-shadow-xs)",
                                    backdropFilter: "blur(4px)",
                                }}
                            >
                                {copied ? <IconCheck size={16} /> : <IconCopy size={16} />}
                            </ActionIcon>
                        </Tooltip>
                    )}
                </CopyButton>
            </Group>
            <ScrollArea h={maxHeight} type="auto">
                <Box p="md">
                    <Code block className={classes.code}>
                        {formattedJson}
                    </Code>
                </Box>
            </ScrollArea>
            <Box className={classes.indicator} />
        </Box>
    );
}
