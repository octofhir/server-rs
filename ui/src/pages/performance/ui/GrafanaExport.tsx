import { Alert, Button, Modal, PasswordInput, Select, Tabs, TextInput } from "@octofhir/ui-kit";
import { useMutation } from "@tanstack/react-query";
import { createEvent, createStore } from "effector";
import { useUnit } from "effector-react";
import { Download, ExternalLink, Upload } from "lucide-react";
import { useState } from "react";
import {
  type Dashboard,
  type GrafanaDatasource,
  type GrafanaFolder,
  type GrafanaTarget,
  metricsApi,
  type PushedDashboard,
} from "@/entities/metrics";
import classes from "./PerformancePage.module.css";

const URL_KEY = "octofhir.grafana.url";

/**
 * The Grafana token lives only in memory for this tab, so pushing several dashboards does not ask
 * for it again; a reload forgets it. Only the URL is remembered across sessions.
 */
const grafanaConnected = createEvent<GrafanaTarget | null>();
const $grafana = createStore<GrafanaTarget | null>(null).on(grafanaConnected, (_, target) => target);

function rememberedUrl(): string {
  try {
    return window.localStorage.getItem(URL_KEY) ?? "";
  } catch {
    return "";
  }
}

function rememberUrl(url: string): void {
  try {
    window.localStorage.setItem(URL_KEY, url);
  } catch {
    // Storage may be unavailable (private mode); the URL is a convenience only.
  }
}

function saveFile(name: string, data: unknown): void {
  const blob = new Blob([`${JSON.stringify(data, null, 2)}\n`], { type: "application/json" });
  const href = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = href;
  link.download = name;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(href);
}

function fileName(dashboard: Dashboard): string {
  const slug = (dashboard.id ?? dashboard.title).toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
  return `octofhir-${slug || "dashboard"}.grafana.json`;
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : "Request failed";
}

export function GrafanaExport({ dashboard, onClose }: { dashboard: Dashboard; onClose: () => void }) {
  const [connected, connect] = useUnit([$grafana, grafanaConnected]);
  const [tab, setTab] = useState<string>("download");
  const [url, setUrl] = useState(connected?.url ?? rememberedUrl());
  const [token, setToken] = useState(connected?.token ?? "");
  const [datasources, setDatasources] = useState<GrafanaDatasource[] | null>(null);
  const [folders, setFolders] = useState<GrafanaFolder[]>([]);
  const [datasource, setDatasource] = useState<string | null>(null);
  const [folder, setFolder] = useState<string | null>(null);
  const [pushed, setPushed] = useState<PushedDashboard[] | null>(null);

  const download = useMutation({
    mutationFn: () => metricsApi.exportGrafana(dashboard),
    onSuccess: (json) => saveFile(fileName(dashboard), json),
  });

  const lookup = useMutation({
    mutationFn: async (target: GrafanaTarget) => {
      const [found, folderList] = await Promise.all([
        metricsApi.grafanaDatasources(target),
        metricsApi.grafanaFolders(target),
      ]);
      return { found, folderList, target };
    },
    onSuccess: ({ found, folderList, target }) => {
      setDatasources(found);
      setFolders(folderList);
      setDatasource(found.find((ds) => ds.isDefault)?.uid ?? found[0]?.uid ?? null);
      connect(target);
      rememberUrl(target.url);
    },
  });

  const push = useMutation({
    mutationFn: () =>
      metricsApi.grafanaPush({
        url,
        token,
        datasourceUid: datasource ?? "",
        folderUid: folder ?? undefined,
        dashboards: [dashboard],
      }),
    onSuccess: setPushed,
  });

  return (
    <Modal open onClose={onClose} title={`Export “${dashboard.title}” to Grafana`} size="md">
      <Tabs value={tab} onChange={(value) => setTab(value ?? "download")}>
        <Tabs.List>
          <Tabs.Tab value="download" icon={<Download size={14} />}>
            Download JSON
          </Tabs.Tab>
          <Tabs.Tab value="push" icon={<Upload size={14} />}>
            Push
          </Tabs.Tab>
        </Tabs.List>
        <Tabs.Panel value="download">
          <div className={classes.exportBody}>
            <p className={classes.exportText}>
              A Grafana dashboard JSON with every query scoped to this deployment. In Grafana use{" "}
              <b>Dashboards → New → Import</b>; it asks which Prometheus datasource to use.
            </p>
            {download.error ? <Alert theme="danger" message={errorText(download.error)} /> : null}
            <div className={classes.exportActions}>
              <Button color="primary" onClick={() => download.mutate()} loading={download.isPending}>
                <Button.Icon side="left">
                  <Download size={14} />
                </Button.Icon>
                Download
              </Button>
            </div>
          </div>
        </Tabs.Panel>
        <Tabs.Panel value="push">
          <form
            className={classes.exportBody}
            onSubmit={(event) => {
              event.preventDefault();
              if (datasources && datasource) push.mutate();
              else lookup.mutate({ url, token });
            }}
          >
            <p className={classes.exportText}>
              Creates the dashboard in your Grafana, or updates it if pushed before. Use a service-account token
              with the Editor role; it is sent with this request only and never stored on the server.
            </p>
            <TextInput
              label="Grafana URL"
              value={url}
              onChange={(value) => {
                setUrl(value);
                setDatasources(null);
              }}
              placeholder="https://grafana.example.com"
              w="100%"
            />
            <PasswordInput
              label="Service-account token"
              value={token}
              onChange={(value) => {
                setToken(value);
                setDatasources(null);
              }}
              autoComplete="off"
              w="100%"
            />
            {datasources ? (
              <div className={classes.exportRow}>
                <Select
                  label="Prometheus datasource"
                  data={datasources.map((ds) => ({ value: ds.uid, label: ds.name }))}
                  value={datasource}
                  onChange={setDatasource}
                  placeholder={datasources.length === 0 ? "No Prometheus datasource" : undefined}
                  w="100%"
                />
                <Select
                  label="Folder"
                  data={[{ value: "", label: "General" }, ...folders.map((f) => ({ value: f.uid, label: f.title }))]}
                  value={folder ?? ""}
                  onChange={(value) => setFolder(value || null)}
                  w="100%"
                />
              </div>
            ) : null}
            {lookup.error ? <Alert theme="danger" message={errorText(lookup.error)} /> : null}
            {push.error ? <Alert theme="danger" message={errorText(push.error)} /> : null}
            {pushed ? (
              <Alert
                theme="success"
                message={
                  <span>
                    Pushed.{" "}
                    {pushed.map((item) => (
                      <a key={item.url} href={item.url} target="_blank" rel="noreferrer" className={classes.exportLink}>
                        Open {item.title} in Grafana <ExternalLink size={12} />
                      </a>
                    ))}
                  </span>
                }
              />
            ) : null}
            <div className={classes.exportActions}>
              {datasources ? (
                <Button type="submit" color="primary" disabled={!datasource} loading={push.isPending}>
                  <Button.Icon side="left">
                    <Upload size={14} />
                  </Button.Icon>
                  Push
                </Button>
              ) : (
                <Button type="submit" color="primary" disabled={!url.trim() || !token.trim()} loading={lookup.isPending}>
                  Connect
                </Button>
              )}
            </div>
          </form>
        </Tabs.Panel>
      </Tabs>
    </Modal>
  );
}
