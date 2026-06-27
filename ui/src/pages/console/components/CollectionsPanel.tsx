import { ActionIcon, Badge, Button, Drawer, Text, TextInput } from "@octofhir/ui-kit";
import { useUnit } from "effector-react";
import { Bookmark, Save, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import { isHttpMethod } from "@/shared/api";
import { useSavedRequests } from "../hooks/useSavedRequests";
import type { SavedRequest } from "../services/savedRequestService";
import {
  $body,
  $customHeaders,
  $method,
  $rawPath,
  setBody,
  setCustomHeaders,
  setMethod,
  setRawPath,
} from "../state/consoleStore";
import styles from "./CollectionsPanel.module.css";

interface Props {
  opened: boolean;
  onClose: () => void;
}

export function CollectionsPanel({ opened, onClose }: Props) {
  const { collections, requests, saveRequest, createCollection, deleteRequest } =
    useSavedRequests();
  const { method, rawPath, body, customHeaders } = useUnit({
    method: $method,
    rawPath: $rawPath,
    body: $body,
    customHeaders: $customHeaders,
  });
  const {
    setMethod: setMethodEvent,
    setRawPath: setRawPathEvent,
    setBody: setBodyEvent,
    setCustomHeaders: setHeadersEvent,
  } = useUnit({ setMethod, setRawPath, setBody, setCustomHeaders });

  const [saving, setSaving] = useState(false);
  const [name, setName] = useState("");
  const [collectionName, setCollectionName] = useState("");

  const collectionNameById = useMemo(
    () => new Map(collections.map((c) => [c.id, c.name])),
    [collections]
  );

  const grouped = useMemo(() => {
    const groups = new Map<string, SavedRequest[]>();
    for (const r of requests) {
      const key = r.collection ? (collectionNameById.get(r.collection) ?? "Other") : "Ungrouped";
      const arr = groups.get(key) ?? [];
      arr.push(r);
      groups.set(key, arr);
    }
    return [...groups.entries()];
  }, [requests, collectionNameById]);

  const handleSave = async () => {
    if (!name.trim()) return;
    let collectionId: string | undefined;
    if (collectionName.trim()) {
      const existing = collections.find(
        (c) => c.name.toLowerCase() === collectionName.trim().toLowerCase()
      );
      collectionId = existing
        ? existing.id
        : (await createCollection({ name: collectionName.trim() })).id;
    }
    await saveRequest({
      name: name.trim(),
      collection: collectionId,
      method,
      path: rawPath,
      body,
      headers: customHeaders,
    });
    setName("");
    setCollectionName("");
    setSaving(false);
  };

  const restore = (r: SavedRequest) => {
    setMethodEvent(isHttpMethod(r.method) ? r.method : "GET");
    setRawPathEvent(r.path);
    if (r.body) setBodyEvent(r.body);
    if (r.headers) setHeadersEvent(r.headers);
    onClose();
  };

  return (
    <Drawer
      open={opened}
      onOpenChange={(next) => !next && onClose()}
      placement="right"
      size={460}
      title="Saved Requests"
    >
      <div className={styles.root}>
        {saving ? (
          <div className={styles.saveForm}>
            <TextInput placeholder="Request name" value={name} onChange={setName} />
            <TextInput
              placeholder="Collection (optional)"
              value={collectionName}
              onChange={setCollectionName}
            />
            <div className={styles.saveActions}>
              <Button size="sm" variant="subtle" onClick={() => setSaving(false)}>
                Cancel
              </Button>
              <Button size="sm" variant="filled" onClick={handleSave} disabled={!name.trim()}>
                Save
              </Button>
            </div>
          </div>
        ) : (
          <Button size="sm" variant="light" onClick={() => setSaving(true)}>
            <Button.Icon>
              <Save size={14} />
            </Button.Icon>
            Save current request
          </Button>
        )}

        <div className={styles.list}>
          {grouped.length === 0 ? (
            <div className={styles.empty}>
              <Bookmark size={28} className={styles.emptyIcon} />
              <Text size="sm" c="dimmed">
                No saved requests yet.
              </Text>
            </div>
          ) : (
            grouped.map(([group, items]) => (
              <div key={group} className={styles.group}>
                <Text className={styles.groupLabel}>{group}</Text>
                {items.map((r) => (
                  <div key={r.id} className={styles.item}>
                    <button type="button" className={styles.itemMain} onClick={() => restore(r)}>
                      <div className={styles.itemHead}>
                        <Badge size="sm" variant="light">
                          {r.method}
                        </Badge>
                        <span className={styles.itemName}>{r.name}</span>
                      </div>
                      <span className={styles.itemPath}>{r.path}</span>
                    </button>
                    <ActionIcon
                      size="sm"
                      variant="subtle"
                      onClick={() => deleteRequest(r.id)}
                      aria-label="Delete"
                    >
                      <Trash2 size={14} />
                    </ActionIcon>
                  </div>
                ))}
              </div>
            ))
          )}
        </div>
      </div>
    </Drawer>
  );
}
