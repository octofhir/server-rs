import {
  Button,
  DataTable,
  type DataTableColumn,
  EmptyState,
  notify,
  ReferenceLink,
  StatusBadge,
  type StatusTone,
} from "@octofhir/ui-kit";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import dayjs from "dayjs";
import relativeTime from "dayjs/plugin/relativeTime";
import { BellRing, FileJson, Power, Send } from "lucide-react";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import {
  channelOf,
  type EventLogEntry,
  fetchEventLog,
  listSubscriptions,
  listTopics,
  ping,
  type SubscriptionResource,
  setEnabled,
  type TopicResource,
  topicUrl,
} from "../lib/api";
import classes from "./SubscriptionsPage.module.css";

dayjs.extend(relativeTime);

const LIST_KEY = ["subscriptions", "list"] as const;
const LOG_POLL_MS = 3000;

const STATUS_TONE: Record<string, StatusTone> = {
  active: "success",
  requested: "info",
  error: "danger",
  off: "neutral",
  "entered-in-error": "neutral",
};

const DELIVERY_TONE: Record<EventLogEntry["status"], StatusTone> = {
  delivered: "success",
  pending: "warning",
  processing: "info",
  failed: "danger",
  expired: "neutral",
};

const EVENT_LABELS: Record<EventLogEntry["eventType"], string> = {
  handshake: "Handshake",
  heartbeat: "Heartbeat",
  "event-notification": "Notification",
};

function ago(at: string | null | undefined): string {
  return at ? dayjs(at).fromNow() : "—";
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : "The server rejected the request.";
}

const SUBSCRIPTION_COLUMNS: DataTableColumn<SubscriptionResource>[] = [
  {
    id: "status",
    header: "Status",
    width: 104,
    cell: (subscription) => (
      <StatusBadge tone={STATUS_TONE[subscription.status ?? ""] ?? "neutral"}>{subscription.status ?? "—"}</StatusBadge>
    ),
  },
  {
    id: "reason",
    header: "Subscription",
    cell: (subscription) => (
      <span className={classes.stack}>
        <span>{subscription.reason ?? subscription.id}</span>
        {subscription.status === "error" && subscription.error && (
          <span className={classes.error}>{subscription.error}</span>
        )}
      </span>
    ),
  },
  {
    id: "topic",
    header: "Topic",
    width: 320,
    cell: (subscription) => <span className={classes.mono}>{topicUrl(subscription) || "—"}</span>,
  },
  {
    id: "channel",
    header: "Channel",
    width: 300,
    cell: (subscription) => {
      const channel = channelOf(subscription);
      return (
        <span className={classes.channel}>
          <span className={classes.channelType}>{channel.type}</span>
          {channel.endpoint && <span className={classes.mono}>{channel.endpoint}</span>}
        </span>
      );
    },
  },
];

function EventLogPanel({ subscription }: { subscription: SubscriptionResource }) {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const id = subscription.id ?? "";
  const log = useQuery({
    queryKey: ["subscriptions", "events", id],
    queryFn: () => fetchEventLog(id),
    refetchInterval: LOG_POLL_MS,
  });
  const test = useMutation({
    mutationFn: () => ping(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["subscriptions", "events", id] }),
    onError: (error) => notify({ theme: "danger", title: "Test not sent", content: errorText(error) }),
  });
  const toggle = useMutation({
    mutationFn: (enabled: boolean) => setEnabled(subscription, enabled),
    onSuccess: () => {
      // Activation happens right after the write; give the server a moment before re-reading.
      setTimeout(() => void queryClient.invalidateQueries({ queryKey: LIST_KEY }), 600);
      void queryClient.invalidateQueries({ queryKey: LIST_KEY });
    },
    onError: (error) => notify({ theme: "danger", title: "Status not changed", content: errorText(error) }),
  });
  const active = subscription.status === "active";
  const enabled = subscription.status !== "off";

  const columns: DataTableColumn<EventLogEntry>[] = [
    { id: "number", header: "#", width: 56, cell: (event) => <span className={classes.mono}>{event.eventNumber}</span> },
    { id: "type", header: "Event", width: 120, cell: (event) => EVENT_LABELS[event.eventType] },
    {
      id: "focus",
      header: "Focus",
      width: 280,
      cell: (event) =>
        event.focusResourceType && event.focusResourceId ? (
          <span className={classes.focus}>
            <ReferenceLink
              reference={`${event.focusResourceType}/${event.focusResourceId}`}
              href={`${import.meta.env.BASE_URL}resources/${event.focusResourceType}/${event.focusResourceId}`}
              onClick={(click) => {
                click.preventDefault();
                navigate(`/resources/${event.focusResourceType}/${event.focusResourceId}`);
              }}
            />
            {event.focusEvent && <span className={classes.muted}>{event.focusEvent}</span>}
          </span>
        ) : (
          <span className={classes.muted}>—</span>
        ),
    },
    {
      id: "status",
      header: "Delivery",
      width: 150,
      cell: (event) => (
        <span className={classes.delivery}>
          <StatusBadge tone={DELIVERY_TONE[event.status]}>{event.status}</StatusBadge>
          {event.attempts > 1 && <span className={classes.muted}>×{event.attempts}</span>}
        </span>
      ),
    },
    {
      id: "http",
      header: "HTTP",
      width: 64,
      align: "right",
      cell: (event) => <span className={classes.mono}>{event.httpStatus ?? "—"}</span>,
    },
    {
      id: "time",
      header: "Time",
      width: 72,
      align: "right",
      cell: (event) => <span className={classes.mono}>{event.responseTimeMs != null ? `${event.responseTimeMs} ms` : "—"}</span>,
    },
    {
      id: "when",
      header: "When",
      width: 130,
      cell: (event) => (
        <span className={classes.muted} title={new Date(event.createdAt).toLocaleString()}>
          {ago(event.createdAt)}
        </span>
      ),
    },
    {
      id: "error",
      header: "Error",
      cell: (event) =>
        event.error ? (
          <span className={classes.error} title={event.error}>
            {event.status === "pending" && event.nextRetryAt ? `Retry ${ago(event.nextRetryAt)} · ` : ""}
            {event.error}
          </span>
        ) : (
          <span className={classes.muted}>—</span>
        ),
    },
  ];

  return (
    <section className={classes.panel} aria-label="Subscription activity">
      <header className={classes.panelHeader}>
        <h2 className={classes.panelTitle}>{subscription.reason ?? `Subscription/${id}`}</h2>
        <div className={classes.actions}>
          <Button
            size="sm"
            variant="outline"
            disabled={!active}
            loading={test.isPending}
            onClick={() => test.mutate()}
            title={active ? "Queue a heartbeat to this subscription's channel" : "Only active subscriptions are notified"}
          >
            <Button.Icon side="left">
              <Send size={13} />
            </Button.Icon>
            Send test
          </Button>
          <Button size="sm" variant="outline" loading={toggle.isPending} onClick={() => toggle.mutate(!enabled)}>
            <Button.Icon side="left">
              <Power size={13} />
            </Button.Icon>
            {enabled ? "Turn off" : "Turn on"}
          </Button>
          <Button size="sm" variant="subtle" onClick={() => navigate(`/resources/Subscription/${id}`)}>
            <Button.Icon side="left">
              <FileJson size={13} />
            </Button.Icon>
            JSON
          </Button>
        </div>
      </header>

      <dl className={classes.stats}>
        <div>
          <dt>Events</dt>
          <dd>{log.data?.eventsSinceSubscriptionStart.toLocaleString() ?? "—"}</dd>
        </div>
        <div>
          <dt>Last delivery</dt>
          <dd>{ago(log.data?.lastDeliveryAt)}</dd>
        </div>
        <div>
          <dt>Failed events</dt>
          <dd>{log.data?.errorCount.toLocaleString() ?? "—"}</dd>
        </div>
        <div className={classes.wide}>
          <dt>Last error</dt>
          <dd className={log.data?.lastErrorMessage ? classes.error : undefined}>
            {log.data?.lastErrorMessage
              ? `${log.data.lastErrorMessage} (${ago(log.data.lastErrorAt)})`
              : "None"}
          </dd>
        </div>
      </dl>

      <DataTable
        aria-label="Notifications"
        data={log.data?.events ?? []}
        columns={columns}
        getRowId={(event) => event.id}
        loading={log.isPending}
        loadingRowCount={4}
        maxHeight={420}
        emptyState={
          active
            ? "No notifications yet. Change a resource the topic watches, or send a test."
            : "No notifications. The subscription has to be active to receive them."
        }
      />
    </section>
  );
}

