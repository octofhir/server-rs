import { useState, type FormEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Alert, Badge, Button, Card, DataPreview, EmptyState, Modal, PasswordInput, Select, Switch, Text, TextInput } from "@octofhir/ui-kit";
import { Plus } from "lucide-react";
import { serverApi } from "@/shared/api/serverApi";
import type { StorageProvider } from "@/shared/api/storageProvider";
import { WorkspacePageLayout } from "@/widgets/workspace-page";
import classes from "./StorageProvidersPage.module.css";

const queryKey = ["storage-providers"];
export function StorageProvidersPage() {
  const cache = useQueryClient();
  const providers = useQuery({ queryKey, queryFn: () => serverApi.getStorageProviders() });
  const [editing, setEditing] = useState<StorageProvider | "new" | null>(null);
  const [deleting, setDeleting] = useState<StorageProvider | null>(null);
  const remove = useMutation({
    mutationFn: (provider: StorageProvider) => serverApi.deleteStorageProvider(provider),
    onSuccess: () => { setDeleting(null); void cache.invalidateQueries({ queryKey }); },
  });
  return <WorkspacePageLayout title="Storage providers" description="Reusable S3 connections for private implementation guide packages" actions={<div className={classes.actions}><Button variant="outline" onClick={() => providers.refetch()}>Refresh</Button><Button onClick={() => setEditing("new")}><Plus size={16} /> Add provider</Button></div>}>
    <Card className={classes.card}>
      {providers.isPending ? <Text>Loading storage providers…</Text> : providers.isError ?
        <EmptyState title="Cannot load storage providers" description={providers.error.message} actions={[<Button key="retry" onClick={() => providers.refetch()}>Retry</Button>]} /> :
        !providers.data.length ? <EmptyState title="No storage providers yet" description="Connect Amazon S3 or an S3-compatible service, then reference the provider from your IG package configuration." actions={[<Button key="create" onClick={() => setEditing("new")}>Add provider</Button>]} /> :
        <DataPreview columns={[{ id: "name", label: "Provider" }, { id: "endpoint", label: "Endpoint" }, { id: "region", label: "Region" }, { id: "credentials", label: "Credentials" }, { id: "status", label: "Status" }, { id: "actions", label: "Actions" }]} rows={providers.data.map(provider => ({
          name: <div className={classes.identity}><Text><strong>{provider.name || provider.id}</strong></Text><Text variant="caption-2" color="secondary">{provider.id}</Text></div>,
          endpoint: provider.endpoint || "AWS regional endpoint", region: provider.region,
          credentials: provider.credentials.type === "stored" ? "Encrypted keys" : "Environment references",
          status: <Badge color={provider.enabled ? "green" : "gray"}>{provider.enabled ? "Enabled" : "Disabled"}</Badge>,
          actions: <div className={classes.actions}><Button size="sm" variant="subtle" onClick={() => setEditing(provider)}>Edit</Button><Button size="sm" variant="subtle" color="red" onClick={() => { remove.reset(); setDeleting(provider); }}>Delete</Button></div>,
        }))} getRowKey={(_, index) => providers.data[index].id} />}
    </Card>
    {editing && <ProviderEditor key={editing === "new" ? "new" : `${editing.id}:${editing.meta?.versionId}`} provider={editing === "new" ? null : editing} onClose={() => setEditing(null)} />}
    <Modal opened={deleting !== null} onClose={() => setDeleting(null)} title="Delete storage provider" size="md">
      <div className={classes.form}>
        <Text>Delete {deleting?.name || deleting?.id}? New operations using this connection will fail. Providers referenced by configured IG packages must be removed from that configuration first.</Text>
        {remove.isError && <Alert color="red">{remove.error.message}</Alert>}
        <div className={classes.actions}><Button variant="subtle" onClick={() => setDeleting(null)}>Cancel</Button><Button color="red" loading={remove.isPending} onClick={() => deleting && remove.mutate(deleting)}>Delete provider</Button></div>
      </div>
    </Modal>
  </WorkspacePageLayout>;
}
function ProviderEditor({ provider, onClose }: { provider: StorageProvider | null; onClose: () => void }) {
  const cache = useQueryClient();
  const [id, setId] = useState(provider?.id || "");
  const [name, setName] = useState(provider?.name || "");
  const [endpoint, setEndpoint] = useState(provider?.endpoint || "");
  const [region, setRegion] = useState(provider?.region || "us-east-1");
  const [addressing, setAddressing] = useState<StorageProvider["addressingStyle"]>(provider?.addressingStyle || "path");
  const [enabled, setEnabled] = useState(provider?.enabled ?? true);
  const [allowHttp, setAllowHttp] = useState(provider?.allowHttp ?? false);
  const [credentialType, setCredentialType] = useState(provider?.credentials.type || "environment");
  const env = provider?.credentials.type === "environment" ? provider.credentials : null;
  const [accessEnv, setAccessEnv] = useState(env?.accessKeyIdEnv || "AWS_ACCESS_KEY_ID");
  const [secretEnv, setSecretEnv] = useState(env?.secretAccessKeyEnv || "AWS_SECRET_ACCESS_KEY");
  const [tokenEnv, setTokenEnv] = useState(env?.sessionTokenEnv || "");
  const [accessKey, setAccessKey] = useState("");
  const [secretKey, setSecretKey] = useState("");
  const [token, setToken] = useState("");
  const [validation, setValidation] = useState("");
  const save = useMutation({
    mutationFn: (value: StorageProvider) => serverApi.saveStorageProvider(value, provider === null),
    onSuccess: () => { setAccessKey(""); setSecretKey(""); setToken(""); void cache.invalidateQueries({ queryKey }); onClose(); },
  });
  function submit(event: FormEvent) {
    event.preventDefault(); setValidation("");
    if (!/^[A-Za-z0-9_.-]{1,64}$/.test(id)) { setValidation("Provider ID must contain 1–64 letters, digits, dots, underscores or hyphens."); return; }
    if (credentialType === "stored" && ((!!accessKey !== !!secretKey) || (!!token && !accessKey) || (!accessKey && provider?.credentials.type !== "stored"))) { setValidation("Provide both access key ID and secret key when creating or rotating stored credentials."); return; }
    save.mutate({ resourceType: "StorageProvider", id, name: name || null, enabled, type: "s3", endpoint: endpoint || null, region, addressingStyle: addressing, allowHttp,
      credentials: credentialType === "environment" ? { type: "environment", accessKeyIdEnv: accessEnv, secretAccessKeyEnv: secretEnv, sessionTokenEnv: tokenEnv || null } : { type: "stored", ...(accessKey ? { accessKeyId: accessKey, secretAccessKey: secretKey, ...(token ? { sessionToken: token } : {}) } : {}) },
      ...(provider?.meta ? { meta: provider.meta } : {}),
    });
  }
  return <Modal opened onClose={onClose} title={provider ? "Edit storage provider" : "Add storage provider"} size="lg">
    <form className={classes.form} onSubmit={submit}>
      <div className={classes.grid}><TextInput label="Provider ID" required disabled={!!provider} value={id} onChange={setId} placeholder="private-igs" /><TextInput label="Display name" value={name} onChange={setName} /></div>
      <TextInput label="S3 endpoint" value={endpoint} onChange={setEndpoint} placeholder="https://objects.example.org" description="Leave empty for the AWS regional endpoint. Use the S3 API origin without a bucket or path." />
      <div className={classes.grid}><TextInput label="Signing region" required value={region} onChange={setRegion} placeholder="us-east-1" /><Select label="Bucket addressing" value={addressing} onChange={v => setAddressing(v === "virtual_hosted" ? "virtual_hosted" : "path")} data={[{ value: "path", label: "Path: endpoint/bucket/key" }, { value: "virtual_hosted", label: "Virtual host: bucket.endpoint/key" }]} /></div>
      <Switch label="Enabled" checked={enabled} onChange={setEnabled} />
      <Switch label="Allow HTTP for this connection" checked={allowHttp} onChange={setAllowHttp} />
      <Select label="Credentials" value={credentialType} onChange={v => { setCredentialType(v === "stored" ? "stored" : "environment"); setAccessKey(""); setSecretKey(""); setToken(""); }} data={[{ value: "environment", label: "Environment variable references" }, { value: "stored", label: "Encrypted keys in database" }]} />
      {credentialType === "environment" ? <>
        <div className={classes.grid}><TextInput label="Access key environment variable" required value={accessEnv} onChange={setAccessEnv} /><TextInput label="Secret key environment variable" required value={secretEnv} onChange={setSecretEnv} /></div>
        <TextInput label="Session token environment variable (optional)" value={tokenEnv} onChange={setTokenEnv} />
      </> : <>
        <Text variant="caption-2" color="secondary">Stored keys require the server encryption key. Existing values are never returned. Leave all fields blank to retain them; fill both key fields to rotate.</Text>
        <PasswordInput label="Access key ID" value={accessKey} onChange={setAccessKey} /><PasswordInput label="Secret access key" value={secretKey} onChange={setSecretKey} /><PasswordInput label="Session token (optional)" value={token} onChange={setToken} />
      </>}
      {(validation || save.isError) && <Alert color="red">{validation || save.error?.message}</Alert>}
      <div className={classes.actions}><Button type="button" variant="subtle" onClick={onClose}>Cancel</Button><Button type="submit" loading={save.isPending}>Save provider</Button></div>
    </form>
  </Modal>;
}
