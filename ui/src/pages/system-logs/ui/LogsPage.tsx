import { createSignal, For, Show } from "solid-js";
import styles from "./LogsPage.module.css";
import { Card } from "@/shared/ui/Card";
import { Input } from "@/shared/ui/Input";
import { Select } from "@/shared/ui/Select";
import { Button } from "@/shared/ui/Button";

interface LogEntry {
    id: string;
    timestamp: string;
    level: "INFO" | "WARN" | "ERROR" | "DEBUG";
    source: string;
    message: string;
}

const mockLogs: LogEntry[] = [
    { id: "1", timestamp: "2025-11-24T10:00:01Z", level: "INFO", source: "Server", message: "Server started on port 8080" },
    { id: "2", timestamp: "2025-11-24T10:00:05Z", level: "INFO", source: "Database", message: "Connected to database" },
    { id: "3", timestamp: "2025-11-24T10:01:20Z", level: "WARN", source: "Auth", message: "Failed login attempt from 192.168.1.5" },
    { id: "4", timestamp: "2025-11-24T10:02:15Z", level: "ERROR", source: "FHIR", message: "Invalid resource format: Patient/123" },
    { id: "5", timestamp: "2025-11-24T10:05:00Z", level: "DEBUG", source: "Indexer", message: "Indexing resource Patient/124" },
];

export const LogsPage = () => {
    const [logs, setLogs] = createSignal<LogEntry[]>(mockLogs);
    const [filter, setFilter] = createSignal("");
    const [levelFilter, setLevelFilter] = createSignal("ALL");
    const [autoScroll, setAutoScroll] = createSignal(true);

    const filteredLogs = () => {
        return logs().filter(log => {
            const matchesText = log.message.toLowerCase().includes(filter().toLowerCase()) ||
                log.source.toLowerCase().includes(filter().toLowerCase());
            const matchesLevel = levelFilter() === "ALL" || log.level === levelFilter();
            return matchesText && matchesLevel;
        });
    };

    return (
        <div class={styles.container}>
            <div class={styles.header}>
                <h1 class={styles.title}>System Logs</h1>
                <div class={styles.controls}>
                    <Input
                        placeholder="Filter logs..."
                        value={filter()}
                        onInput={(e) => setFilter(e.currentTarget.value)}
                        class={styles.searchInput}
                    />
                    <Select
                        value={levelFilter()}
                        onChange={(value) => setLevelFilter(value)}
                        options={[
                            { value: "ALL", label: "All Levels" },
                            { value: "INFO", label: "Info" },
                            { value: "WARN", label: "Warn" },
                            { value: "ERROR", label: "Error" },
                            { value: "DEBUG", label: "Debug" },
                        ]}
                        class={styles.levelSelect}
                    />
                    <Button
                        variant={autoScroll() ? "primary" : "secondary"}
                        onClick={() => setAutoScroll(!autoScroll())}
                    >
                        {autoScroll() ? "Auto-scroll On" : "Auto-scroll Off"}
                    </Button>
                </div>
            </div>

            <Card class={styles.logsContainer} padding="none">
                <div class={styles.logsHeader}>
                    <div class={styles.colTimestamp}>Timestamp</div>
                    <div class={styles.colLevel}>Level</div>
                    <div class={styles.colSource}>Source</div>
                    <div class={styles.colMessage}>Message</div>
                </div>
                <div class={styles.logsList}>
                    <For each={filteredLogs()}>
                        {(log) => (
                            <div class={`${styles.logRow} ${styles[log.level.toLowerCase()]}`}>
                                <div class={styles.colTimestamp}>{new Date(log.timestamp).toLocaleTimeString()}</div>
                                <div class={styles.colLevel}>
                                    <span class={`${styles.badge} ${styles[`badge${log.level}`]}`}>{log.level}</span>
                                </div>
                                <div class={styles.colSource}>{log.source}</div>
                                <div class={styles.colMessage}>{log.message}</div>
                            </div>
                        )}
                    </For>
                    <Show when={filteredLogs().length === 0}>
                        <div class={styles.emptyState}>No logs found matching filter.</div>
                    </Show>
                </div>
            </Card>
        </div>
    );
};