function TopicsPanel({ topics }: { topics: TopicResource[] }) {
  return (
    <section className={classes.panel} aria-label="Topics">
      <header className={classes.panelHeader}>
        <h2 className={classes.panelTitle}>Topics</h2>
        <span className={classes.muted}>{topics.length} defined · subscriptions point at a topic URL</span>
      </header>
      {topics.length === 0 ? (
        <p className={classes.muted}>No SubscriptionTopic resources yet; create one to describe what can be subscribed to.</p>
      ) : (
        <ul className={classes.topics}>
          {topics.map((topic) => (
            <li key={topic.id} className={classes.topic}>
              <StatusBadge tone={topic.status === "active" ? "success" : "neutral"}>{topic.status ?? "—"}</StatusBadge>
              <span className={classes.topicTitle}>{topic.title ?? topic.id}</span>
              <span className={classes.mono}>{topic.url}</span>
              <span className={classes.triggers}>
                {(topic.resourceTrigger ?? [])
                  .map((trigger) => `${trigger.resource ?? "?"} ${(trigger.supportedInteraction ?? []).join("/")}`)
                  .join(" · ")}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

export function SubscriptionsPage() {
  const navigate = useNavigate();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const subscriptions = useQuery({ queryKey: LIST_KEY, queryFn: listSubscriptions, refetchInterval: 10_000 });
  const topics = useQuery({ queryKey: ["subscriptions", "topics"], queryFn: listTopics });
  const rows = subscriptions.data ?? [];
  const selected = rows.find((subscription) => subscription.id === selectedId) ?? null;

  return (
    <WorkspacePageLayout
      title="Subscriptions"
      description="Who gets notified about which changes, and whether the notifications arrive."
      className="page-enter"
      actions={
        <Button variant="outline" onClick={() => navigate("/resources/Subscription")}>
          Manage resources
        </Button>
      }
    >
      <section className={classes.panel} aria-label="Subscriptions">
        <DataTable
          aria-label="Subscriptions"
          data={rows}
          columns={SUBSCRIPTION_COLUMNS}
          getRowId={(subscription) => subscription.id ?? ""}
          activeRowId={selectedId ?? undefined}
          loading={subscriptions.isPending}
          loadingRowCount={4}
          maxHeight="none"
          onRowClick={(subscription) => setSelectedId((id) => (id === subscription.id ? null : (subscription.id ?? null)))}
          emptyState={
            <EmptyState
              image={<BellRing width={32} height={32} aria-hidden="true" />}
              title="No subscriptions"
              description="Create a Subscription with status “requested”; the server checks its topic and channel and activates it."
            />
          }
        />
      </section>
      {selected && <EventLogPanel key={selected.id} subscription={selected} />}
      <TopicsPanel topics={topics.data ?? []} />
    </WorkspacePageLayout>
  );
}
